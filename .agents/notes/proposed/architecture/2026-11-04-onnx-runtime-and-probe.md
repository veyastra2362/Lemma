# Agent Note: ONNX 运行时选型与 OCR/TTS 探针结论

Status: proposed

## Problem

M4（端侧 OCR）与 M5（离线朗读）都要跑 ONNX 模型，开工前有三个未知数会直接决定架构：

1. 用哪个运行时？纯 Rust 的 `tract` 包体小、不需要动态库，但可能跑不了 Paddle 导出的模型；
   官方的 `ort`（ONNX Runtime）能跑，但要随包分发原生库。
2. 模型档位怎么选？PaddleOCR 有 mobile / server 两套，体积差 24 倍。
3. 内存能不能守住？项目口径是主进程常驻 30–60 MB（见 `docs/PERF.md`）。

## Proposal

1. **OCR 用 `ort`**（ONNX Runtime 绑定）；`tract` 已实测不可行。
2. **只用 mobile 模型，不提供"高精度"档**。server det 实测 734 ms / 399 MB 且精度无提升。
3. **OCR session 常驻**（125 MB 可接受），**TTS 必须进程隔离或按需加载**（空载 242 MB）。
4. **M4b 的重点是后处理（unclip）而不是模型**：框外扩 0.3→0.9 倍框高后识别从"能看"变"几乎全对"。
5. 探针代码进仓库（`tools/onnx-probe/`，独立 crate），保证结论可复现。

## Alternatives considered

- **`tract-onnx`（纯 Rust）**：最强的理由是部署——不依赖 onnxruntime 动态库，包体小十几 MB，
  也没有"加载原生库"的观感问题，符合项目"单 exe 即走"的调性。否决原因是**实测跑不了**：
  Paddle2ONNX 导出的模型带 `p2o.DynamicDimension.N` 这类符号维度名，tract 的 TDim 解析在
  图优化阶段就失败，显式 `with_input_fact` 也救不回来。要跑通得先改写 ONNX 的 dim_param，
  这种"预处理别人的模型文件"的脆弱性不该进产品。
- **OCR 提供 mobile / server 两档让用户选**：直觉上"给用户选择"更好。否决原因：实测 server
  在这类截图上精度**没有提升**，却要多花 18 倍时间、3 倍内存。给用户一个只会让软件变慢的开关
  是伪选择。
- **TTS 与主进程同进程常驻**：最简单，一次加载处处可用。否决原因：空载 242 MB、合成中 272 MB，
  把主进程口径从 30–60 MB 打到 300 MB 量级；且 `ort` 释放内存不及时，"用完 drop"还要再验。
- **TTS 用 espeak-ng 做音素化**：现成、语言多、一条命令出音素。否决原因是 GPL 许可与原生依赖
  的边界争议——本项目是 MIT，动态加载的法律边界不清晰，静态链接会传染。

## Acceptance criteria

- M4b：真实截图（深色 UI 白字、非整数缩放）上，端到端 < 300 ms，且识别准确率肉眼可接受；
  DB 后处理按官方算法实现（最小外接矩形 + `area × unclip_ratio / perimeter`），斜体与透视文本能正确定位。
- M4b：主进程 RSS 增量 < 150 MB（mobile det + rec 两套 session 常驻）。
- M5b：TTS 不进入常驻内存（子进程或按需加载），主进程 RSS 增量 < 20 MB。
- M5b 前置：先定中文音素化方案（`docs/TODO.md` #20），否则模型跑得再快也念不出中文。

## Risks

- 随包分发 onnxruntime 动态库会让安装包明显变大（约 +10–20 MB），且首次引入原生依赖，
  需要复核 `AGENTS.md` 的安全边界表述（加载正统原生库不属于"注入"，但要在规矩里写清楚）。
- `ort` 的内存释放不及时：TTS "按需加载后 drop" 是否真的把内存还给系统尚未验证；
  若不能，只能走子进程，M5 的工程量会显著增加。
- 探针只验证了合成图（白底黑字）。真实截图的抗锯齿、深色主题白字可能让 det 阈值（0.3）失效，
  需要重新标定。
- 中文音素化若选自建映射表，多音字与轻声处理会成为长期维护负担。
- PP-OCRv5 目前只找到 84 + 80 MB 的 fp32 版本；若产品后期确实需要 v5 的精度，
  必须先解决量化，否则内存口径守不住。

## Evidence

完整实测数据在 `docs/ONNX-PROBE.md`。关键几条：

```
OCR mobile : 会话 280ms | det 40ms | rec 48ms | RSS 16→125MB
OCR server : 会话 542ms | det 734ms| rec 47ms | RSS 16→399MB（精度无提升）
模型切换   : mobile→server 324ms / server→mobile 65ms（内存才是问题，不是时间）
OCR 精度   : 框外扩 0.3→0.9 倍框高后，从"能看"变为几乎全对
TTS(Kokoro zh q8): 会话 1214ms | RSS 242MB | RTF 0.82–0.89
```
