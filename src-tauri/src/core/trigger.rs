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
use crate::core::reader::{self, ReaderPayload};
use crate::core::settings::{ReaderOpenMode, SettingsStore, Trigger};
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

    // 双栏热键独立于呼出热键，按自己的开关决定是否注册。
    if settings.reader_open_mode == ReaderOpenMode::Hotkey {
        if let Ok(reader_shortcut) = settings.reader_hotkey.parse::<Shortcut>() {
            let mode = settings.capture_mode;
            let registered = manager.on_shortcut(reader_shortcut, move |app, _shortcut, event| {
                if event.state != ShortcutState::Pressed {
                    return;
                }
                let capture = selection::capture(mode);
                if capture.text.trim().is_empty() {
                    return;
                }
                reader::set_payload(ReaderPayload {
                    text: capture.text,
                    app: capture.app,
                });
                shell::present_reader_on_main(app);
            });
            // 注册失败必须向上抛：否则用户看到"设置已保存"，但按键其实没生效。
            // （AGENTS 不变式：不允许「存下来了但不生效」。）
            registered.map_err(|error| {
                format!("注册双栏热键「{}」失败：{error}", settings.reader_hotkey)
            })?;
            logs::line(format!("trigger reader hotkey {}", settings.reader_hotkey));
        } else {
            logs::line(format!(
                "reader hotkey 格式不对：{}",
                settings.reader_hotkey
            ));
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
