//! 触发方式装配：双击 Ctrl 与全局热键二选一。
//!
//! 键盘观察是**常驻安装**的（安装/卸载低级钩子代价高、且卸载时机容易出竞态），
//! 是否真的呼出由设置决定——切换触发方式只是改一个判断，不需要动钩子。
//!
//! **顺序**：先取词，再呼出。HUD 一显示就会抢走前台焦点，而剪贴板兜底是给前台
//! 窗口发 Ctrl+C 的，顺序反了就对着自己发的 HUD 发 Ctrl+C。

use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::core::double_tap::DoubleTap;
use crate::core::settings::{SettingsStore, Trigger};
use crate::core::{logs, selection, shell};
use crate::platform;

/// 安装键盘观察，并按当前设置装配热键。
pub fn install<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let detector = Arc::new(Mutex::new(DoubleTap::default()));
    let handle = app.clone();

    platform::watch_keyboard(move |signal| {
        let fired = {
            let Ok(mut guard) = detector.lock() else {
                return;
            };
            guard.on_signal(signal, Instant::now())
        };
        if !fired {
            return;
        }
        let settings = handle.state::<SettingsStore>().get();
        if settings.trigger != Trigger::DoubleControl {
            return;
        }

        logs::line("trigger double-tap fits");
        // 取词会阻塞几十毫秒，放在这个观察线程上做，别阻塞主线程。
        selection::capture(settings.capture_mode);
        shell::toggle_on_main(&handle);
    })?;
    logs::line("keyboard watch installed");

    apply(app)
}

/// 按当前设置重新装配热键（幂等，可以反复调用）。
pub fn apply<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let settings = app.state::<SettingsStore>().get();
    let manager = app.global_shortcut();
    manager.unregister_all().map_err(|e| e.to_string())?;

    // 截图识别热键
    if settings.ocr_hotkey_enabled {
        match settings.ocr_hotkey.parse::<Shortcut>() {
            Ok(shortcut) => {
                let registered = manager.on_shortcut(shortcut, |app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        shell::present_region_select_on_main(app);
                    }
                });
                if let Err(error) = registered {
                    // 热键被别人占了：降级为只用托盘菜单，但要记下来
                    logs::line(format!(
                        "ocr hotkey failed（{}）：{error}（托盘菜单仍可用）",
                        settings.ocr_hotkey
                    ));
                } else {
                    logs::line(format!("trigger ocr hotkey {}", settings.ocr_hotkey));
                }
            }
            Err(error) => logs::line(format!(
                "ocr hotkey 格式不对（{}）：{error}",
                settings.ocr_hotkey
            )),
        }
    }

    if settings.trigger != Trigger::Hotkey {
        logs::line("trigger mode doubleControl");
        return Ok(());
    }

    let shortcut: Shortcut = settings
        .hotkey
        .parse()
        .map_err(|e| format!("热键格式不正确「{}」：{e}", settings.hotkey))?;

    manager
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let mode = app.state::<SettingsStore>().get().capture_mode;
                selection::capture(mode);
                shell::toggle_on_main(app);
            }
        })
        .map_err(|e| format!("注册热键「{}」失败：{e}", settings.hotkey))?;

    logs::line(format!("trigger hotkey {}", settings.hotkey));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 前端录制出来的热键字符串必须能被解析。
    /// 实测 `Alt+Q` 与 `Alt+KeyQ` 两种写法都能过，前端用更易读的前者。
    #[test]
    fn user_facing_hotkey_strings_parse() {
        for raw in ["Alt+Q", "Ctrl+Shift+D", "Alt+Shift+Q", "Alt+Digit1"] {
            assert!(raw.parse::<Shortcut>().is_ok(), "热键写法无法解析：{raw}");
        }
    }

    #[test]
    fn garbage_hotkey_is_rejected() {
        assert!("".parse::<Shortcut>().is_err());
        assert!("不是热键".parse::<Shortcut>().is_err());
    }
}
