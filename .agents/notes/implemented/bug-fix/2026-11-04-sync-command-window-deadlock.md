# Agent Note: 同步命令里创建窗口导致死锁（双栏按钮"点了没反应"）

Status: implemented

## Problem

双栏窗口做好后实测：点 HUD 上的「双栏」按钮**毫无反应**，随后整个 HUD 也失去响应——`Esc` 关不掉、点别处不收起，看起来像卡死。

日志里只有 `capture` 和 `hud present`，**没有任何窗口创建或错误记录**。前端那条 `invoke` 既不成功也不失败，永远悬着。

## Decision

把会创建窗口的两个命令（`open_reader`、`open_settings`）从**同步命令改成 `async`**。

根因：Tauri 的**同步命令在主线程上执行**，而 `WebviewWindowBuilder::build()` 需要事件循环转起来才能把窗口创建完——主线程正被这条命令占着，于是互等成死锁。死锁的是整个事件循环，所以不只是按钮没反应，HUD 的关闭逻辑（`Esc`、失焦隐藏）也一起冻住了。

改成 `async` 后命令跑在异步运行时的工作线程上，窗口创建请求照常投递给主线程，正常返回。

配套的诊断设施（这次是靠它们才定位到的，保留下来）：

- `commands::log_frontend`：前端可以把日志写进 Rust 的日志文件。GUI 的 console 我看不到，而 `catch(() => undefined)` 一吞，问题就彻底消失——这是本次排查最痛的教训。
- `LEMMA_READER_SMOKE_IPC` 环境变量：启动 2.5s 后由 Rust 发事件、**让前端自己走一遍 `invoke("open_reader")`**，因此不需要点击、不需要窗口焦点就能验证整条 IPC 链路。
- 启动时打印 `config windows: [...]`，确认编译进二进制的窗口清单。
- `present_reader` 在配置拿不到时用硬编码 `WebviewWindowBuilder` 兜底。

## Alternatives considered

- **把命令改成"发一个任务到主线程然后立即返回"**：更符合 Tauri 的线程模型，也不会让工作线程等窗口创建。否决原因：这条命令需要把失败原因返回给前端（按钮点不动时要能看到原因），
  立即返回就拿不到结果；而 `async` + 直接调用已经能同时满足"不阻塞主线程"和"拿到结果"。
- **在 `present_reader` 里用 `run_on_main_thread` + 一次性 channel 等结果**：能拿到结果，但要处理"已经在主线程时内联执行"的分支，代码复杂度高且容易再次踩坑。
- **只在 HUD 侧做乐观 UI**（点了先显示"已打开"）：把问题藏起来而不是修掉。否决。
- **移除 `LEMMA_READER_SMOKE_IPC` 冒烟入口**：生产包里少一个环境变量分支。否决原因：我在这个会话里反复因为"抢不到窗口焦点"而无法验证 GUI 路径，
  这个入口是把"必须人点"变成"可以脚本验证"的唯一办法，收益远大于那几行代码。

## Consequences

- 收益：双栏可以打开了；同类"命令里建窗口"的坑被写进 `AGENTS.md`，下一个人不会再踩。
- 代价 1：`async` 命令的返回要经过序列化边界，比同步命令多一次往返（毫秒级，可忽略）。
- 代价 2：仓库里多了两个诊断设施（`log_frontend`、冒烟环境变量）。它们有用但属于"开发期脚手架"，
  发布前应复核是否保留——目前判断是保留，因为 GUI 类项目缺少它们几乎无法远程排障。
- 教训（已进 `AGENTS.md`）：**任何会创建窗口的命令都必须是 `async`**；同步命令只允许做纯数据操作。

## Testing

- 冒烟实测（真实 IPC 路径，不点击）：
  ```
  LEMMA_READER_SMOKE_IPC="..." lemma.exe
  → [frontend:info] smoke event received, chars=70
  → window created reader
  → reader opened
  → [frontend:info] smoke openReader ok
  → frontend ready reader #/reader
  → translate ok provider=google chars=28 / chars=40   （两段分别翻译成功）
  ```
  窗口列表出现「Lemma 双栏」，双栏窗口内两段原文各自拿到译文。
- 修复前同一冒烟的日志止于 `smoke event received`，之后**既无成功也无失败**——正是死锁特征。
