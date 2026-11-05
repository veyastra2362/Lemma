//! OS 契约层。
//!
//! 纪律：`cfg(target_os = ...)` 只允许出现在本文件与各后端内部。
//! `core/` 永远只通过这里导出的函数与类型访问操作系统。

pub mod audio;
pub mod clipboard;
pub mod console;
pub mod keyboard;
pub mod placement;
pub mod screen;
pub mod uia;
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

/// 播放 WAV（异步，立即返回；后一次调用会打断前一次）。
pub fn play_wav(path: &std::path::Path) -> Result<(), String> {
    audio::play_wav(path)
}

/// 停止当前播放。
pub fn stop_audio() {
    audio::stop();
}

#[cfg(target_os = "windows")]
pub(crate) fn backend_play_wav(path: &std::path::Path) -> Result<(), String> {
    audio::windows_impl::play(path)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn backend_play_wav(_path: &std::path::Path) -> Result<(), String> {
    Err("当前平台暂不支持音频播放".to_string())
}

#[cfg(target_os = "windows")]
pub(crate) fn backend_stop_audio() {
    audio::windows_impl::stop();
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn backend_stop_audio() {}

/// 整个虚拟桌面（所有显示器）的物理像素边界。
pub fn virtual_screen_bounds() -> Option<placement::Rect> {
    backend::virtual_screen_bounds()
}

/// 截取屏幕上的一块矩形（物理像素，允许负坐标）。
pub fn capture_region(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<screen::Screenshot, String> {
    screen::capture_region(x, y, width, height)
}

#[cfg(target_os = "windows")]
pub(crate) fn backend_capture_region(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<screen::Screenshot, String> {
    screen::windows_impl::capture(x, y, width, height)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn backend_capture_region(
    _x: i32,
    _y: i32,
    _width: u32,
    _height: u32,
) -> Result<screen::Screenshot, String> {
    Err("当前平台暂不支持屏幕截图".to_string())
}

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

/// 探测某个屏幕点（默认光标位置）的取词能力，用于建立支持矩阵。
pub fn uia_probe(point: Option<(i32, i32)>) -> uia::ProbeReport {
    backend::uia_probe(point)
}

/// 走 UIA 取词（选区优先，其次光标所在词）。不做剪贴板兜底。
pub fn uia_capture() -> uia::UiaOutcome {
    backend::uia_capture()
}

/// 模拟 Ctrl+C 把选区复制出来（T2 兜底）。
pub fn copy_selection(timeout: std::time::Duration) -> Result<clipboard::ClipboardLease, String> {
    backend::copy_selection(timeout)
}

/// 把 stdout 挂到父进程控制台（探针模式用）。
pub fn attach_parent_console() {
    backend::attach_parent_console();
}

/// 还原剪贴板（由 `ClipboardLease::drop` 调用）。
pub fn restore_clipboard(previous: Option<&str>, after_seq: u32) {
    backend::restore_clipboard(previous, after_seq)
}
/// 等桌面合成器真正完成一帧。
///
/// 隐藏窗口之后立刻 `BitBlt` 会拿到**旧**的屏幕内容（合成是异步的），
/// 于是框选层的遮罩/提示会被截进 OCR 图片里（用户实测遇到过）。
/// `DwmFlush` 会阻塞到下一次合成完成，比 sleep 固定时长可靠。
pub fn flush_compositor() {
    backend::flush_compositor();
}
