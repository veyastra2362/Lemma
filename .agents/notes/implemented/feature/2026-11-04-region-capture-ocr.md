# Agent Note: 框选截图 OCR（M4c）

Status: implemented

## Problem

M4b 把 OCR 管线做进主工程了，但只能靠命令行喂图片。产品形态还差最后一段：
**用户在屏幕上框一块 → 立刻看到识别与翻译**。

M4b 的标定结论直接约束了这里：整页 OCR 会把长边压到 960，2201 px 的截图缩到 0.44 倍后
细节被抹平（`李杜`→`李士`）；而几百像素宽的**区域**几乎不缩放，内容 CER 0.000。
**所以这个功能必须是"框选"，不能是"整屏识别"。**

## Decision

三段：**GDI 截屏 → 框选层 UI → 复用既有取词链路**。

1. **截屏**用 GDI `BitBlt`（`platform/screen.rs` + `platform/windows/` 后端）：
   同步、无 COM 依赖、桌面截图足够。带 `CAPTUREBLT` 让分层窗口也进画面。
   实测 600×200 只要 **14 ms**。
2. **框选层**是已有的 `ocr-region` 窗口（`create: false` 懒创建），显示前先把窗口
   铺满**整个虚拟桌面**（`GetSystemMetrics(SM_VIRTUALSCREEN)` 系列，含多屏负坐标）。
   前端用 pointer 事件拖矩形，右键或 `Esc` 取消。
3. **坐标换算**：前端把 CSS 像素 × `devicePixelRatio` + 窗口左上角的物理坐标，得到物理矩形再传给 Rust。
   截图、OCR 全部按物理像素走。
4. **藏起来再截**：`run_region_ocr` 先 `hide_region_select`，`sleep(120ms)` 等窗口真的消失，
   否则遮罩会被自己截进去。
5. **复用取词链路**：OCR 文本经 `selection::store_ocr` 装成一次 `Capture`（新增 `Tier::Ocr`，标签"截图"），
   于是**翻译、HUD、双栏全部自动可用**——不需要为 OCR 再写一套显示逻辑。
6. 入口：托盘菜单「截图识别…」。命令 `start_region_select` / `run_region_ocr` / `cancel_region_select`
   都是 `async` 或纯隐藏操作，不违反"创建窗口的命令必须 async"。

## Alternatives considered

- **整屏识别（不做框选）**：少一个交互步骤，实现也简单。否决原因：M4b 用真实截图量过，
  整屏缩放会让准确率明显下降（`李杜`→`李士`、`跟`→`眼`），而且整页 67 个框要 1.0–1.2 s。
  框选把这两个问题一起解决：不缩放 + 框少。
- **Windows.Graphics.Capture（WGC）**：更现代，支持 HDR、受保护内容、逐窗口捕获。否决原因：
  需要 COM 上下文的窗口句柄与帧池异步回调，代码量是 BitBlt 的数倍；当前需求（桌面矩形）
  BitBlt 完全够用。留给将来"捕获单个窗口"时再换。
- **先冻结整屏成位图再框选**（截图工具常见做法）：拖动时画面不跳动，体验更好。否决原因：
  需要把整屏位图传给前端（一张 4K 图约 33 MB），或者做一次额外往返；
  当前遮罩是半透明覆盖，底下的内容仍然可见，体验差别不大。
- **给 OCR 单独做一套结果展示**：直觉上"截图结果"和"取词结果"是两种东西。否决原因：
  复用 `Capture` 之后翻译、双栏、点词下探全部直接可用，这几乎是零成本的收益。

## Consequences

- 收益：从"能识别图片"变成"能识别屏幕"；实测整条链路（截屏 14 ms + OCR 冷 572 ms）可用，
  且因为复用取词链路，**框选一段英文会直接给出译文**。
- 代价 1：**多显示器不同 DPI 下坐标换算会错位**。框选层是一个跨所有显示器的窗口，
  WebView2 只按主窗口的 `devicePixelRatio` 渲染，副屏若 DPI 不同，选出的矩形会偏。
  单屏或所有屏同 DPI 时正确。这一条记入 `docs/TODO.md`。
- 代价 2：只支持托盘菜单触发，还没有热键（用户此前对双栏要求过"可配置开关"，OCR 同理，
  等形态稳定后再加设置项）。
- 代价 3：HDR / DRM 保护内容截不到（BitBlt 的固有限制）。
- 代价 4：截图会包含窗口阴影、渐变背景等，深色 UI 上白字的对比度低时会掉准确率（未量化）。

## Testing

- `cargo test`：`platform::screen` 的空区域校验 + 既有 107 项全过。
- 无头验证（不依赖点击/焦点）：
  ```
  lemma.exe --capture 100,100,600,200 data/capture-test.png   → 14 ms，画面正确
  LEMMA_REGION_SMOKE="100,100,600,200" lemma.exe
  → region ocr capture=14ms boxes=3 chars=27
  → region smoke ok text=Rayburst / OBS Studio / Codex
  ```
  冒烟走的是与命令完全相同的 `run_region_capture`（提取成共用函数，避免两条实现漂移）。
- 真机交互（拖拽框选）需手动验证：自动化抢不到鼠标。
