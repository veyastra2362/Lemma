# ONNX 探针：OCR（M4a）与 TTS（M5a）实测

> 探针代码：`tools/onnx-probe/`（独立 crate，不进主工程依赖）。
> 这里记实测数据与结论；正式接入分别在 M4b / M5b。

## 怎么跑

模型不进仓库（`.gitignore` 已挡 `*.onnx`），放到 `data/` 下：

```powershell
# OCR
#   data/ocr-models/det.onnx         ch_PP-OCRv4_det_infer.onnx        (4.53 MB)
#   data/ocr-models/det_server.onnx  ch_PP-OCRv4_det_server_infer.onnx (108.1 MB，对照组)
#   data/ocr-models/rec.onnx         ch_PP-OCRv4_rec_infer.onnx        (10.35 MB)
#   data/ocr-models/ppocr_keys_v1.txt  (6623 行)
# TTS
#   data/tts-models/kokoro-zh-q8.onnx  Kokoro-82M-v1.1-zh 的 model_quantized.onnx (121.5 MB)

cd tools/onnx-probe
$env:PAD="0.9"; $env:REC_MAX_WIDTH="640"          # OCR（见下文，这两个参数决定精度）
cargo run --release -- "D:\Code\Lemma\data\ocr-models" "D:\Code\Lemma\data\ocr-test.png"

cargo run --release --bin tts -- "D:\Code\Lemma\data\tts-models\kokoro-zh-q8.onnx"
```

---

# OCR（M4a）

## 实测（本机 release，920×320 三行图）

| 阶段 | mobile det（4.53 MB） | server det（108 MB） |
| --- | --- | --- |
| 会话创建（det+rec） | 280 ms | 542 ms |
| det 推理 | **40 ms** | **734 ms**（18×） |
| 后处理（简化版） | 1 ms | 1 ms |
| rec（3 行） | ~48 ms | ~47 ms |
| 进程 RSS | 16 → **125 MB** | 16 → **399 MB** |
| 识别结果 | 三行全对 | 三行全对（**没有更好**） |

**结论：server 模型不值得。** 精度在干净截图上没有提升，却要 18 倍时间和 3 倍内存。

## 模型切换成本

同一进程内"换模型"= 再建一个 session：

| 切换动作 | 耗时 | 说明 |
| --- | --- | --- |
| mobile → server | 324 ms | 冷加载 108 MB |
| server → mobile | 65 ms | 文件已在系统缓存 |

真正的问题不是切换耗时，而是**峰值内存**（399 MB）。所以：
- 同一时刻只保留一套 OCR session；
- 切换做成"先释放旧的再建新的"，不要在内存里并存；
- 不要提供"server 质量档"这种选项——用户按了就得吃 399 MB。

## 三个关键结论

### 1. `tract-onnx` 走不通，用 `ort`

tract 是纯 Rust（不用分发 onnxruntime.dll，包体小），但 Paddle2ONNX 导出的模型带
`p2o.DynamicDimension.N` 这类符号维度名，tract 的 TDim 解析直接失败：

```
Parsing as TDim: `p2o.DynamicDimension.0' → Failed to parse ".DynamicDimension.0"
```

显式 `with_input_fact` 也救不回来（失败发生在图优化阶段）。改用 `ort`（ONNX Runtime 绑定，
构建期自动下载二进制），代价是随包分发 onnxruntime 动态库。

### 2. 字符表末尾必须补一个空格

模型输出类别数 6625 = 1(blank) + 6623(charset) + 1(**空格**)。
`ppocr_keys_v1.txt` 只有 6623 行，不补空格会让英文整行连成一片（`Alemmaisthecanonical...`）。

### 3. rec 宽度 640 最优；**框太紧才是精度杀手**

同一张图只改 rec 输入宽度：

| REC_MAX_WIDTH | 结果 |
| --- | --- |
| 320 | 严重截断（`Alemasteana`、`ee`） |
| **640** | 空格正常，可用 |
| 1920 | 空格又丢，耗时翻 3 倍（159 ms） |

真正的精度瓶颈在 det 的框：DB 输出的是**收缩核**，框比字形小，框高只有 21 px（字是 36 pt），
crop 进去信息不足。把外扩从 `0.3×框高` 提到 `0.9×框高`：

| PAD | 识别结果 |
| --- | --- |
| 0.3 | `深度阅读需要把原又和译又放仕一起对照` |
| **0.9** | `深度阅读需要把原文和译文放在一起对照。` |

英文同样从 `A lemma is the canonical torm ot a word` 变成 `A lemma is the canonical form of a word.`。
**所以 M4b 的重点是框（unclip），不是模型。**

## 对 M4b 的直接输入

1. 后处理按 PaddleOCR 的 DB 算法实现：阈值 → 连通域 → **最小外接矩形** →
   `distance = area × unclip_ratio / perimeter`（ratio ≈ 1.5）→ 排序合并 → 透视校正裁图。
   探针里的"轴对齐框 + 0.9×高外扩"只是近似，斜体/透视文本会失败。
2. rec 宽度以 640 为目标；超过就**切段拼接**，不要靠压缩或放大。
3. 长行、竖排、旋转文本都还没处理。
4. PP-OCRv5 目前只找到 84 + 80 MB 的 fp32 版本，要用必须先量化或找 mobile 版。

---

# TTS（M5a）

## 实测（Kokoro-82M-v1.1-zh，q8 量化，121.5 MB）

| 指标 | 数值 |
| --- | --- |
| 会话创建 | **1214 ms** |
| 进程 RSS | 11.8 → **242 MB**（空载），合成中 ~272 MB |
| 合成耗时 | 0.4 s 音频 → 356 ms；2.2 s → 1816 ms；2.5 s → 2028 ms |
| **RTF（耗时/音频时长）** | **0.82–0.89×** |

接口形状（实测，不是猜的）：

```
in  input_ids  int64  [1, N]        （音素 token，不是文字）
in  style      f32    [1, 256]      （音色向量，每个 voice 一行的 speaker embedding）
in  speed      f32    [1]
out waveform   f32    [num_samples] （24 kHz）
out duration   int64  [1]
```

## 三个结论

### 1. 内存必须隔离进程

242 MB 空载、272 MB 合成中——**远超主进程 30–60 MB 的目标**（见 `docs/PERF.md`）。
所以 TTS 要么：
- 做成**独立子进程**（按需启动、退出即回收），要么
- 只在用户显式开启朗读时加载，且用完立刻 `drop`（`ort` 释放内存不及时，这条要实测）。

在 30–60 MB 口径下，**进程内常驻 TTS 不可接受**。

### 2. RTF 0.82 意味着"勉强能跟读"

合成 1 秒音频要 0.82 秒计算，余量只有 18%。流式分句时能勉强跟上，但：
- 首句延迟 = 会话创建 1.2 s + 首块合成，用户要等 1.5 s 以上才听到声音；
- 4 线程满载时 CPU 占用会很明显。

要做"读到哪念到哪"，必须**预取下一句**，并接受第一句有 1.5 s 延迟。

### 3. 真正的门槛是**音素化**，不是模型

`input_ids` 是**音素**而不是文字。中文要经过：分词 → 拼音（带声调）→ 音素映射。
目前成熟实现都在 Python 侧（misaki / pypinyin）。纯 Rust 没有等价物，三条路：

| 方案 | 优点 | 代价 |
| --- | --- | --- |
| 打包离线"拼音→音素"映射表 + 自己实现分词（已有 CC-CEDICT 分词器可复用） | 纯 Rust、无外部依赖 | 要自己校音素表，多音字要单独处理 |
| 调 espeak-ng（DLL 或子进程） | 现成、语言多 | GPL 许可风险 + 原生依赖 |
| 把音素化放进一个小 Python sidecar | 最快跑通 | 多一个运行时，包体和启动都变重 |

**这是 M5 开工前必须先决定的事**，否则模型跑得再快也念不出中文。

## 还没测的

- OCR 在**真实截图**（深色 UI 白字、抗锯齿、非整数缩放）上的表现——只验过合成白底黑字。
- OCR 接入主进程后的实际内存增量（探针是独立进程，基线不同）。
- TTS 的音频质量与中文多音字表现（本次用任意 token 序列，只测吞吐）。
- TTS 在 `drop(session)` 后内存是否真的归还给系统。
