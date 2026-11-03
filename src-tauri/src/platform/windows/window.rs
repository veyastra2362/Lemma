//! Windows 窗口强制显隐。
//!
//! `SW_SHOW` / `SW_HIDE` 直接作用于原生窗口句柄，用来和 Tauri 自己的
//! 可见性状态对齐。Tauri 的 `hwnd()` 返回的 `HWND` 可能来自另一个
//! `windows` crate 版本，这里用裸指针中转后再转回本地类型。

use tauri::Runtime;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOW, ShowWindow};

use crate::platform::window::{Window, WindowOps};

fn handle<R: Runtime>(window: &Window<R>) -> Option<HWND> {
    let raw = window.inner().hwnd().ok()?;
    let raw = raw.0 as isize;
    if raw == 0 {
        return None;
    }
    Some(HWND(raw as *mut core::ffi::c_void))
}

impl<R: Runtime> WindowOps<R> for Window<R> {
    fn force_show(&self) {
        if let Some(hwnd) = handle(self) {
            // SAFETY: hwnd 来自当前进程自己的窗口。
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
        }
    }

    fn force_hide(&self) {
        if let Some(hwnd) = handle(self) {
            // SAFETY: 同上。
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
    }
}
