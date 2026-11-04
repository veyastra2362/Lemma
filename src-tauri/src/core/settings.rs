//! 用户设置：触发方式、取词模式、卡片展示、HUD 尺寸。落盘到 `<app_data>/settings.json`。
//!
//! 故意不用 tauri-plugin-store：字段少，手写 JSON 更小更可控，
//! 而且序列化错误可以给出可读的报错。
//!
//! **新增字段一律加 `#[serde(default)]`**，否则老的 settings.json 会导致整体反序列化失败、
//! 用户设置被静默重置。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::core::translate::{Lang, ProviderKind};

/// 呼出方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Trigger {
    /// 双击 Ctrl（默认）。需要只读键盘观察。
    DoubleControl,
    /// 全局热键。
    Hotkey,
}

/// 取词模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureMode {
    /// 有选区就读选区，没有就取光标下的词（默认）。
    Auto,
    /// 只读选区。浏览器场景推荐——实测 Chrome 选区 8ms 且稳定，悬停取词不稳定。
    SelectionOnly,
    /// 只取光标下的词。适合原生应用里"悬停即查"。
    HoverOnly,
}

/// 卡片上显示哪些区块。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardSections {
    pub phonetic: bool,
    pub translation: bool,
    pub definition: bool,
    pub tags: bool,
    pub exchange: bool,
}

impl Default for CardSections {
    fn default() -> Self {
        // 默认只放"读一眼就够"的信息：音标 + 中文释义 + 考试标签。
        // 英文释义和词形变化收起来——深度阅读时信息过载比信息不足更烦。
        Self {
            phonetic: true,
            translation: true,
            definition: false,
            tags: true,
            exchange: false,
        }
    }
}

/// 默认热键。注意 `RegisterHotKey` 是独占的，这个键很可能已被别的软件占用，
/// 所以真正的默认触发是双击 Ctrl。
pub const DEFAULT_HOTKEY: &str = "Alt+Q";

/// HUD 逻辑宽度（固定），自适应只改高度。
pub const HUD_WIDTH: f64 = 420.0;
/// HUD 最小高度。
pub const HUD_MIN_HEIGHT: f64 = 132.0;
/// HUD 最大高度的上限（用户可在这个范围内调）。
pub const HUD_MAX_HEIGHT_LIMIT: f64 = 720.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub trigger: Trigger,
    /// 形如 `Alt+Q` / `Ctrl+Shift+D`，仅在 `trigger == Hotkey` 时生效。
    pub hotkey: String,
    #[serde(default = "default_capture_mode")]
    pub capture_mode: CaptureMode,
    /// HUD 自适应的最大高度（逻辑像素）。
    #[serde(default = "default_hud_max_height")]
    pub hud_max_height: u32,
    /// 是否显示 HUD 底部那排快捷键提示。
    #[serde(default = "default_true")]
    pub show_hints: bool,
    /// 翻译走哪个服务。
    #[serde(default = "default_provider")]
    pub translate_provider: ProviderKind,
    /// 目标语言。
    #[serde(default = "default_target_lang")]
    pub target_lang: Lang,
    #[serde(default = "default_base_url")]
    pub openai_base_url: String,
    /// 明文存在 settings.json 里——这是本机个人工具，不做密钥托管；
    /// 但**不要**把它提交进仓库（`.gitignore` 已挡）。
    #[serde(default)]
    pub openai_api_key: String,
    #[serde(default = "default_model")]
    pub openai_model: String,
    #[serde(default)]
    pub card_sections: CardSections,
}

fn default_capture_mode() -> CaptureMode {
    CaptureMode::Auto
}

fn default_hud_max_height() -> u32 {
    480
}

fn default_true() -> bool {
    true
}

fn default_provider() -> ProviderKind {
    ProviderKind::Google
}

fn default_target_lang() -> Lang {
    Lang::Zh
}

fn default_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            trigger: Trigger::DoubleControl,
            hotkey: DEFAULT_HOTKEY.to_string(),
            capture_mode: default_capture_mode(),
            hud_max_height: default_hud_max_height(),
            show_hints: true,
            translate_provider: default_provider(),
            target_lang: default_target_lang(),
            openai_base_url: default_base_url(),
            openai_api_key: String::new(),
            openai_model: default_model(),
            card_sections: CardSections::default(),
        }
    }
}

impl Settings {
    /// 把用户填的最大高度钳进合法范围。
    pub fn clamped_max_height(&self) -> f64 {
        (self.hud_max_height as f64).clamp(HUD_MIN_HEIGHT, HUD_MAX_HEIGHT_LIMIT)
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
        let dir = std::env::temp_dir().join(format!("lemma-settings-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn defaults_are_sane() {
        let settings = Settings::default();
        assert_eq!(settings.trigger, Trigger::DoubleControl);
        assert_eq!(settings.hotkey, DEFAULT_HOTKEY);
        assert_eq!(settings.capture_mode, CaptureMode::Auto);
        assert_eq!(settings.hud_max_height, 480);
        assert!(settings.card_sections.translation);
        // 默认收起英文释义与词形变化
        assert!(!settings.card_sections.definition);
        assert!(!settings.card_sections.exchange);
    }

    #[test]
    fn survives_a_round_trip() {
        let dir = temp_dir("roundtrip");
        let store = SettingsStore::load(&dir);
        assert_eq!(store.get().trigger, Trigger::DoubleControl);

        let mut next = store.get();
        next.trigger = Trigger::Hotkey;
        next.hotkey = "Ctrl+Shift+D".to_string();
        next.capture_mode = CaptureMode::SelectionOnly;
        next.hud_max_height = 600;
        next.card_sections.definition = true;
        store.set(next).expect("写入应成功");

        let reloaded = SettingsStore::load(&dir);
        let settings = reloaded.get();
        assert_eq!(settings.trigger, Trigger::Hotkey);
        assert_eq!(settings.hotkey, "Ctrl+Shift+D");
        assert_eq!(settings.capture_mode, CaptureMode::SelectionOnly);
        assert_eq!(settings.hud_max_height, 600);
        assert!(settings.card_sections.definition);
    }

    /// 老版本的 settings.json 只有 trigger + hotkey，新字段必须能补上默认值，
    /// 而不是整个文件反序列化失败、把用户设置清空。
    #[test]
    fn old_settings_files_gain_new_defaults() {
        let dir = temp_dir("legacy");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{ "trigger": "hotkey", "hotkey": "Alt+X" }"#,
        )
        .unwrap();

        let store = SettingsStore::load(&dir);
        let settings = store.get();
        assert_eq!(settings.trigger, Trigger::Hotkey);
        assert_eq!(settings.hotkey, "Alt+X");
        assert_eq!(settings.capture_mode, CaptureMode::Auto);
        assert_eq!(settings.hud_max_height, 480);
        assert!(settings.card_sections.translation);
    }

    #[test]
    fn a_corrupt_file_falls_back_to_defaults() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), "{ not json").unwrap();

        let store = SettingsStore::load(&dir);
        assert_eq!(store.get().trigger, Trigger::DoubleControl);
    }

    #[test]
    fn max_height_is_clamped() {
        let tiny = Settings {
            hud_max_height: 5,
            ..Default::default()
        };
        assert_eq!(tiny.clamped_max_height(), HUD_MIN_HEIGHT);

        let huge = Settings {
            hud_max_height: 99_999,
            ..Default::default()
        };
        assert_eq!(huge.clamped_max_height(), HUD_MAX_HEIGHT_LIMIT);
    }
}
