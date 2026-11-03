use tauri::Runtime;

use crate::platform::window::{Window, WindowOps};

impl<R: Runtime> WindowOps<R> for Window<R> {
    fn force_show(&self) {
        // 其它平台由 Tauri 自己管可见性，这里不做额外动作。
    }

    fn force_hide(&self) {}
}
