# Lemma · 词元

> 高频深度阅读的轻量桌面辅助：**划词 → 沿词下探 → 对照 / 朗读**，全程不离开当前页面。

**当前状态：M0–M5 完成，M6（原地回填）待做。**
双击 Ctrl 取词、redb 离线词库（77 万条 / 2 µs）、卡片栈无限下探、整句整段翻译（免密在线 + 落盘缓存）、
中文分词、框选截图 OCR、图文双栏、中英文朗读（含跟读高亮）。
欠账与后续项见 [`docs/TODO.md`](docs/TODO.md)，手动验收清单见 [`docs/ACCEPTANCE.md`](docs/ACCEPTANCE.md)。

---

## 它解决什么问题

读英文长文时，查词被切成三段割裂的动作：**选中 → 切到词典 → 切回来**。切走再切回，读进去的思路就断了。

Lemma 把这三段压成一步：选中一个词（或把鼠标停在词上），双击 Ctrl，一张卡片贴在光标旁边给出释义；
点释义里的任意词继续下探，按 Esc 一层层退回来。**全程没有第二个窗口抢焦点，也没有剪贴板污染。**

名字取自语言学里的 *lemma*（词元）：一个词所有屈折形式的词典原形。这也正是它的骨架——
你点到的 `thresholds`、`went`，最终都归到一个词元上，再从词元继续走。

## 现在能做什么

| 能力 | 状态 |
| --- | --- |
| 双击 Ctrl 呼出（或自定义热键，可录制） | ✅ |
| 三档取词：UIA 选区 → UIA 光标词 → 剪贴板兜底 | ✅ |
| 离线词库：ECDICT 77 万条，redb 存储，**2 µs/次** | ✅ |
| 卡片栈无限下探 + 面包屑退层（Esc 逐层返回） | ✅ |
| HUD 按内容自适应高度（可设上限）、卡片区块可开关 | ✅ |
| 取词自动清洗首尾标点（`当前进度：` → `当前进度`） | ✅ |
| 中文分词：CC-CEDICT 12.5 万词条，多词给候选 chips | ✅ |
| 整句/整段翻译：Google 免密 / OpenAI 兼容，**译文落盘缓存**（重启后命中 0 ms） | ✅ |
| 截图对照（双栏）：图文并排 / 译文覆盖原图（气泡自动翻转）+ 点词下探，**只对截图结果开放** | ✅ |
| 框选截图 OCR：GDI 截屏 + PP-OCRv4 mobile + 纯 Rust DB 后处理 | ✅ |
| 朗读：Kokoro-82M，中英文，**子进程隔离**，跟读逐词高亮，流式分句 | ✅ |
| 本地模型管理：设置里看安装状态、一键打开目录 | ✅ |
| 原地回填（M6） | ⏳ |

## 快速开始

需要 **Rust 1.85+**（Windows 需 MSVC 工具链）、**Node 20+ / pnpm 11+**、Windows 10 2004+ / 11。

```powershell
pnpm install

# 1) 下载 ECDICT 词库（63 MB，不进仓库）
New-Item -ItemType Directory -Force data | Out-Null
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/skywind3000/ECDICT/master/ecdict.csv" -OutFile data/ecdict.csv

# 2) 构建 release（首次会编译依赖）
pnpm tauri build --no-bundle

# 3) 导入词库 → 77 万条 / 约 129 MB / 约 3 秒
.\src-tauri\target\release\lemma.exe --import-dict data\ecdict.csv

# 4) 启动，然后双击 Ctrl
.\src-tauri\target\release\lemma.exe
```

不想装词库也能跑：HUD 只显示取到的原文与句子，不显示释义。

### 可选：中文分词 / OCR / 朗读

除词库外的数据都放在 `%APPDATA%\dev.lunar.lemma\` 下，**设置 → 本地模型**里能看到装没装、
并一键打开目录（目录不存在会先建出来）。

| 能力 | 需要什么 | 怎么装 |
| --- | --- | --- |
| 中文分词 + 中文词释义 | CC-CEDICT 文本（`cedict.txt`） | `lemma.exe --import-cedict data\cedict.txt` |
| 翻译 | 无需配置（Google 免密） | 开箱可用；要更好质量可在设置里填 OpenAI 兼容接口 |
| 截图 OCR | `ocr-models\`：`det.onnx` / `rec.onnx` / `ppocr_keys_v1.txt` | 来源见 [`docs/ONNX-PROBE.md`](docs/ONNX-PROBE.md) |
| 朗读 | `tts-models\`：`kokoro-zh-q8.onnx` / `tokenizer.json` / `zf_001.bin` / `cmudict.dict` | 同上 |
| 汉语词典（中文释义） | chinese-xinhua 的 `word.json`（汉字）+ `ci.json`（词语），共 28 万条 | `lemma.exe --import-zh-defs <目录>`；实测导入 1.9 秒、词库 +1.7 MB |

模型权重不进仓库也不进安装包（`*.onnx` 已在 `.gitignore` 里）。缺文件时相关入口会隐藏或给出
"缺少模型"的明确提示，不影响取词与翻译。

## 命令行

```powershell
lemma.exe                            # 启动 GUI
lemma.exe --probe                    # 取词探针：光标处的 UIA 能力（见 docs/UIA-MATRIX.md）
lemma.exe --probe --point 800,400    # 探测指定屏幕坐标
lemma.exe --import-dict <csv>        # 导入 ECDICT 词库
lemma.exe --import-cedict <txt>      # 导入 CC-CEDICT（中文分词 + 中文词释义）
lemma.exe --lookup threshold --repeat 1000   # 查词并压测
lemma.exe --segment "当前进度：多服务商"     # 分词探针
lemma.exe --translate "<text>"       # 翻译（走设置里的 provider）
lemma.exe --ocr <image>              # 对图片做 OCR
lemma.exe --ocr-bench <dir>          # OCR 基准集（每张图配同名 .txt 参考答案，输出 CER）
lemma.exe --capture x,y,w,h <out.png>   # 截取屏幕矩形
lemma.exe --tts <text> <out.wav>     # 合成语音
lemma.exe --help
```

## 呼出方式

默认 **双击 Ctrl**：不占组合键、不和输入法冲突。320 ms 窗口，长按 Ctrl 不算，中间按过别的键会复位
（所以 `Ctrl+C` 之后紧接着按 Ctrl 不会误触）。也可以在设置里改成自定义热键（可录制）。

截图识别默认热键 **Alt+S**，可在设置里关掉（关掉后只能从托盘菜单触发）。

## 取词档位（为什么不是所有软件都一样）

| 档 | 机制 | 覆盖 | 代价 |
| --- | --- | --- | --- |
| T1 | `TextPattern.GetSelection()`（焦点元素） | 原生 Win32 / WPF / WinForms / WinUI / **Chrome** | 无。实测 Chrome 选区 **8 ms** |
| T0 | `RangeFromPoint` + `WordUnit` | 原生控件好用；**Chromium 上不稳定** | 无 |
| T2 | 模拟 Ctrl+C 读剪贴板 | 几乎一切可复制的软件（Firefox / Electron / PDF） | 污染剪贴板、多几十毫秒 |

结论写在 [`docs/UIA-MATRIX.md`](docs/UIA-MATRIX.md)：**在浏览器里"选中再双击"是最快的路径（8 ms，零副作用）**。
所以默认取词模式是「自动（选区优先，其次光标词）」——只认光标词时在 Chrome 上会频繁落到剪贴板兜底。

## 截图识别、双栏与朗读

- **截图识别**：`Alt+S` 或托盘「截图识别…」→ 拖出矩形 → 截屏 → OCR → 结果进 HUD 并自动翻译。
  每个显示器一个框选层，避免跨屏 DPI 错位。**框选一小块文字也能识别**（后处理按外扩后的框判断）。
- **截图对照（双栏）**：设置里开启后，**截图结果卡片**上出现「双栏」按钮
  （选词结果不提供——HUD 里已经是原文+译文并排，再开窗口是重复）。有截图时是**图文对照**——
  左原图 + 右译文，或「覆盖」模式把译文贴到原图对应文本框上（hover 出气泡显示完整译文）。
  没有截图时是纯文本双栏。两栏都能点词下探，`Esc` 逐层退。
- **朗读**：卡片上有**「朗读原文」与「朗读译文」两个按钮**（设置里可分别关掉）。
  读英文原文想听中文译文时就点第二个；双栏窗口头部同样有两个入口。
  中英文混排都支持；读原文时跟读逐词高亮（读译文不逐词高亮）；长文本按句合成 + 预取，首句不用等整段。

## 性能

口径与完整数据见 [`docs/PERF.md`](docs/PERF.md)。2026-11-04 实测：

| 指标 | 实测 |
| --- | --- |
| 冷启动到托盘就绪 | **634 ms** |
| **Rust 主进程**工作集 | **36.4 MB**（私有 9.3 MB） |
| 整个进程树私有内存 | **172 MB**（7 个进程，其中 6 个是 WebView2） |
| HUD 首帧（热键 → 首帧） | 15 – 27 ms |
| 词库查询 | 2.0 µs/次（77 万条） |
| OCR | 热态 ~87 ms/张（12 张基准集平均），冷态多约 250 ms 建会话 |
| 朗读 | RTF 0.8–1.5×；分句后首句延迟 5.8 s（含冷启动） |
| NSIS 安装包 | **6.19 MB**（不含词库与模型） |

> 「常驻 30–60 MB」只能指 Rust 主进程。整个进程树私有约 172 MB，96% 是 WebView2 的
> 浏览器 / 渲染 / GPU 进程——任何 WebView 方案都有这个底噪。**报数字必须带口径。**
> 按需加载的模型权重（OCR 约 125 MB、TTS 约 300 MB）不计入上表：
> TTS 关在子进程里，OCR 首次使用后常驻主进程。

## 安全边界

- **只装一个键盘钩子，且是只读的**：`WH_KEYBOARD_LL` 仅用于识别「双击 Ctrl」与「用户按了别的键」。
  回调里只把信号塞进 channel 然后 `CallNextHookEx`——**不吞键、不改键、不阻塞、不记录按键内容**。
  原因：`RegisterHotKey` 只能注册组合键，表达不了「双击」这种时序动作。
- **不注入 DLL、不打补丁、不 hook 窗口消息**。加载自家的原生依赖（`ort` 静态链接的 onnxruntime）
  属于正常依赖，与注入是两件事；它只做推理、不碰别的进程。
- **不模拟键鼠**（M6「回填」的兜底路径除外，且必须严格保存 / 恢复剪贴板全格式）。
- 词库、模型、原始语料都不进仓库。译文缓存落盘在应用数据目录（明文，含原文与译文）。
- 网络访问只有两处：翻译（Google 免密端点，或你自己配的 OpenAI 兼容接口）与构建期下载资源。

## 架构

```
src-tauri/src/
├── lib.rs          组合根（窗口、托盘、迁移、命令注册、冒烟钩子）
├── cli.rs          命令行：探针 / 导入 / 查词 / 分词 / 翻译 / OCR / 截图 / 朗读 / worker
├── commands.rs     IPC 适配层（命令名是前端契约）
├── core/           跨平台业务逻辑，禁止 cfg(target_os) 与原生 API
│   ├── selection.rs  取词编排：三档降级 + 中文候选
│   ├── segment.rs    中文分词（CC-CEDICT 词表 + 逆向最大匹配）
│   ├── dict.rs       redb 词库：英文 terms + 中文 zhTerms
│   ├── text.rs       句子切分、词边界清洗
│   ├── translate.rs + translate_cache.rs   翻译分层路由、语义缓存与落盘
│   ├── ocr/          det / rec / DB 后处理 / 几何（凸包·旋转卡壳·unclip·透视裁切）
│   ├── tts/          音素化（拼音→注音 / ARPAbet→IPA）、子进程客户端
│   ├── reader.rs     双栏：切块、图文 payload、PNG 编码
│   ├── double_tap.rs 双击 Ctrl 时序判定
│   ├── settings.rs   设置（新增字段必须 serde(default)）
│   └── shell.rs      四个窗口的生命周期、预热、尺寸、定位
└── platform/       OS 契约与后端（编译期全平台可检查）
    ├── keyboard / clipboard / uia / window / placement / screen / audio   契约
    └── windows/ …   Win32 实现（只读键盘钩子、GDI 截图、PlaySound）
```

`core/` 不出现 `cfg(target_os)`，由 `scripts/check-architecture.ps1` 强制检查。

## 已知限制

- **中文取词不知道光标落在哪个字**：UIA 只给整段文本，所以多词时列候选让你点。
- **Firefox 没有 TextPattern**，只能走剪贴板兜底（会短暂占用剪贴板）。
- **翻译依赖网络**：断网时句子档不可用（单词档仍走本地词库）。
- **英文朗读只认词典里的词**：生僻词、专名、变形词查不到时跳过（日志有记录），不做规则回退。
- **跟读高亮是时间估算**，不是真同步；数字与标点处会有偏移。
- **OCR 对多栏整页排版会按行交错**：框选一栏可规避；只支持水平方向的整块文本。
- **原地回填未做**（M6）。

## 更多文档

| 文档 | 内容 |
| --- | --- |
| [`docs/ACCEPTANCE.md`](docs/ACCEPTANCE.md) | 手动验收清单（逐项怎么点、预期是什么） |
| [`docs/TODO.md`](docs/TODO.md) | 待做与欠账 |
| [`docs/PERF.md`](docs/PERF.md) | 性能口径与完整数据 |
| [`docs/UIA-MATRIX.md`](docs/UIA-MATRIX.md) | 各应用的 UIA 取词实测矩阵 |
| [`docs/ONNX-PROBE.md`](docs/ONNX-PROBE.md) | OCR / TTS 的探针实测与模型来源 |
| [`docs/OCR-CALIBRATION.md`](docs/OCR-CALIBRATION.md) | OCR 精度标定过程（含踩过的坑） |
| [`AGENTS.md`](AGENTS.md) | 仓库规矩：命令、坑、不变式、安全边界 |

## 许可

MIT，见 [`LICENSE`](LICENSE)。
