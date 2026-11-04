//! 朗读子进程客户端。
//!
//! 为什么要有这一层：Kokoro 中文模型加载后主进程 RSS 会从 30 MB 涨到 **332 MB**
//! （见 `docs/PERF.md` §3.5），是内存口径的 5–10 倍。把模型关进子进程后，
//! 主进程只负责"喂一句文本、拿一个 WAV 路径、播出来"。
//!
//! 协议：一行 JSON 请求 → 一行 JSON 响应（日志走 stderr，不污染通道）。
//!
//! 进程生命周期：
//! - 第一次朗读时拉起 `lemma.exe --tts-worker`；
//! - 空闲超过 `IDLE_TIMEOUT` 由后台线程杀掉；
//! - 主进程退出时 stdin 关闭，worker 读到 EOF 自己退出（所以强杀主进程也不会留孤儿）。

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 空闲多久后杀掉 worker。
///
/// 权衡：留着能省掉下次约 1.2 s 的模型加载；但它是 332 MB 的常驻，
/// 而朗读是低频操作，所以 60 秒后就还回去。
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// 回收线程的检查间隔。测试时可调小（`LEMMA_TTS_IDLE_SECS`）。
const REAP_INTERVAL: Duration = Duration::from_secs(5);

fn idle_timeout() -> Duration {
    std::env::var("LEMMA_TTS_IDLE_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(IDLE_TIMEOUT)
}

struct Worker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    last_used: Instant,
}

static WORKER: Mutex<Option<Worker>> = Mutex::new(None);

/// 合成结果。
pub struct WorkerSpeech {
    pub path: PathBuf,
    pub duration_secs: f32,
}

/// 请 worker 合成一段语音，返回 WAV 路径。
///
/// **传音素串而不是原文**：拼音要查 redb 词库，而词库被主进程独占加锁，
/// 子进程打开会直接失败（`Database already open`）。
/// 所以切分点在音素边界——主进程负责文本→音素（便宜、已持锁），worker 只负责音素→音频。
pub fn speak(phonemes: &str, speed: f32) -> Result<WorkerSpeech, String> {
    let mut guard = WORKER.lock().map_err(|_| "worker 锁被毒化".to_string())?;

    // 已经有 worker 但已经死了（崩溃/被杀）：丢掉重来
    if let Some(worker) = guard.as_mut()
        && worker.child.try_wait().ok().flatten().is_some()
    {
        *guard = None;
    }

    if guard.is_none() {
        *guard = Some(spawn()?);
    }
    let worker = guard
        .as_mut()
        .ok_or_else(|| "worker 启动失败".to_string())?;

    let request = serde_json::json!({ "phonemes": phonemes, "speed": speed });
    writeln!(worker.stdin, "{request}").map_err(|e| format!("写入 worker 失败：{e}"))?;
    worker
        .stdin
        .flush()
        .map_err(|e| format!("刷新 worker 管道失败：{e}"))?;

    let mut line = String::new();
    loop {
        line.clear();
        let read = worker
            .stdout
            .read_line(&mut line)
            .map_err(|e| format!("读取 worker 失败：{e}"))?;
        if read == 0 {
            // worker 挂了：清掉，下次重新拉起
            *guard = None;
            return Err("朗读子进程意外退出（下次会自动重启）".to_string());
        }
        let trimmed = line.trim();
        // 防御：万一有非 JSON 的行混进来（日志没改道成功），跳过而不是报错
        if !trimmed.starts_with('{') {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|e| format!("worker 响应解析失败：{e}"))?;
        if value.get("ok").and_then(|ok| ok.as_bool()).unwrap_or(false) {
            let path = value
                .get("path")
                .and_then(|path| path.as_str())
                .ok_or_else(|| "worker 没返回路径".to_string())?;
            let duration_secs = value
                .get("durationSecs")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0) as f32;
            worker.last_used = Instant::now();
            return Ok(WorkerSpeech {
                path: PathBuf::from(path),
                duration_secs,
            });
        }
        let error = value
            .get("error")
            .and_then(|error| error.as_str())
            .unwrap_or("未知错误")
            .to_string();
        return Err(error);
    }
}

fn spawn() -> Result<Worker, String> {
    let exe = std::env::current_exe().map_err(|e| format!("拿不到自身路径：{e}"))?;
    let mut child = Command::new(exe)
        .arg("--tts-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("拉起朗读子进程失败：{e}"))?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "拿不到子进程 stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "拿不到子进程 stdout".to_string())?;
    crate::core::logs::line("tts worker spawned");

    // 独立的回收线程：不能只靠"下次朗读时检查"——用户可能只读一次，
    // 那样 worker 会一直揣着 300 MB 活到应用退出。线程在 worker 被回收后自行结束。
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(REAP_INTERVAL);
            if !running() {
                return;
            }
            reap_if_idle();
        }
    });

    Ok(Worker {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        last_used: Instant::now(),
    })
}

/// 空闲就杀掉 worker。由调用方定期触发（不需要独立线程）。
pub fn reap_if_idle() {
    let Ok(mut guard) = WORKER.lock() else {
        return;
    };
    let Some(worker) = guard.as_mut() else {
        return;
    };
    if worker.last_used.elapsed() < idle_timeout() {
        return;
    }
    let _ = worker.child.kill();
    let _ = worker.child.wait();
    *guard = None;
    crate::core::logs::line("tts worker reaped (idle)");
}

/// worker 是否在跑（界面可用来显示"已加载"）。
pub fn running() -> bool {
    WORKER.lock().map(|guard| guard.is_some()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_timeout_is_generous_enough() {
        // 60 秒是权衡结果：太短白付模型加载，太长又长期占着 332 MB
        assert!(IDLE_TIMEOUT >= Duration::from_secs(30));
        assert!(IDLE_TIMEOUT <= Duration::from_secs(120));
    }
}
