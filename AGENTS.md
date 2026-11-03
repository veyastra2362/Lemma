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

## 不变式（改动了会出隐蔽故障）

1. **`tauri.conf.json` 必须保留 `shell` / `settings` / `ocr-region` 三个窗口声明**。
   三个窗口共用同一份前端产物，靠 `index.html#/xxx` hash 路由区分；降级成纯动态
   创建会破坏 WebView2 路由解析与 `noRedirectionBitmap` 透明透传。
2. **Win32 与 Tauri 的可见性状态必须同时更新**：凡是调用 `force_hide`（`SW_HIDE`）
   隐藏的窗口，再显示时必须同时 `window.show()` + `force_show()` + `set_focus()`，
   只做一侧会出现「宿主显示了但 WebView 空白」或反之。
3. **设置窗口显示必须两步**：先 `window.show()` 让宿主出来，再给 WebView 控制器
   `webview.set_size()` + `webview.show()`。只做前者会在透明客户区外留一圈可见边框。
4. **`single-instance` 必须是第一个注册的插件**，否则事件会被其它插件先吃掉。
5. **呼出路径上禁止创建窗口**。所有窗口启动时创建好，之后只 `show` / `hide` /
   `set_position`。重建 WebView2 = 白屏闪烁 + 几十毫秒 + 内存泄漏。

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
