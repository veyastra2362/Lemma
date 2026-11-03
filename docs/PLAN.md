# Lemma — 高频深度阅读桌面辅助系统 · 工程计划

> 状态：v0.1 草案（待决策项见 §7）
> 工作目录：`D:\Code\Lemma`（仓库名 Lemma；改名前的数据目录见 `.agents/notes/implemented/architecture/2026-11-03-rename-to-lemma.md`）
> 目标平台：Windows 10 1809+ / 11，x64（首发），预留 macOS/Linux 抽象位

---

## 0. 定位与前提假设

一句话：**不打断阅读流的取词-翻译-回填闭环**。

三个前提假设（如果和你设想不一致，后面的排期都要改）：

| # | 假设 | 影响 |
|---|---|---|
| A1 | 翻译引擎 = 混合制（本地词典 → 本地小模型 → 在线 API 兜底），不是纯离线 | 决定 M3 工作量与联网合规 |
| A2 | "原地替换"默认是**标注覆盖层**，"真·改写原文"是高级选项 | 决定 M6 风险等级 |
| A3 | 单人开发，按里程碑可独立演示 | 决定里程碑切分粒度 |

---

## 1. 技术架构

```
┌──────────────────────────────────────────────────────────────┐
│ Tauri v2 Shell (single-instance + tray + RegisterHotKey)      │
│                                                              │
│  Frontend (React 19 + TS + Vite)                             │
│   ├─ hud-window        透明置顶 HUD（复用预热实例）            │
│   ├─ deck-panel        卡片栈 Deck / 面包屑 / 会话树           │
│   ├─ reader-window     图文双栏对照 + 原图译文覆盖层           │
│   └─ settings          引擎、模型、内存档位、支持矩阵           │
│                                                              │
│  Rust Core (src-tauri, edition 2024)                         │
│   ├─ capture/   UIA 取词三档 + 截图(WGC/BitBlt) + DPI 感知     │
│   ├─ lex/       redb 词库 + 归一化 + 词形还原 + FST 前缀补全   │
│   ├─ translate/ Provider trait: LocalDict | LocalLLM | Cloud   │
│   ├─ ocr/       ort + PP-OCRv5 det/rec + Rust DB 后处理        │
│   ├─ tts/       ort + Kokoro + 流式分句 + 音频时间戳对齐       │
│   ├─ inject/    标注层(默认) / TextPattern.SetText / 剪贴板回退 │
│   └─ sys/       热键、单例、内存修剪、tracing 环形日志         │
└──────────────────────────────────────────────────────────────┘
```

分层原则：**所有 OS 交互（UIA / 截图 / 注入 / 音频）都藏在 trait 后面**，上层只认
`SelectionSource`、`ScreenCapture`、`TextSink`、`AudioSink` 四个接口。这样 fixture 回放测试、
跨平台移植、以及"某个应用不支持 UIA 就换实现"都变成加一个 impl 的事。

---

## 2. 关键设计要点

### 2.1 取词：三档降级，默认零剪贴板污染

| 档 | 机制 | 优点 | 代价 |
|---|---|---|---|
| T0 | `TextPattern.RangeFromPoint` + `WordUnit` 扩展 | 不破坏用户选区、不动剪贴板、**可用鼠标悬停直接取词** | 依赖目标应用暴露 TextPattern |
| T1 | `TextPattern2.GetSelection` | 用户已选中时的最准来源，含格式 | 同上 |
| T2 | 模拟 Ctrl+C + 剪贴板读回 | 全覆盖兜底 | 污染剪贴板、需要保存恢复（含富文本格式）、有 30~80ms 开销 |

**取词必须带上下文**：单词脱离句子在深度阅读里没有意义。每次取词同时抓
`word / sentence / 段落前后各 120 字`，释义卡片可折叠展开语境例句。

### 2.2 词库：redb 只负责存，索引放内存

- 表结构
  - `terms: &[u8] (normalized) -> postcard(Entry)`
  - `alias: &[u8] (lemma/变体/缩写) -> &[u8] (canonical)`
  - `meta: &str -> &[u8]`
- 归一化管线：小写 → Unicode NFKC → 去变音 → 剥离所有格/复数候选 → 词形还原（规则版 + 不规则表）
- 前缀补全 / 模糊（编辑距离 ≤1）：内存 `fst` 或 `vob`，百万条索引常驻约 8~15MB
- 热路径实测目标：精确查询 P99 < 200µs（含反序列化）

### 2.3 Deck：不是栈，是**会话树**

线性栈会丢掉分支。数据模型：

```rust
struct Card { id, term, lang, senses: Vec<Sense>, picked: usize, ctx: Context }
struct Session { nodes: HashMap<CardId, Card>, root: CardId, active_path: Vec<CardId> }
```

- `active_path` 即面包屑，pop 即回退
- 从任意层点另一个词 → 插入新分支，旧分支保留在 `nodes` 里，随时可跳回
- 所有查词结果按 `(normalized_term, sense)` 缓存在 `HashMap`，回退时**不重新查询**
- 防环：同一条路径内同一 term 再次出现 → 高亮提示"回路"而不是无限 push

渲染层：
- 只挂载 `active_path` 的**顶层 3 层**；更深的节点降级为 `CardGhost`（记录高度 + scrollTop + 折叠态），回退时从 state 水合重挂
- 每层做 `transform: translate3d()` 位移而非改 layout，`contain: layout style paint` + `content-visibility: auto`
- 层间过渡走合成器，避免 reflow 抖动

### 2.4 OCR：难点在后处理，不在推理

PP-OCRv5 推理只是喂张量；真正的工作量在 Rust 里重写：
1. 预处理：长边缩放（det 限 960）、归一化、padding 到 32 倍数
2. det 输出 → 概率图 → 二值化 → 连通域（`imageproc` 或自写并查集）→ 轮廓 → `unclip` 外扩 → 最小外接矩形
3. 文本框排序与合并（按 y 聚类 → 行内按 x 排 → 行间距相似度合并成段落）
4. 文本行裁剪 + 旋转校正（可选 cls 模型）→ 按高度归一 48 → 宽度分桶 batch（rec 动态宽）
5. CTC 贪心解码 + `ppocr_keys_v1.txt` 字典 → 置信度过滤

**交付门禁**：用 PaddleOCR（Python，仅离线生成）跑一批真实截图产出 golden JSON，
Rust 实现必须逐字段对齐（框坐标容差 2px、文本完全一致），作为 CI 里的回归。

### 2.5 翻译层：Provider 抽象 + 三级路由

```
请求 → 缓存命中? → 是: 直接返回（redb: hash(text+langs+provider) -> 译文）
                 → 否: 分级路由
                       1) 单词且词库有释义 → 0 延迟，纯离线
                       2) 段落 + 本地小模型可用 → 离线批量译（后台线程，可中断）
                       3) 其余 / 质量兜底 → 云端 API（超时降级回 2 或 1）
```

要点：
- **段落对齐优先于逐行翻译**。双栏对照要按"段落块"译，逐行译出来是碎句，读不下去
- 分句保护：代码块、URL、专有名词、已有译文的 term 用占位符 `\u{e000}N` 包裹后替换，避免被模型糟蹋
- 语义缓存 + 请求合并（同一段被重复读的概率极高，深度阅读场景缓存命中率通常 >40%）

### 2.6 内存：区分"冷工作集"和"真释放"

`EmptyWorkingSet` 只是把页踢回 standby list，RSS 数字好看了，但下次呼出会
page fault 抖动 100~200ms。所以做**两阶段**：

| 阶段 | 触发 | 动作 | 效果 |
|---|---|---|---|
| L1 轻修剪 | HUD 隐藏后 5s | `SetProcessWorkingSetSize(-1,-1)` + 释放截图/OCR 中间缓冲 | 工作集 -20~40MB，热页仍在 |
| L2 深度休眠 | 空闲 10min 或内存压力 | 释放 ONNX session（det/rec/tts 各自惰性）、清空图片缓存、redb 页缓存回落 | 常驻回落至 30MB 级，下次唤醒多 300ms（或后台预加载） |

模型体积大（PP-OCRv5 + Kokoro 合计 50~150MB），所以**模型不常驻**，用 LRU + 空闲卸载。
简历上写 30-60MB 时要把口径写清："不含按需加载的模型权重"。

### 2.7 安全与合规边界（重要，建议一开始就定死）

- **不装全局键盘钩子（no `SetWindowsHookEx` LLM）**，只用 `RegisterHotKey` + 鼠标位置轮询（低频）
- **不注入 DLL、不打补丁、不 hook 消息**
- 这样能避免 95% 的杀软误报和企业 EDR 拦截，也避开"模拟输入"的合规灰区
- 文档里明确列出不支持的场景（游戏、UWP 沙盒、远程桌面会话等）

---

## 3. 里程碑路线图

工作量按单人全职估算；括号里是可演示的产物。

### M0 骨架与基线（3~5d）→「Ctrl+Alt+D 能弹出一个空格子」
- 仓库结构：`src-tauri/` + `src/`（pnpm workspace），`rust-toolchain.toml` 钉 1.85+ / edition 2024
- Tauri v2：单实例插件、托盘、`RegisterHotKey`、HUD 窗口 `visible: false` 预热、多窗口管理
- 前后端契约：`ipc` 类型用 `ts-rs` 从 Rust struct 生成 TS 类型（避免手写漂移）
- CI：`cargo fmt --check` / `cargo clippy -D warnings` / `cargo test` / `vitest` / `cargo build --release`
- **性能基线脚本**：热键→HUD 首帧、空查询往返、空闲 RSS，跑 50 次出 P50/P95

**验收**：冷启动到可呼出 < 1.5s；常驻 RSS < 35MB（不含模型）；HUD 呼出 P95 < 80ms

### M1 取词链路（5~7d）→「在任何地方取一个词，HUD 显示原文+来源」
- `SelectionSource` trait + T0/T1/T2 三档实现
- UIA 客户端封装（`windows` crate，`IUIAutomation`，TextPattern/TextPattern2/TextPatternRange）
- 句子/段落上下文捕获（按 `DocumentRange` 向两侧扩展句子单元）
- DPI 感知（per-monitor v2）+ 多屏坐标换算 + HUD 在光标附近智能避让
- 诊断面板：显示本次取词走的是哪档、耗时分解、命中的 Element 类名

**验收**：对 Chrome / Edge / 记事本 / VS Code / Word / Adobe Reader 的取词矩阵全部通过；
剪贴板内容在 T2 路径后完整还原（含富文本）

### M2 词库 + Deck（7~10d）→「能真的查词、能无限下探」【MVP 可交付点】
- 词库构建管线（离线 bin）：源数据 → 归一化 → 变体/词形 → redb 打包
- 检索 API：精确 / 前缀 / 模糊 / 词形回退，bench 到 P99 < 200µs
- Deck 会话树 + 面包屑 + 分支回看 + 回路检测
- 虚拟化挂载（顶层 3 层）+ 卡片动画 + 键盘导航（↑↓ 选义项 / Enter 下探 / Esc 回退）
- 翻译接口先接一个临时 provider（词典直出）跑通闭环

**验收**：百万词库冷启动首查 < 50ms；连续下探 30 层无卡顿（帧率 ≥ 55fps）；内存增长 < 15MB

### M3 翻译层（5~7d）→「段落能读懂了」
- Provider trait + LocalDict / LocalLLM(可选) / Cloud 三实现 + 路由与降级
- 语义缓存（redb）+ 请求合并 + 占位符保护 + 段落对齐算法
- 双栏阅读窗口（左原文右译文，段落级联动滚动）

**验收**：重复段落缓存命中率 ≥ 40%；在线全失败时仍能走词典档可用

### M4 端侧 OCR（10~14d）→「截一张图，双栏译文覆盖上去」
- `ort` 集成（onnxruntime dylib 打包策略、EP 选择、arena 复用）
- det/rec 全管线 + Rust DB 后处理 + CTC 解码（**本里程碑最大工作量，占 60%**）
- golden fixture 对拍 CI
- 截图（WGC 优先，BitBlt 兜底）+ 区域选择 + DPI
- 双栏对照 + 原图译文覆盖层（按框位置贴译文，可切换"覆盖/并排/悬浮"）
- 滚动阅读的**带状增量 OCR**：只重算新增区域，区域 hash 缓存

**验收**：1920×1080 全屏 OCR P95 < 600ms；中英混排 F1 ≥ 0.90（对拍集）；OCR 闲置 60s 后 session 卸载

### M5 TTS（5~7d）→「选中一段，读出当前句并高亮」
- Kokoro ONNX 管线（音素化方案见 §7 D3）
- 流式分句合成：第一句先播，边合成边播（首音延迟目标 < 400ms）
- 音频时间戳 ↔ 文本 span 对齐，实现跟读高亮
- 音色选择、语速、暂停/续读

**验收**：首音延迟 P95 < 500ms；长文连续朗读不积压、可即时打断

### M6 回填与注入（5~7d）→「译文能留在原文位置上」
- **默认路径**：标注覆盖层（HUD 浮层跟随目标控件矩形，仅显示不改原文）
- 高级路径：`TextPattern2`/`TextRange.SetText` 真替换，前置**撤销快照**（原文 + 选区范围）
- 兜底路径：剪贴板 + `SendInput` Ctrl+V（严格保存/恢复剪贴板全格式，失败必回滚）
- 支持矩阵与风险提示 UI（不可写控件降级为标注层）

**验收**：Word / VS Code / Chrome textarea 的替换成功率与撤销可用性；只读控件零误写

### M7 性能与收尾（5~7d）→「可发布」
- 双阶段内存修剪落地 + 模型 LRU
- 启动时间优化（前端 code split、Rust 侧延迟初始化、安装包 < 25MB）
- 崩溃上报（本地文件 + 可选）、自动更新（tauri-plugin-updater）
- 安装包、签名、README、演示视频/GIF

**总计**：约 45~60 人日（全职）。若只有周末，M0→M3 约 2.5 个月，可作为一个完整可用的产品先发。

---

## 4. 性能预算（SLO，从第一天就当门禁）

| 指标 | 目标 | 上限 |
|---|---|---|
| 热键 → HUD 首帧 | < 50ms P50 / < 80ms P95 | 150ms |
| 取词（T0 路径） | < 10ms | 30ms |
| 词库查询 | < 1ms P50 / < 200µs 单次 | 5ms |
| 卡片渲染 | < 16ms | 33ms |
| 全屏 OCR | < 400ms P95 | 900ms |
| TTS 首音 | < 400ms P95 | 800ms |
| 常驻 RSS（模型未加载） | 38MB | 60MB |
| 冷启动到托盘就绪 | < 1.2s | 2s |
| 安装包 | < 20MB | 30MB |

---

## 5. 风险台账

| # | 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|---|
| R1 | PP-OCRv5 DB 后处理在 Rust 里重写，精度调参耗时长 | 高 | 高 | golden fixture 对拍；先只做"单栏横排"场景，竖排/表格后续迭代 |
| R2 | UIA 覆盖不全（Chrome 需开 a11y、Electron 差、PDF 阅读器差异大） | 高 | 中 | 三档降级 + 明确支持矩阵；Chrome 走 `--force-renderer-accessibility` 检测与提示 |
| R3 | `EmptyWorkingSet` 造成唤醒抖动，指标好看体验变差 | 中 | 高 | 分两档修剪；用"唤醒到首帧"而不是"RSS"单独考核 |
| R4 | ONNX session 内存远超预期（EP arena） | 中 | 高 | 首周就做模型加载基准；关闭 arena 复用 / int8 量化 |
| R5 | 中文 Kokoro 音素化不可用 | 中 | 中 | D3 决策：备选 sherpa-onnx / piper / 英文优先 |
| R6 | 杀软误报 / EDR 拦截 | 中 | 高 | §2.7 边界；不 hook 不注入；代码签名 |
| R7 | 无 Python 但模型转换需要预处理 | 低 | 低 | 模型转换在构建期一次性完成，产物入库/发布为 artifact，运行时零 Python |

---

## 6. 我的补充建议（超出原描述的增量）

1. **取词默认不碰剪贴板**：`RangeFromPoint` + WordUnit 让"鼠标悬停 + 热键"直接取词，
   这是"不打断阅读流"的关键；T2 剪贴板只做兜底。原描述没提这一点，但它是体验分水岭。
2. **翻译是原描述里最大的空白**：有 OCR、有词库、有 TTS，唯独没说句子怎么译。建议补齐
   三级路由 + 语义缓存，否则"双栏对照"没法成立。
3. **Deck 用会话树而不是线性栈**：分支可回看、可收藏成"阅读线索"导出笔记 —— 这既是产品差异点，
   也是面试时能讲透的技术点（图遍历 + 虚拟化渲染 + 缓存）。
4. **原地替换默认改成"标注层"**：`SetText` 会破坏撤销栈、可能触发文档云同步冲突、只能写可编辑控件。
   90% 场景用户想要的是"译文贴在旁边/盖在上面"，那本质上就是一个跟随矩形定位的浮层。
   真替换降级为高级选项 + 撤销快照。
5. **OCR 增量与段落对齐**：滚动阅读只 OCR 新增条带；译文按段落块而非按行生成，否则读起来是碎句。
6. **TTS 不做"朗读全文"，做"读当前卡片/当前句"**：流式分句 + 首句先播 + 跟读高亮。
7. **先写 SLO 再写代码**：桌面端最容易死在"功能全做了但都慢半拍"。
8. **模型加载策略是个独立设计**：惰性加载 + LRU + 空闲卸载，否则常驻内存目标一开局就破。
9. **可测试性前置**：UIA/截图/注入三块最难测，抽象成 trait + 录制成 fixture 回放，
   这是这类项目最常见的烂尾点。
10. **诊断面板**：HUD 里显示"本次取词来源 / 各阶段耗时"，自己调优 + 用户信任感双收益。
11. **合规边界写进 README**：不 hook、不注入、不模拟键鼠（除兜底粘贴），这既是技术选择也是信任资产。
12. **平台抽象位留好**：`SelectionSource` / `TextSink` 两个 trait 定死，macOS(AX) / Linux(AT-SPI)
    后续接入成本极低，简历上也多一句"跨平台架构"。
13. **建议至少录一段 30s 演示 GIF**：划词 → 下探 5 层 → 截图 OCR → 双栏 → 回填。
    这类项目"能演示"的价值远大于"功能多"。

### 如果目标是作品集/简历项目，优先级要改
把 M4(OCR) 和 M6(回填) 提前到 M3 之前，TTS 放最后：OCR 和回填是**视觉冲击力最大**的两个；
TTS 最难演示、最容易让人觉得"锦上添花"。功能完整度按"能录 30s 演示"排序，不按技术难度排序。

---

## 7. 决策记录

| # | 决策 | 结论 | 时间 |
|---|---|---|---|
| D1 | 翻译引擎 | **混合分级**：本地词典直出 → 在线 API 兜底；本地小模型留接口，不在 M3 实现 | 2026-11-03 |
| D2 | "原地替换"语义 | **默认标注覆盖层**；真替换（`TextRange.SetText`）作为高级选项 + 前置撤销快照 | 2026-11-03 |
| D3 | TTS 方案 | 英文优先，音素化方案在 M5 前定（参考实现用 `misaki-rs` 跑通过，可作候选） | 2026-11-03 |
| D4 | 词库数据源 | **ECDICT** 起手（`ecdict.mini.csv` 只是样例，必须用完整 CSV） | 2026-11-03 |
| D5 | 基线策略 | **混合**：脚手架与架构照抄思路、Rust 核心自己写；不拷贝参考实现源码（其无 LICENSE） | 2026-11-03 |
| D6 | 项目定位 | **自用工具**：优先「能天天用的闭环」与稳定性，不为演示效果提前做 TTS | 2026-11-03 |
| D7 | 呼出方式 | **默认双击 Ctrl**（320ms 窗口 / 长按不算 / 中间按过其它键复位），另提供可自定义热键，二者可在设置里切换并持久化 | 2026-11-03 |
| D8 | 键盘钩子边界 | **允许一个只读 `WH_KEYBOARD_LL`**，仅识别双击 Ctrl 与"其它键按下"，不吞键/不注入/不记录内容。代价：可能被个别杀软关注，所以边界写死在 `AGENTS.md` | 2026-11-03 |
| D9 | M1 验收目标 | **Chrome 优先，Firefox 其次**；PDF 阅读器与 Electron 应用放到 M1 之后单独评估 | 2026-11-03 |

### 由此产生的优先级调整

原计划顺序 M0→M1→**M2→M3**→M4→M6→M5 保持；因为定位是自用工具，
**M2+M3 完成即为可用产品**（能查、能下探、能翻译），M4/M6 是差异化，M5（TTS）最后。

> 参考实现（[`LunarFlow42/TransPortal`](https://github.com/LunarFlow42/TransPortal)）的调研结论
> 与踩坑清单见 [`REFERENCE-NOTES.md`](REFERENCE-NOTES.md)。
> 其中最关键的一条：它自己的实测表明 OCR「50ms / 45MB」的原始设想**没有达成**
> （热态 272ms、探针峰值 79.4MiB），本项目所有对外口径按真实数据写。

---

## 8. 待办：M1 起步前的三个前置

1. Rust 工具链 + MSVC Build Tools 就绪（正在装）。
2. `scripts/ci` 门禁在本地跑通（fmt / clippy / test / 分层脚本）。
3. `scripts/perf-baseline.ps1` 产出第一份 JSON 基线，作为后续所有优化的对照。

