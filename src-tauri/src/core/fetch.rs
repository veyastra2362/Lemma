//! 可选功能（朗读 / OCR 模型）的按需下载。
//!
//! 安装包只带「程序 + 英汉词库」，86 MiB 的朗读模型与 13 MiB 的 OCR 模型改成点按钮下载：
//! 下载到 `%APPDATA%\dev.lunar.lemma\<模型目录>\`，校验 SHA-256 后与原内置路径完全一致，
//! 上层 `models_ready()` / `model_dir()` 不需要知道文件是下载来的还是安装来的。
//!
//! 下载源放在 GitHub Releases（仓库已公开），可用环境变量 `LEMMA_MODEL_BASE_URL` 覆盖，
//! 便于本地用 `python -m http.server` 验证或换 CDN。

use std::io::{Read, Write};
use std::path::PathBuf;

use serde::Serialize;

/// 一个要下载的文件。
pub struct Asset {
    pub name: &'static str,
    /// 落在数据目录的哪个子目录（空表示数据目录根）。
    pub subdir: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

/// 朗读模型（Kokoro，84 MiB 压缩后）。
pub const TTS: &[Asset] = &[
    Asset {
        name: "kokoro-zh-q8.onnx",
        subdir: "tts-models",
        bytes: 127_356_597,
        sha256: "a39469be791eeaa3089c1ed5e58b8731d1f2462ea0e7dae2bc44388e58f973d8",
    },
    Asset {
        name: "cmudict.dict",
        subdir: "tts-models",
        bytes: 3_618_488,
        sha256: "81917843c7f44ce2b094ac63873c2c7a4cf802040792c455ba3ca406891c3d22",
    },
    Asset {
        name: "zf_001.bin",
        subdir: "tts-models",
        bytes: 522_240,
        sha256: "0a89ec12bb93fb9c74077924daf02568baad64e1f869389f5aaee01a386035f8",
    },
    Asset {
        name: "tokenizer.json",
        subdir: "tts-models",
        bytes: 4_944,
        sha256: "5715a60b09d5e4b9074435d68c6ccd5675b9d48b220e109fdea3cda681e23d15",
    },
];

/// OCR 模型（PP-OCRv4 mobile）。
pub const OCR: &[Asset] = &[
    Asset {
        name: "det.onnx",
        subdir: "ocr-models",
        bytes: 4_745_517,
        sha256: "d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9",
    },
    Asset {
        name: "rec.onnx",
        subdir: "ocr-models",
        bytes: 10_857_958,
        sha256: "48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b",
    },
    Asset {
        name: "ppocr_keys_v1.txt",
        subdir: "ocr-models",
        bytes: 26_250,
        sha256: "a1c84d9bdb9ab29043c58896224d32941783eb821629618416dcb08f12886492",
    },
];

/// 默认下载源：本仓库的 Release 资产。
const DEFAULT_BASE: &str = "https://github.com/veyastra2362/Lemma/releases/download/models-v1";

fn base_url() -> String {
    std::env::var("LEMMA_MODEL_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE.to_string())
}

/// 下载进度事件（`models://download`）。
#[derive(Serialize, Clone)]
pub struct Progress {
    /// `tts` 或 `ocr`
    pub kind: String,
    pub file: String,
    /// 已下载字节数（跨文件累计）。
    pub done: u64,
    /// 总字节数。
    pub total: u64,
    /// `start` / `progress` / `verify` / `done` / `error`
    pub state: String,
    pub message: String,
}

/// 把一组资源下载到数据目录；`on_progress` 每 1% 或每切换文件时回调一次。
///
/// 已存在且大小一致的文件直接跳过（重复点按钮不会白下 84 MiB）。
pub fn fetch(assets: &[Asset], mut on_progress: impl FnMut(Progress)) -> Result<usize, String> {
    let data_dir = crate::core::paths::app_data_dir().ok_or("拿不到应用数据目录")?;
    let base = base_url();
    let kind = if std::ptr::eq(assets, TTS) {
        "tts"
    } else {
        "ocr"
    };
    let total: u64 = assets.iter().map(|asset| asset.bytes).sum();
    let mut done_before = 0u64;
    let mut fetched = 0usize;

    for asset in assets {
        let dir = data_dir.join(asset.subdir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("建目录失败：{e}"))?;
        let dest = dir.join(asset.name);

        // 已经下过且大小一致就跳过（重复点按钮不会白下几十 MB）
        if std::fs::metadata(&dest).is_ok_and(|meta| meta.len() == asset.bytes) {
            done_before += asset.bytes;
            continue;
        }

        let url = format!("{base}/{}", asset.name);
        let mut response = reqwest::blocking::get(&url)
            .map_err(|e| format!("连接下载源失败：{e}"))?
            .error_for_status()
            .map_err(|e| format!("下载源返回错误：{e}"))?;

        let tmp = dest.with_extension("part");
        let mut file = std::fs::File::create(&tmp).map_err(|e| format!("建文件失败：{e}"))?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut this_file = 0u64;
        let mut last_report = 0u64;

        loop {
            let read = response
                .read(&mut buf)
                .map_err(|e| format!("读取数据失败：{e}"))?;
            if read == 0 {
                break;
            }
            file.write_all(&buf[..read])
                .map_err(|e| format!("写文件失败：{e}"))?;
            this_file += read as u64;
            let done = done_before + this_file;
            // 每 1% 或每 8 MB 报一次，避免事件刷屏
            if done - last_report >= (total / 100).max(8 * 1024 * 1024) {
                last_report = done;
                on_progress(Progress {
                    kind: kind.to_string(),
                    file: asset.name.to_string(),
                    done,
                    total,
                    state: "progress".to_string(),
                    message: String::new(),
                });
            }
        }
        file.flush().map_err(|e| format!("落盘失败：{e}"))?;
        drop(file);

        // 校验：宁可失败也不要留下半坏模型
        on_progress(Progress {
            kind: kind.to_string(),
            file: asset.name.to_string(),
            done: done_before + this_file,
            total,
            state: "verify".to_string(),
            message: String::new(),
        });
        let actual = sha256_file(&tmp)?;
        if actual != asset.sha256 {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!(
                "{} 校验失败（期望 {}，实际 {}）",
                asset.name, asset.sha256, actual
            ));
        }
        std::fs::rename(&tmp, &dest).map_err(|e| format!("改名失败：{e}"))?;
        done_before += this_file;
        fetched += 1;
        crate::core::logs::line(format!(
            "model fetched {} ({:.1} MB)",
            asset.name,
            this_file as f64 / 1048576.0
        ));
    }

    Ok(fetched)
}

fn sha256_file(path: &PathBuf) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| format!("打开文件失败：{e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buf).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
