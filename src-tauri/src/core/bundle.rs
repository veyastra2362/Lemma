//! 把安装包里自带的模型权重复制到应用数据目录。
//!
//! 为什么是"复制"而不是"直接读安装目录"：
//! - `core::ocr` / `core::tts` 的路径约定是「应用数据目录下的固定文件夹」，
//!   直接改读取路径会牵动两处模型加载与设置页的状态判断；
//! - 复制后用户仍可自行替换/删除模型（与以前的行为一致）。
//!
//! 只在**缺失时**复制，已有文件一律不动，避免覆盖用户自己换的模型。

use tauri::{AppHandle, Manager, Runtime};

/// 目录型资源：（安装目录相对路径 → 数据目录子目录）。
const BUNDLED_DIRS: [(&str, &str); 2] =
    [("models/ocr", "ocr-models"), ("models/tts", "tts-models")];

/// 单文件型资源：（安装目录相对路径 → 数据目录文件名）。
///
/// `dict.redb` 只含 ECDICT + CC-CEDICT（中英互译所需）；汉语词典不在其中，
/// 那份数据版权不明确，由用户 `--import-zh-defs` 自己导。
const BUNDLED_FILES: [(&str, &str); 1] = [("models/dict.redb", "dict.redb")];

/// 把安装包自带的模型落到数据目录；返回复制了多少个文件。
pub fn seed_models<R: Runtime>(app: &AppHandle<R>) -> usize {
    let Ok(resource_dir) = app.path().resource_dir() else {
        return 0;
    };
    let Some(data_dir) = crate::core::paths::app_data_dir() else {
        return 0;
    };

    let mut copied = 0;
    for (from, to) in BUNDLED_DIRS {
        let source = resource_dir.join(from);
        if !source.is_dir() {
            continue;
        }
        let target = data_dir.join(to);
        if std::fs::create_dir_all(&target).is_err() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&source) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            let dest = target.join(name);
            // 已有就跳过：用户可以自己换模型，不能被安装包覆盖
            if dest.exists() {
                continue;
            }
            if std::fs::copy(&path, &dest).is_ok() {
                copied += 1;
            }
        }
    }
    for (from, to) in BUNDLED_FILES {
        let source = resource_dir.join(from);
        if !source.is_file() {
            continue;
        }
        let dest = data_dir.join(to);
        if dest.exists() {
            continue;
        }
        if std::fs::copy(&source, &dest).is_ok() {
            copied += 1;
        }
    }

    copied
}
