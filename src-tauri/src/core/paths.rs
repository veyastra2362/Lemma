//! 应用数据目录。
//!
//! 不依赖 Tauri，因此 CLI（导入词库、取词探针）也能用同一套路径。
//!
//! 注意这里的目录名必须与 `tauri.conf.json` 的 `identifier` 保持一致：
//! Tauri 的 `app_data_dir()` = `%APPDATA%\<identifier>`，两边对不上就会
//! 出现「GUI 写在 A、CLI 读 B」的诡异分叉。

use std::path::PathBuf;

/// 当前标识，与 `tauri.conf.json::identifier` 相同。
pub const IDENTIFIER: &str = "dev.lunar.lemma";

/// 改名前的标识。只用于一次性数据迁移。
pub const LEGACY_IDENTIFIER: &str = "dev.lunar.trans";

fn roaming_dir(identifier: &str) -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    Some(base.join(identifier))
}

/// `%APPDATA%\dev.lunar.lemma`（与 Tauri 的 `app_data_dir()` 一致）。
pub fn app_data_dir() -> Option<PathBuf> {
    roaming_dir(IDENTIFIER)
}

/// 改名前的目录，仅供迁移使用。
pub fn legacy_app_data_dir() -> Option<PathBuf> {
    roaming_dir(LEGACY_IDENTIFIER)
}

/// 中文词表侧文件（每行一个词）。
///
/// 为什么单独存一份：分词器只需要词，不需要词条内容。遍历 redb 中文表会
/// 把整张表的页都带进工作集（实测多占约 23 MB），顺序读这个 1.5 MB 的文件则不会。
pub fn zh_words_path() -> Option<PathBuf> {
    app_data_dir().map(|dir| dir.join("zh-words.txt"))
}

/// 词库文件路径。
pub fn dict_path() -> Option<PathBuf> {
    app_data_dir().map(|dir| dir.join("dict.redb"))
}

pub fn ensure_dir(path: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dict_lives_under_the_current_identifier() {
        if let Some(dict) = dict_path() {
            assert!(dict.ends_with("dict.redb"));
            assert!(
                dict.to_string_lossy().contains(IDENTIFIER),
                "词库路径应包含当前标识：{}",
                dict.display()
            );
        }
    }

    #[test]
    fn legacy_dir_differs_from_the_current_one() {
        assert_ne!(IDENTIFIER, LEGACY_IDENTIFIER);
    }
}
