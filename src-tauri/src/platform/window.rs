//! 与平台无关的窗口能力契约。
//!
//! `Window` 是一个薄包装：所有 OS 差异放在各后端的 `impl WindowOps` 里。
//! 之所以需要 `force_show` / `force_hide`，是因为 Win32 的 `SW_HIDE` 与
//! Tauri/Tao 自己的可见性状态会各自记账，只调一侧会出现「宿主已经显示但
//! WebView 还是空白的边框」这种经典故障。

use tauri::{Runtime, WebviewWindow};

pub trait WindowOps<R: Runtime> {
    /// 用 OS 原生方式强制显示。
    fn force_show(&self);
    /// 用 OS 原生方式强制隐藏。
    fn force_hide(&self);
    /// 透明窗口的额外平台配置（默认无操作）。
    fn configure_transparency(&self) {}
}

pub struct Window<R: Runtime> {
    inner: WebviewWindow<R>,
}

impl<R: Runtime> Window<R> {
    pub fn new(inner: WebviewWindow<R>) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &WebviewWindow<R> {
        &self.inner
    }
}
