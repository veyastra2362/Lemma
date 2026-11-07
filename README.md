# Lemma · 词元

读外文时最烦的是被打断：复制、切窗口、粘贴、再切回来，一句话读完思路已经散了。

Lemma 就是为这件事做的。在任意窗口选中一段文字，双击 Ctrl，光标旁边浮出一张小卡片，上面是译文，下面是原文；如果选中的是一个词，就直接给音标、词性和释义。点释义里的词还能继续往下查，卡片一层层叠起来，Esc 一层层退。看完把鼠标挪开，卡片自己消失。

名字取自语言学里的 *lemma*（词元）——一个词所有屈折形式的词典原形。你点到的 `thresholds`、`went`，最后都归到一个词元上，再从词元继续往下走。

## 能做什么

取词走三档，按"快且无副作用"优先：

1. UIA 选区（`TextPattern.GetSelection`）——原生控件和 Chrome 都很快，实测 Chrome 选区 8 ms；
2. UIA 光标词（`RangeFromPoint`）——原生控件好用，Chromium 上不稳；
3. 模拟 Ctrl+C 读剪贴板——几乎什么软件都能用，但会临时占用剪贴板，慢几十毫秒。

默认是"选区优先，其次光标词"。哪档都不行时才落到剪贴板。

词库在本地（redb），ECDICT 77 万英文词条、CC-CEDICT 12.5 万中文词条，查一次约 2 微秒，断网也能用。句子翻译走 Google 免密接口，不需要配置；也可以换成 OpenAI 兼容接口。译文会落盘缓存，重复的段落不再走网络。

中文会先分词再查，一个词有多个切法时给候选让你点。

遇到选不中的文字——图片、扫描件、PDF、游戏界面——按 Alt+S 框一块区域，本地 OCR（PP-OCRv4 mobile）识别出来，再进同样的卡片流程。框选层每个显示器一个，避免跨屏 DPI 错位。

装好语音模型之后还能朗读，中英文都支持。读原文时逐词高亮跟着走；长文本按句合成并预取，不用等整段算完。合成跑在独立子进程里，闲着一分钟自己退出。

还有一块是「回填」：把译文直接写回网页原文的位置。浏览器不允许外部程序改页面内容（UIA 的文本范围根本没有写接口），所以这部分单独做了个扩展，装上之后卡片上才会出现那个按钮。

## 装上用

发布页里有四样东西：

- `Lemma_0.1.0_x64-setup.exe` —— 安装版，推荐
- `Lemma_0.1.0_x64-portable.zip` —— 免安装，解压双击 `lemma.exe`
- `lemma-extension-chrome.zip` —— Chrome / Edge 扩展
- `lemma-extension-firefox.xpi` —— Firefox 扩展

前两个都自带词库和 OCR 模型（约 145 MB），装完就能用。中文朗读的模型没打进包，去设置里点一下下载（86 MB），或者手动放 `tts-models.zip`。

扩展得自己加载，浏览器不允许程序代装：Chrome/Edge 解压后到 `chrome://extensions` 开开发者模式，加载已解压的扩展程序；Firefox 到 `about:debugging` 临时载入，重启会失效（未签名没法永久装）。

数据都在 `%APPDATA%\dev.lunar.lemma\`。卸载时会问要不要一起删掉（设置、词库、模型，约 270 MB），默认保留。

## 从源码跑

需要 Rust 1.85+（Windows 还要 MSVC 工具链）、Node 20+、pnpm。

```powershell
pnpm install

# 词库不进仓库，先自己下（63 MB）
New-Item -ItemType Directory -Force data | Out-Null
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/skywind3000/ECDICT/master/ecdict.csv" -OutFile data/ecdict.csv

# 构建。注意别用 cargo build --release，那样产出的 exe 是 dev 配置，窗口会白屏
pnpm tauri build --no-bundle

# 导入词库（77 万条，约 129 MB，三四秒）
.\src-tauri\target\release\lemma.exe --import-dict data\ecdict.csv

# 起
.\src-tauri\target\release\lemma.exe
```

不想装词库也能跑，只是卡片上没有释义。

出发布包用 `scripts\package-release.ps1`，会生成安装包和免安装 zip 到 `dist\`。

## 命令行

```powershell
lemma.exe                              # 启动 GUI
lemma.exe --probe                      # 光标处的 UIA 能力探针
lemma.exe --probe --point 800,400      # 探测指定屏幕坐标
lemma.exe --import-dict <csv>          # 导入 ECDICT
lemma.exe --import-cedict <txt>        # 导入 CC-CEDICT（中文分词 + 中文词义）
lemma.exe --import-zh-defs <目录>       # 导入 chinese-xinhua（中文释义，可选）
lemma.exe --lookup threshold --repeat 1000   # 查词并压测
lemma.exe --segment "当前进度：多服务商"     # 分词探针
lemma.exe --translate "<text>"         # 走设置里的 provider 翻译
lemma.exe --phonemes "<text>"          # 看音素转换结果（朗读排查）
lemma.exe --speak-wav "<text>" <out.wav>     # 合成一段音频
lemma.exe --ocr <image>                # 对图片做 OCR
lemma.exe --ocr-bench <dir>            # OCR 基准集（配同名 .txt 算 CER）
lemma.exe --capture x,y,w,h <out.png>  # 截屏幕矩形
lemma.exe --install-extension          # 写原生宿主登记（扩展用）
lemma.exe --package-extension <目录>    # 打 Chrome zip 与 Firefox xpi
lemma.exe --native-host                # Native Messaging 宿主（浏览器会自己调）
lemma.exe --help
```

## 数据放哪

- 词库：`%APPDATA%\dev.lunar.lemma\dict.redb`
- OCR 模型：`ocr-models\`（det.onnx / rec.onnx / ppocr_keys_v1.txt）
- 朗读模型：`tts-models\`（kokoro-zh-q8.onnx / tokenizer.json / zf_001.bin / cmudict.dict）
- 设置与缓存在同一目录下。设置 → 本地模型能看到装没装、一键打开目录。

模型权重不进仓库也不进安装包（`*.onnx` 在 `.gitignore` 里）。缺文件时相关入口会隐藏或明确提示缺什么，不影响取词和翻译。

## 性能

数字都带口径，完整数据见 [`docs/PERF.md`](docs/PERF.md)。

| 指标 | 实测 |
| --- | --- |
| 冷启动到托盘就绪 | 575 ms（三次中位数） |
| HUD 首帧（热键 → 出图） | 16–27 ms |
| 词库查询 | 2 µs/次（77 万条） |
| Rust 主进程 | 工作集 36 MB，私有 9.9 MB |
| 整个进程树 | 私有约 168 MB（7 个进程，6 个是 WebView2） |
| 光标移开自动收起 | 113 ms |
| OCR | 热态约 87 ms/张，冷态多 250 ms 建会话 |
| 朗读 | RTF 0.8–1.5×，首句约 6 秒（含冷启动） |
| 安装包 | 44.9 MB（含词库与 OCR 模型） |

"常驻几十 MB"只能指 Rust 主进程。整个进程树的私有内存约 170 MB，绝大部分是 WebView2 的浏览器、渲染、GPU 进程，任何 WebView 方案都有这个底噪。OCR 模型首次使用后常驻主进程（约 125 MB），朗读模型在子进程里，闲着会退出。

## 安全边界

只装一个键盘钩子，而且是只读的：`WH_KEYBOARD_LL` 仅用来识别「双击 Ctrl」和「中间有没有按别的键」，回调里把信号丢进 channel 就 `CallNextHookEx`，不吞键、不改键、不阻塞、不记按键内容。之所以要钩子，是因为 `RegisterHotKey` 只能注册组合键，表达不了"双击"这种时序动作。

不注入 DLL，不打补丁，不 hook 窗口消息。加载自家的原生依赖（`ort` 静态链接的 onnxruntime）属于正常依赖，只做推理，不碰别的进程。

不模拟键鼠。回填走的是浏览器扩展，不是假装按键，也不借道剪贴板。

词库、模型、原始语料都不进仓库。译文缓存落盘在应用数据目录，明文，含原文和译文。网络访问只有两处：翻译接口，以及构建期下载资源。

## 目录

```
src-tauri/src/
├── lib.rs        组合根：窗口、托盘、命令注册、冒烟钩子
├── cli.rs        命令行
├── commands.rs   IPC 适配层（命令名是前端契约）
├── native_host.rs  浏览器扩展的原生宿主（stdio 帧协议）
├── core/         跨平台业务逻辑，禁止 cfg(target_os) 与原生 API
│   ├── selection.rs   取词编排：三档降级 + 中文候选
│   ├── segment.rs     中文分词（逆向最大匹配）
│   ├── dict.rs        redb 词库：英文 terms + 中文 zhTerms + 汉语 zhDefs
│   ├── translate.rs   翻译路由、缓存与落盘
│   ├── ocr/           det / rec / DB 后处理 / 几何
│   ├── tts/           音素化与子进程客户端
│   ├── extension.rs   扩展安装与打包
│   ├── writeback*.rs  回填：应用 → 宿主 → 扩展
│   └── shell.rs       窗口生命周期、预热、尺寸、定位
└── platform/     OS 契约与后端
extension/        Chrome/Edge 与 Firefox 扩展源码
```

`core/` 里不出现 `cfg(target_os)`，由 `scripts/check-architecture.ps1` 卡住。

## 已知限制

- **中文不知道光标落在哪个字**：UIA 只给整段文本，所以多词时列候选让你点。
- **Firefox 没有 TextPattern**，只能走剪贴板兜底，会短暂占用剪贴板。
- **翻译要联网**，断网时句子档不可用（单词仍然查本地词库）。
- **英文朗读只认词典里的词**：生僻词、专名、变形词查不到就跳过，日志里有记录，不做规则回退。
- **中英混排的朗读一般**：英文词在中文句子里时长偏短。
- **跟读高亮是时间估算**，不是真同步，数字和标点处会偏。
- **OCR 对多栏整页会按行交错**，框一栏可以绕过；只支持水平方向的整块文本。
- **回填只对普通网页有效**：`chrome://`、PDF 阅读器、扩展商店动不了。

## 文档

| 文档 | 内容 |
| --- | --- |
| [`docs/ACCEPTANCE.md`](docs/ACCEPTANCE.md) | 手动验收清单 |
| [`docs/TODO.md`](docs/TODO.md) | 待做与欠账 |
| [`docs/PERF.md`](docs/PERF.md) | 性能口径与完整数据 |
| [`docs/EXTENSION.md`](docs/EXTENSION.md) | 浏览器扩展的方案与协议 |
| [`docs/UIA-MATRIX.md`](docs/UIA-MATRIX.md) | 各应用的 UIA 取词实测 |
| [`docs/ONNX-PROBE.md`](docs/ONNX-PROBE.md) | OCR / TTS 探针与模型来源 |
| [`docs/OCR-CALIBRATION.md`](docs/OCR-CALIBRATION.md) | OCR 精度标定 |
| [`AGENTS.md`](AGENTS.md) | 仓库规矩：命令、坑、不变式、安全边界 |

## 许可

MIT，见 [`LICENSE`](LICENSE)。
