//! 全局热键。
//!
//! 只用 `RegisterHotKey`（由 global-shortcut 插件封装），**不装键盘钩子**。
//! 这既是为了不打断用户的按键流，也是为了避开杀软/EDR 对 LL hook 的误判。

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// HUD 呼出热键：Alt+Q。
pub fn hud() -> Shortcut {
    Shortcut::new(Some(Modifiers::ALT), Code::KeyQ)
}

pub fn register_defaults(app: &AppHandle) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(hud(), |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                crate::core::shell::toggle(app);
            }
        })
        .map_err(|e| format!("注册 Alt+Q 失败（可能已被其它程序占用）：{e}"))
}
