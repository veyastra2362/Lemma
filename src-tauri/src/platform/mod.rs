//! OS 契约层。
//!
//! 纪律：`cfg(target_os = ...)` 只允许出现在本文件与各后端内部。
//! `core/` 永远只通过这里导出的函数与类型访问操作系统。

pub mod keyboard;
pub mod placement;
pub mod window;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as backend;

#[cfg(not(target_os = "windows"))]
mod fallback;
#[cfg(not(target_os = "windows"))]
use fallback as backend;

pub use window::Window as PlatformWindow;

/// 当前光标位置（物理像素，可为负值，多屏副屏）。
pub fn cursor_position() -> Option<placement::Point> {
    backend::cursor_position()
}

/// 给定物理坐标所在显示器的工作区（已排除任务栏）。拿不到时返回 `None`，
/// 由调用方退化为整屏区域。
pub fn work_area_at(point: placement::Point) -> Option<placement::Rect> {
    backend::work_area_at(point)
}

/// 安装只读键盘观察。回调在平台线程里被调用，**不要在里面做耗时操作**。
pub fn watch_keyboard(
    callback: impl Fn(keyboard::KeySignal) + Send + 'static,
) -> Result<(), String> {
    backend::watch_keyboard(callback)
}
