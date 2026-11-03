//! 应用数据目录。
//!
//! 不依赖 Tauri，因此 CLI（导入词库、取词探针）也能用同一套路径。

use std::path::PathBuf;

/// `%APPDATA%\dev.lunar.trans`（与 Tauri 的 `app_data_dir()` 一致）。
pub fn app_data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    Some(base.join("dev.lunar.trans"))
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
