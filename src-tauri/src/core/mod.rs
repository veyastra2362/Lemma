//! 跨平台应用逻辑。禁止出现 `cfg(target_os)` 与原生 API。

pub mod logs;
pub mod perf;
pub mod shell;
pub mod shortcut;
pub mod tray;
