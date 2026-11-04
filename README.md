# Lemma · 词元

> 高频深度阅读的轻量桌面辅助系统：**划词 → 沿词下探 → 回到原文**，全程不离开当前页面。

**当前状态：M3 已完成。** 双击 Ctrl 取词、redb 离线词库（77 万条 / 2µs 查询）、卡片栈无限下探、
**整句/整段翻译（免密在线，零配置）**、可配置 HUD。OCR、TTS、原地回填在 M4–M6。
欠账与后续项见 [`docs/TODO.md`](docs/TODO.md)。

---

## 它解决什么问题

读英文长文时，查词这件事被切成了三段割裂的动作：**选中 → 切到词典 → 切回来**。切走再切回，读进去的思路就断了。

Lemma 把这三段压成一步：选中一个词（或把鼠标停在词上），双击 Ctrl，一张卡片贴在光标旁边给出释义；点释义里的任意词继续下探，按 Esc 一层层退回来。**全程没有第二个窗口抢焦点，也没有剪贴板污染。**

名字取自语言学里的 *lemma*（词元）：一个词所有屈折形式的词典原形。这也正是它的骨架——你点到的 `thresholds`、`went`，最终都归到一个词元上，再从词元继续走。

## 现在能做什么

| 能力 | 状态 |
| --- | --- |
| 双击 Ctrl 呼出（或自定义热键，可录制） | ✅ |
| 三档取词：UIA 选区 → UIA 光标词 → 剪贴板兜底 | ✅ |
| 离线词库：ECDICT 77 万条，redb 存储，**2 µs/次** | ✅ |
| 卡片栈无限下探 + 面包屑退层（Esc 逐层返回） | ✅ |
| HUD 按内容自适应高度（可设上限）、卡片区块可开关 | ✅ |
| 取词自动清洗首尾标点（`当前进度：` → `当前进度`） | ✅ |
| **中文分词**：CC-CEDICT 12.5 万词条，多词给候选 chips | ✅ |
| **双栏对照阅读**：块级对齐 + 联动滚动 + 点词下探（默认关闭，设置里开启） | ✅ |
| **端侧 OCR**：PP-OCRv4 mobile + 纯 Rust DB 后处理，热态 ~160 ms（`--ocr` 可无头验证） | ✅ 管线 |
| 整句/整段翻译（Google 免密 或 OpenAI 兼容） | ✅ |
| 端侧 OCR（PP-OCRv5） | ⏳ M4 |
| 离线朗读（Kokoro） | ⏳ M5 |
| 标注层 / 原地回填 | ⏳ M6 |

## 快速开始

需要 **Rust 1.85+**（Windows 需 MSVC 工具链）、**Node 20+ / pnpm 11+**、Windows 10 2004+ / 11。

```powershell
pnpm install

# 1) 下载 ECDICT 词库（63 MB，不进仓库）
New-Item -ItemType Directory -Force data | Out-Null
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/skywind3000/ECDICT/master/ecdict.csv" -OutFile data/ecdict.csv

# 2) 构建 release（第一次会顺带编译依赖、导入词库前需先构建出 exe）
pnpm tauri build --no-bundle

# 3) 导入词库 → 77 万条 / 约 129 MB / 约 3 秒
.\src-tauri\target\release\lemma.exe --import-dict data\ecdict.csv

# 4) 启动，然后双击 Ctrl
.\src-tauri\target\release\lemma.exe
```

不想装词库也能跑：没有词库时 HUD 只显示取到的原文与句子，不显示释义。

### 命令行

```powershell
lemma.exe --probe                    # 取词探针：探测光标处的 UIA 能力（见 docs/UIA-MATRIX.md）
lemma.exe --probe --point 800,400    # 探测指定屏幕坐标
lemma.exe --import-dict <csv>        # 导入 ECDICT
lemma.exe --lookup threshold --repeat 1000   # 查词并压测
lemma.exe --help
```

## 呼出方式

默认 **双击 Ctrl**：不占组合键、不和输入法冲突。320 ms 窗口，长按 Ctrl 不算，中间按过别的键会复位（所以 `Ctrl+C` 之后紧接着按 Ctrl 不会误触）。

也可以在设置里改：

| 方式 | 说明 |
| --- | --- |
| 双击 Ctrl | 默认。需要装一个**只读**键盘钩子，见下方安全边界 |
| 自定义热键 | 设置页点「录制」按组合键。注册失败（被别的软件占用）会回滚旧配置并提示原因 |

## 取词档位（为什么不是所有软件都一样）

| 档 | 机制 | 覆盖 | 代价 |
| --- | --- | --- | --- |
| T1 | `TextPattern.GetSelection()`（焦点元素） | 原生 Win32 / WPF / WinForms / WinUI / **Chrome** | 无。实测 Chrome 选区 **8 ms** |
| T0 | `RangeFromPoint` + `WordUnit` | 原生控件好用；**Chromium 上不稳定** | 无 |
| T2 | 模拟 Ctrl+C 读剪贴板 | 几乎一切可复制的软件（Firefox / Electron / PDF） | 污染剪贴板、多几十毫秒 |

实测结论写在 [`docs/UIA-MATRIX.md`](docs/UIA-MATRIX.md)：**在浏览器里"选中再双击"是最快的路径（8 ms，零副作用）**；悬停取词只在原生应用里可靠。所以默认取词模式是「自动（选区优先，其次光标词）」。

## 性能

口径与完整数据见 [`docs/PERF.md`](docs/PERF.md)。

| 指标 | 实测 |
| --- | --- |
| 冷启动到托盘就绪 | 550 – 780 ms |
| **Rust 主进程**私有内存 | **6.3 MB**（工作集 27 – 29 MB） |
| 整个进程树私有内存 | **166 MB**（7 个进程，其中 6 个是 WebView2） |
| HUD 首帧（热键 → 首帧） | 15 – 27 ms |
| 词库查询 | 2.0 µs/次（77 万条） |
| NSIS 安装包 | ≈ 1.2 MiB（不含词库与模型） |

> 「常驻 30–60 MB」只能指 Rust 主进程。整个进程树私有内存约 166 MB，96% 是 WebView2 的浏览器/渲染/GPU 进程——这是任何 WebView 方案的底噪。报数字必须带口径。

## 安全边界

- **只装一个键盘钩子，且是只读的**：`WH_KEYBOARD_LL` 仅用于识别「双击 Ctrl」与「用户按了别的键」。
  回调里只把信号塞进 channel 然后 `CallNextHookEx`——**不吞键、不改键、不阻塞、不记录按键内容**。
  原因：`RegisterHotKey` 只能注册组合键，表达不了「双击」这种时序动作。
- **不注入 DLL、不打补丁、不 hook 窗口消息**。
- **不模拟键鼠**（M6「回填」的兜底路径除外，且必须严格保存/恢复剪贴板全格式）。
- 词库、模型、原始语料都不进仓库；网络访问只在构建期下载资源。

## 架构

```
src-tauri/src/
├── lib.rs          组合根（窗口、托盘、迁移、命令注册）
├── cli.rs          命令行：探针 / 词库导入 / 查词压测
├── commands.rs     IPC 适配层（命令名是前端契约）
├── core/           跨平台业务逻辑，禁止 cfg(target_os) 与原生 API
│   ├── selection.rs  取词编排：三档降级（纯逻辑可单测）
│   ├── dict.rs       redb 词库：归一化 / 回退 / 前缀
│   ├── text.rs       句子切分、词边界清洗
│   ├── double_tap.rs 双击 Ctrl 时序判定
│   ├── settings.rs   设置（新增字段必须 serde(default)）
│   └── shell.rs      HUD 生命周期、预热、尺寸、定位
└── platform/       OS 契约与后端（编译期全平台可检查）
    ├── keyboard.rs / clipboard.rs / uia.rs / window.rs / placement.rs   契约
    └── windows/     UIA（STA 常驻线程）、键盘钩子、剪贴板、DPI
```

纪律由 `scripts/check-architecture.ps1` 在 CI 里守住。

## 开发

```powershell
pnpm typecheck                                   # tsc --noEmit
pnpm test                                        # vitest
pnpm verify-notes                                # 决策笔记校验（见 AGENTS.md）

cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings        # 零 warning
cargo test
```

约定与踩过的坑见 [`AGENTS.md`](AGENTS.md)；重要改动的**理由与取舍**见 [`.agents/notes/`](.agents/notes/)。

## 探针结论（M4a / M5a）

实测数据见 [`docs/ONNX-PROBE.md`](docs/ONNX-PROBE.md)，探针代码在 `tools/onnx-probe/`。
一句话：**OCR 的 mobile 模型够快（40+48 ms），TTS 的内存取不下（242 MB），且中文音素化没有纯 Rust 方案。**

## 路线图

| 里程碑 | 内容 |
| --- | --- |
| M0 ✅ | 骨架 · 单例 · 托盘 · HUD 预热 · 性能基线 |
| M1 ✅ | 三档取词 + 上下文 + 探针 |
| M2 ✅ | redb 词库 + 卡片栈下探 |
| M3 | 翻译分层路由（本地词典 → 在线兜底）+ 双栏对照 |
| M4 | PP-OCRv5 端侧 OCR + 原图覆盖 |
| M5 | Kokoro 离线朗读（流式分句 + 跟读高亮） |
| M6 | 标注层 / 原地回填（含撤销快照） |

## 已知限制

- **中文分词已接入，但不知道光标落在哪个字**：UIA 只给出整段文本，所以多词时列候选让你点，而不是自动定位。
- **中文词表加载会多占约 28 MB**（词表结构 + redb 中文表页缓存），懒加载，英文阅读不触发。优化见 `docs/TODO.md` #18。
- **Firefox 没有 TextPattern**，只能走剪贴板兜底。
- **句子中文译文靠在线接口**：断网时句子档不可用（单词档仍走本地词库）。免密端点是非官方接口，可能失效。
- **Chromium 的悬停取词不稳定**，浏览器请用「选中再双击」。
- **命令行不能和应用同时打开词库**：redb 是独占锁，GUI 运行时 `--lookup` 会报
  `Database already open`。要用 CLI 查词请先退出托盘里的 Lemma。

## 许可

[MIT](LICENSE)。词库数据来自 [ECDICT](https://github.com/skywind3000/ECDICT)，遵循其自身许可。
