//! 用户设置：触发方式与热键。落盘到 `<app_data>/settings.json`。
//!
//! 故意不用 tauri-plugin-store：这里只有两个字段，手写 JSON 更小更可控，
//! 而且序列化错误可以给出可读的报错。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// 呼出方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Trigger {
    /// 双击 Ctrl（默认）。需要只读键盘观察。
    DoubleControl,
    /// 全局热键。
    Hotkey,
}

/// 默认热键。注意 `RegisterHotKey` 是独占的，这个键很可能已被别的软件占用，
/// 所以真正的默认触发是双击 Ctrl。
pub const DEFAULT_HOTKEY: &str = "Alt+Q";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub trigger: Trigger,
    /// 形如 `Alt+Q` / `Ctrl+Shift+D`，仅在 `trigger == Hotkey` 时生效。
    pub hotkey: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            trigger: Trigger::DoubleControl,
            hotkey: DEFAULT_HOTKEY.to_string(),
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<Settings>,
}

impl SettingsStore {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("settings.json");
        let missing = !path.exists();
        let current = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Settings>(&raw).ok())
            .unwrap_or_default();

        let store = Self {
            path,
            current: Mutex::new(current),
        };

        // 首次运行把默认值落盘：用户能在 %APPDATA% 里找到这个文件，手改也能生效。
        if missing {
            let defaults = store.get();
            let _ = store.set(defaults);
        }

        store
    }

    pub fn get(&self) -> Settings {
        self.current
            .lock()
            .map(|settings| settings.clone())
            .unwrap_or_default()
    }

    /// 先写内存再落盘；落盘失败不影响本次会话可用。
    pub fn set(&self, next: Settings) -> Result<Settings, String> {
        {
            let mut slot = self
                .current
                .lock()
                .map_err(|_| "设置状态被污染".to_string())?;
            *slot = next.clone();
        }
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let raw = serde_json::to_string_pretty(&next).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, raw).map_err(|e| format!("写入设置失败：{e}"))?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("trans-settings-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn defaults_to_double_control() {
        let settings = Settings::default();
        assert_eq!(settings.trigger, Trigger::DoubleControl);
        assert_eq!(settings.hotkey, DEFAULT_HOTKEY);
    }

    #[test]
    fn survives_a_round_trip() {
        let dir = temp_dir("roundtrip");
        let store = SettingsStore::load(&dir);
        assert_eq!(store.get().trigger, Trigger::DoubleControl);

        store
            .set(Settings {
                trigger: Trigger::Hotkey,
                hotkey: "Ctrl+Shift+D".to_string(),
            })
            .expect("写入应成功");

        let reloaded = SettingsStore::load(&dir);
        let settings = reloaded.get();
        assert_eq!(settings.trigger, Trigger::Hotkey);
        assert_eq!(settings.hotkey, "Ctrl+Shift+D");
    }

    #[test]
    fn a_corrupt_file_falls_back_to_defaults() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), "{ not json").unwrap();

        let store = SettingsStore::load(&dir);
        assert_eq!(store.get().trigger, Trigger::DoubleControl);
    }
}
