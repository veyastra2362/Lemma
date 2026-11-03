# AGENTS.md

给在本仓库工作的编码 agent 的约定。

## 命令

前端（仓库根目录）：

- 安装依赖：`pnpm install`
- 类型检查：`pnpm typecheck`
- 单测：`pnpm test`；单个文件：`pnpm exec vitest run src/utils/route.test.ts`
- 构建：`pnpm build`
- 热重载：`pnpm tauri dev`

后端（`src-tauri/`）：

- 语法检查：`cargo check`（改 `Cargo.toml` 会触发全量依赖重扫，尽量别乱动）
- 格式化：`cargo fmt --all`，校验 `cargo fmt --all -- --check`
- 静态扫描：`cargo clippy --all-targets -- -D warnings`（零容忍）
- 测试：`cargo test`；聚焦 `cargo test platform::placement::tests`
- 分层纪律：`pwsh -File scripts/check-architecture.ps1`

## 分层纪律（硬约束）

`src-tauri/src/core/` 是跨平台业务逻辑：

- 禁止出现 `cfg(target_os)`；
- 禁止直接依赖 `windows` / 任何原生 crate；
- 只能通过 `crate::platform` 导出的函数与类型触达操作系统。

`cfg(target_os)` 只允许出现在 `platform/mod.rs` 与各平台后端内部。
OS 特定 crate 必须写在 `Cargo.toml` 的 `[target.'cfg(...)'.dependencies]` 下。

## 构建与踩过的坑（都是真踩过的）

1. **不要用裸 `cargo build --release` 验证前端**：`tauri-build` 会置 `cfg(dev)`，
   应用去加载 `http://localhost:1420`，结果就是白屏。一律用 `pnpm tauri build`。
   只想快速编译检查用 `cargo check` / `cargo clippy`。
2. **`pnpm tauri build` 会校验 Rust crate 与 npm 包的 minor 版本一致**，不一致直接失败。
   crates.io 上的 `tauri` 稳定版是 2.11.6，而 npm 的 `@tauri-apps/api` 已到 2.12；
   因此 `@tauri-apps/api` 锁 `~2.11.1`，并且**不要**引入前端用不到的 `@tauri-apps/plugin-*` 包
   （插件只在 Rust 侧用就够了，装了反而触发版本校验失败）。
3. **`tauri.conf.json` 里 `noRedirectionBitmap` / `macOSPrivateApi` 当前不可用**：
   前者要 tauri 2.12（未发布），后者要求 `tauri` 开 `macos-private-api` feature。
   加了会直接编译失败。
4. **`visible: false` 的窗口，其 WebView 不一定会加载页面**（实测 shell 不加载，
   settings/ocr-region 加载）。所以 HUD 需要显式预热，见 `core::shell::prewarm`。
5. **注册全局热键会撞车**（本机 Alt+Q 已被占用）。`RegisterHotKey` 是独占的，
   必须准备候选键并把"实际生效的那个"显示到界面上，不要静默失败。
6. **WebView2 的 IPC 通道建立和页面模块执行有竞争**：`__TAURI_INTERNALS__` 已存在时
   `invoke` 仍可能第一次被拒。前端 IPC 调用统一走 `withRetry`（见 `src/lib/ipc.ts`）。

## 不变式（改动了会出隐蔽故障）

1. **`tauri.conf.json` 必须保留 `shell` / `settings` / `ocr-region` 三个窗口声明**。
   三个窗口共用同一份前端产物，靠 `index.html#/xxx` hash 路由区分；降级成纯动态
   创建会破坏 WebView2 路由解析与透明透传。
   其中 shell 启动即创建（要预热），settings / ocr-region 用 `"create": false` 懒创建——
   三个窗口就是三个 WebView2 渲染进程，全建出来会多占约 130 MB 工作集（见 `docs/PERF.md`）。
2. **Win32 与 Tauri 的可见性状态必须同时更新**：凡是调用 `force_hide`（`SW_HIDE`）
   隐藏的窗口，再显示时必须同时 `window.show()` + `force_show()` + `set_focus()`，
   只做一侧会出现「宿主显示了但 WebView 空白」或反之。
3. **设置窗口显示必须两步**：先 `window.show()` 让宿主出来，再给 WebView 控制器
   `webview.set_size()` + `webview.show()`。只做前者会在透明客户区外留一圈可见边框。
4. **`single-instance` 必须是第一个注册的插件**，否则事件会被其它插件先吃掉。
5. **HUD 的呼出路径上禁止创建窗口**。HUD 窗口启动时就建好并预热，
   之后只 `show` / `hide` / `set_position`。重建 WebView2 = 白屏闪烁 + 几十毫秒 + 内存泄漏。
   （设置 / 框选窗口允许按需创建，它们不在「毫秒级呼出」的关键路径上。）

## 性能测试的规矩

- 「热键 → HUD 首帧」必须由前端 `requestAnimationFrame` 回报
  （`report_first_paint`），不接受 Rust 侧 `show()` 返回时刻作为数据点。
- 报内存必须写清口径：是否含模型权重、是否含 WebView2 子进程。
- 报 OCR/TTS 延迟必须区分冷启动与热态。

## 不要做

- 不要提交密钥或 API key。
- 不要为了方便而放宽 CSP。
- 不要装全局键盘钩子、不要注入 DLL（见 README「安全边界」）。
- 不要自动 `git push`。
- 不要引用 / 拷贝 `LunarFlow42/TransPortal` 的源码：它没有 LICENSE 文件，
  只作为架构与踩坑的参考，实现自己写。
