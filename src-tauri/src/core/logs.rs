//! 极简日志：同时写到 stderr 与应用数据目录下的文件。
//!
//! 为什么要落文件：发布版是 `windows_subsystem = "windows"`，没有控制台，
//! 而 `scripts/perf-baseline.ps1` 需要读这些打点来算冷启动时间。
//! M0 先自己写，等 M7 再接 tracing —— 那时的环形日志与结构化字段才有必要。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// 进程启动时间，用于给出「相对启动的毫秒偏移」。
static START: OnceLock<SystemTime> = OnceLock::new();

pub fn init(path: PathBuf) {
    let _ = START.set(SystemTime::now());
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = LOG_PATH.set(path);
}

fn elapsed_ms() -> u128 {
    START
        .get()
        .and_then(|start| SystemTime::now().duration_since(*start).ok())
        .map(|delta| delta.as_millis())
        .unwrap_or(0)
}

/// 写一行日志，形如 `[+123ms] ...`。
pub fn line(message: impl AsRef<str>) {
    let message = message.as_ref();
    let rendered = format!("[+{}ms] {}", elapsed_ms(), message);

    println!("{rendered}");
    let Some(path) = LOG_PATH.get() else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{rendered}");
    }
}

/// 日志文件的纪元秒级时间戳，便于区分不同次运行。
pub fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_millis())
        .unwrap_or(0)
}
