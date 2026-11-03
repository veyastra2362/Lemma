//! 全局热键。
//!
//! 只用 `RegisterHotKey`（由 global-shortcut 插件封装），**不装键盘钩子**。
//! 这既是为了不打断用户的按键流，也是为了避开杀软/EDR 对 LL hook 的误判。
//!
//! 现实情况：`RegisterHotKey` 是独占的，别的软件占了 Alt+Q 我们就注册不上
//! （本机实测 Alt+Q 已被占用）。所以内置一串候选键，从前往后试，谁成功用谁，
//! 并把最终生效的键报给界面——不要静默失败。

use std::sync::Mutex;

use tauri::{AppHandle, Runtime};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// 候选呼出热键，按优先级排列。
pub fn candidates() -> Vec<(&'static str, Shortcut)> {
    vec![
        ("Alt+Q", Shortcut::new(Some(Modifiers::ALT), Code::KeyQ)),
        (
            "Alt+Shift+Q",
            Shortcut::new(Some(Modifiers::ALT | Modifiers::SHIFT), Code::KeyQ),
        ),
        (
            "Alt+Shift+D",
            Shortcut::new(Some(Modifiers::ALT | Modifiers::SHIFT), Code::KeyD),
        ),
    ]
}

/// 当前实际生效的热键；`None` 表示全部候选都被占用。
#[derive(Default)]
pub struct ActiveHotkey(pub Mutex<Option<String>>);

impl ActiveHotkey {
    pub fn label(&self) -> Option<String> {
        self.0.lock().ok().and_then(|slot| slot.clone())
    }

    fn set(&self, label: Option<String>) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = label;
        }
    }
}

/// 依次尝试候选热键，返回最终生效的那个。
pub fn register_defaults<R: Runtime>(app: &AppHandle<R>) -> Result<String, String> {
    let mut failures = Vec::new();

    for (label, shortcut) in candidates() {
        let result =
            app.global_shortcut()
                .on_shortcut(shortcut, |app: &AppHandle<R>, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        crate::core::shell::toggle(app);
                    }
                });

        match result {
            Ok(()) => return Ok(label.to_string()),
            Err(error) => failures.push(format!("{label}（{error}）")),
        }
    }

    Err(format!("所有候选热键都被占用：{}", failures.join("、")))
}

/// 注册并把结果写进状态与日志。
pub fn install<R: Runtime>(app: &AppHandle<R>, state: &ActiveHotkey) {
    match register_defaults(app) {
        Ok(label) => {
            crate::core::logs::line(format!("shortcut active {label}"));
            state.set(Some(label));
        }
        Err(error) => {
            crate::core::logs::line(format!("shortcut failed {error}"));
            state.set(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_are_distinct_and_ordered() {
        let list = candidates();
        assert_eq!(list.len(), 3);
        let labels: Vec<&str> = list.iter().map(|(label, _)| *label).collect();
        assert_eq!(labels, vec!["Alt+Q", "Alt+Shift+Q", "Alt+Shift+D"]);
    }

    #[test]
    fn active_hotkey_starts_empty_and_round_trips() {
        let state = ActiveHotkey::default();
        assert!(state.label().is_none());
        state.set(Some("Alt+Q".to_string()));
        assert_eq!(state.label().as_deref(), Some("Alt+Q"));
        state.set(None);
        assert!(state.label().is_none());
    }
}
