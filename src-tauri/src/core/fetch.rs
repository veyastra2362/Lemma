//! 可选功能（朗读模型）的按需下载。
//!
//! 安装包带「程序 + 英汉词库 + OCR 模型」；125 MB 的朗读模型单独下发，
//! 且**打成一个 zip**——发布时只需要上传一个文件，用户也只下一次。
//!
//! 下载到 `tts-models.zip` → 校验 SHA-256 → 解包到 `%APPDATA%\dev.lunar.lemma\tts-models\`，
//! 解出来的路径与原内置布局一致，上层 `models_ready()` 不需要知道文件是哪来的。
//!
//! 下载源默认是本仓库的 GitHub Release 资产，可用 `LEMMA_MODEL_BASE_URL` 覆盖
//! （本地 `python -m http.server` 就能端到端验证）。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// 朗读模型压缩包。
///
/// 里面只有 4 个文件，且都是固定的名字；解包时按白名单取，避免 zip 里的路径穿越。
pub struct Archive {
    /// 压缩包文件名（也是 Release 资产名）。
    pub file: &'static str,
    /// 解开后落到数据目录的哪个子目录。
    pub subdir: &'static str,
    /// 解出来的文件名白名单。
    pub members: &'static [&'static str],
    pub bytes: u64,
    pub sha256: &'static str,
}

pub const TTS: Archive = Archive {
    file: "tts-models.zip",
    subdir: "tts-models",
    members: &[
        "kokoro-zh-q8.onnx",
        "cmudict.dict",
        "zf_001.bin",
        "tokenizer.json",
    ],
    bytes: 90_666_483,
    sha256: "03d62e3df84bc39d8de096390c848d0b88bdf38c95d7fc891c8b46d7b7ec6ed9",
};

/// 默认下载源：本仓库的 Release 资产。
const DEFAULT_BASE: &str = "https://github.com/veyastra2362/Lemma/releases/download/models-v1";

fn base_url() -> String {
    std::env::var("LEMMA_MODEL_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE.to_string())
}

/// 下载进度事件（`models://download`）。
#[derive(Serialize, Clone)]
pub struct Progress {
    pub kind: String,
    pub file: String,
    pub done: u64,
    pub total: u64,
    /// `progress` / `verify` / `extract` / `done` / `error`
    pub state: String,
    pub message: String,
}

/// 下载并安装朗读模型；已完成时返回 `Ok(0)`（不重复下载）。
pub fn fetch_tts(mut on_progress: impl FnMut(Progress)) -> Result<usize, String> {
    let data_dir = crate::core::paths::app_data_dir().ok_or("拿不到应用数据目录")?;
    let target = data_dir.join(TTS.subdir);
    std::fs::create_dir_all(&target).map_err(|e| format!("建目录失败：{e}"))?;

    // 四个文件都在就已经装好了
    if TTS.members.iter().all(|name| target.join(name).exists()) {
        return Ok(0);
    }

    let url = format!("{}/{}", base_url(), TTS.file);
    let archive_path = data_dir.join(TTS.file);
    let tmp = archive_path.with_extension("part");

    on_progress(Progress {
        kind: "tts".to_string(),
        file: TTS.file.to_string(),
        done: 0,
        total: TTS.bytes,
        state: "progress".to_string(),
        message: String::new(),
    });

    let mut response = reqwest::blocking::get(&url)
        .map_err(|e| format!("连接下载源失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("下载源返回错误：{e}（检查 Release 里是否有 {}）", TTS.file))?;

    let mut file = std::fs::File::create(&tmp).map_err(|e| format!("建文件失败：{e}"))?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
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
        done += read as u64;
        // 每 1% 或每 8 MB 报一次，避免事件刷屏
        if done - last_report >= (TTS.bytes / 100).max(8 * 1024 * 1024) {
            last_report = done;
            on_progress(Progress {
                kind: "tts".to_string(),
                file: TTS.file.to_string(),
                done,
                total: TTS.bytes,
                state: "progress".to_string(),
                message: String::new(),
            });
        }
    }
    file.flush().map_err(|e| format!("落盘失败：{e}"))?;
    drop(file);

    on_progress(Progress {
        kind: "tts".to_string(),
        file: TTS.file.to_string(),
        done,
        total: TTS.bytes,
        state: "verify".to_string(),
        message: String::new(),
    });
    let actual = sha256_file(&tmp)?;
    if actual != TTS.sha256 {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "压缩包校验失败（期望 {}，实际 {actual}）",
            TTS.sha256
        ));
    }

    on_progress(Progress {
        kind: "tts".to_string(),
        file: TTS.file.to_string(),
        done,
        total: TTS.bytes,
        state: "extract".to_string(),
        message: String::new(),
    });
    let written = extract(&tmp, &target)?;
    // 解完就删：留着要多占 86 MB，而重新下载的成本远低于这个常驻占用
    let _ = std::fs::remove_file(&tmp);
    crate::core::logs::line(format!(
        "tts models installed files={written} archive={:.1}MB",
        done as f64 / 1048576.0
    ));

    on_progress(Progress {
        kind: "tts".to_string(),
        file: TTS.file.to_string(),
        done,
        total: TTS.bytes,
        state: "done".to_string(),
        message: String::new(),
    });
    Ok(written)
}

/// 只解白名单里的文件。
///
/// 不做「按条目路径落盘」：那样必须自己防 `../` 穿越，而这里文件名是固定的四个，
/// 直接按名字取更简单也更安全（多出来的条目一律忽略）。
fn extract(archive: &Path, target: &Path) -> Result<usize, String> {
    let file = std::fs::File::open(archive).map_err(|e| format!("打开压缩包失败：{e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("解析压缩包失败：{e}"))?;

    let mut written = 0usize;
    for name in TTS.members {
        let Ok(mut entry) = zip.by_name(name) else {
            return Err(format!("压缩包里缺少 {name}"));
        };
        let dest = target.join(name);
        let tmp = dest.with_extension("part");
        let mut out = std::fs::File::create(&tmp).map_err(|e| format!("建文件失败：{e}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("解包失败：{e}"))?;
        out.flush().map_err(|e| format!("落盘失败：{e}"))?;
        drop(out);
        std::fs::rename(&tmp, &dest).map_err(|e| format!("改名失败：{e}"))?;
        written += 1;
    }
    Ok(written)
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
