//! Native Messaging 宿主：浏览器扩展 ↔ 本应用。
//!
//! 协议是 Chrome/Firefox 官方的 stdio 帧格式：**4 字节小端长度 + UTF-8 JSON**。
//! 只实现三个方法（见 `docs/EXTENSION.md`），未知方法一律拒绝。
//!
//! 为什么不在本地开端口：那会弹防火墙、而且任何本机进程都能连；
//! Native Messaging 只接受 manifest 里白名单的扩展 ID。

use std::io::{Read, Write};

/// 单次请求的文本上限（防止扩展被利用来灌大文本）。
const MAX_TEXT: usize = 8192;

#[derive(serde::Deserialize)]
struct Request {
    method: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    target: Option<String>,
}

#[derive(serde::Serialize)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cached: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Response {
    fn ok() -> Self {
        Self {
            ok: true,
            text: None,
            provider: None,
            cached: None,
            version: None,
            error: None,
        }
    }

    fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            text: None,
            provider: None,
            cached: None,
            version: None,
            error: Some(message.into()),
        }
    }
}

/// 跑 Native Messaging 循环；stdin 关闭时返回。
pub fn run() {
    // 必须最先做：stdout 是协议通道（4 字节长度 + JSON），
    // 任何日志写进 stdout 都会把帧头写坏（实测：日志混进去后浏览器读到的"长度"是乱的）。
    crate::core::logs::silence_stdout();

    // 词库与翻译都要用；词库缺失不致命（翻译仍可用）
    let _ = crate::core::dict::open_default();

    // 翻译链路里有跨 await 的锁；单线程 runtime 会自锁（实测卡死 30 秒无响应）
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            crate::core::logs::line(format!("native host: 建运行时失败 {error}"));
            return;
        }
    };

    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let output = std::sync::Arc::new(std::sync::Mutex::new(std::io::stdout()));

    // 反向通道：GUI 点「回填」→ 管道 → 这里 → 写成帧推给扩展。
    // 扩展回话的帧带 `id` 而不带 `method`，在下面的循环里按 id 派发。
    let pending: Pending =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    {
        let output = output.clone();
        let pending = pending.clone();
        crate::platform::serve_writeback_pipe(move |_gui_id, text| {
            push_replace(&output, &pending, text)
        });
    }

    while let Some(body) = read_frame(&mut input) {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) else {
            let _ = write_response(&output, Response::err("请求不是合法 JSON"));
            continue;
        };

        // 扩展对推送的回应：{"id":N,"ok":true}
        if let Some(id) = value.get("id").and_then(|v| v.as_u64()) {
            let reply = if value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                crate::platform::pipe::Reply::ok()
            } else {
                crate::platform::pipe::Reply::err(
                    value
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("回填失败")
                        .to_string(),
                )
            };
            if let Some(tx) = pending.lock().ok().and_then(|mut map| map.remove(&id)) {
                let _ = tx.send(reply);
            }
            continue;
        }

        let request = match serde_json::from_value::<Request>(value) {
            Ok(request) => request,
            Err(error) => {
                let _ = write_response(&output, Response::err(format!("请求缺少 method：{error}")));
                continue;
            }
        };
        let response = handle(&runtime, request);
        if write_response(&output, response).is_err() {
            break;
        }
    }
    crate::core::logs::line("native host: stdin closed, exiting");
}

/// 待回的推送请求：id → 收结果的通道。
type Pending = std::sync::Arc<
    std::sync::Mutex<
        std::collections::HashMap<u64, std::sync::mpsc::Sender<crate::platform::pipe::Reply>>,
    >,
>;

fn write_response(
    output: &std::sync::Mutex<std::io::Stdout>,
    response: Response,
) -> std::io::Result<()> {
    let payload = serde_json::to_vec(&response).map_err(std::io::Error::other)?;
    write_frame(output, &payload)
}

/// 把「替换这段文本」推给扩展，并等它回话。
fn push_replace(
    output: &std::sync::Mutex<std::io::Stdout>,
    pending: &Pending,
    text: String,
) -> crate::platform::pipe::Reply {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let (tx, rx) = std::sync::mpsc::channel();
    if let Ok(mut map) = pending.lock() {
        map.insert(id, tx);
    }

    let frame = serde_json::json!({ "type": "replace", "id": id, "text": text });
    let Ok(payload) = serde_json::to_vec(&frame) else {
        return crate::platform::pipe::Reply::err("序列化失败");
    };
    if write_frame(output, &payload).is_err() {
        if let Ok(mut map) = pending.lock() {
            map.remove(&id);
        }
        return crate::platform::pipe::Reply::err("宿主写不了 stdio（浏览器可能已断开）");
    }

    match rx.recv_timeout(std::time::Duration::from_secs(8)) {
        Ok(reply) => reply,
        Err(_) => {
            if let Ok(mut map) = pending.lock() {
                map.remove(&id);
            }
            crate::platform::pipe::Reply::err("浏览器没有响应（扩展未连接，或页面里没选中文本）")
        }
    }
}

fn handle(runtime: &tokio::runtime::Runtime, request: Request) -> Response {
    match request.method.as_str() {
        "ping" => {
            let mut response = Response::ok();
            response.version = Some(env!("CARGO_PKG_VERSION").to_string());
            response
        }
        "translate" => {
            // 设置里关掉「浏览器回填」后，扩展发来的请求一律拒绝
            let data_dir = crate::core::paths::app_data_dir().unwrap_or_default();
            let settings = crate::core::settings::SettingsStore::load(&data_dir).get();
            if !settings.extension_write_back {
                return Response::err("浏览器回填已在 Lemma 设置里关闭");
            }
            if request.text.trim().is_empty() {
                return Response::err("text 不能为空");
            }
            if request.text.chars().count() > MAX_TEXT {
                return Response::err(format!("text 超过 {MAX_TEXT} 字符上限"));
            }
            translate(runtime, request.text, request.target)
        }
        other => Response::err(format!("未知方法：{other}")),
    }
}

fn translate(runtime: &tokio::runtime::Runtime, text: String, target: Option<String>) -> Response {
    // 设置来自磁盘（宿主要和应用用同一份配置：provider / key / 目标语言）
    let data_dir = crate::core::paths::app_data_dir().unwrap_or_default();
    let settings = crate::core::settings::SettingsStore::load(&data_dir).get();
    let mut config = crate::core::translate::TranslateConfig {
        provider: settings.translate_provider,
        target: settings.target_lang,
        base_url: settings.openai_base_url,
        api_key: settings.openai_api_key,
        model: settings.openai_model,
    };
    if let Some(target) = target {
        match target.as_str() {
            "en" => config.target = crate::core::translate::Lang::En,
            "zh" => config.target = crate::core::translate::Lang::Zh,
            other => return Response::err(format!("不支持的目标语言：{other}")),
        }
    }

    // 用 `run()` 建好的多线程 runtime：翻译链路里有跨 await 的锁，
    // 单线程 runtime 会自锁（实测卡死 30 秒无响应）。
    eprintln!("[lemma-host] settings loaded");
    let started = std::time::Instant::now();
    // 上限 20 秒：宿主卡住会让浏览器一直等，必须自己兜住
    eprintln!("[lemma-host] translate start");
    let outcome = runtime.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            crate::core::translate::translate(&text, &config),
        )
        .await
    });
    eprintln!(
        "[lemma-host] translate done {:.0}ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    let outcome = match outcome {
        Ok(result) => result,
        Err(_) => return Response::err("翻译超时（20 秒）"),
    };
    match outcome {
        Ok(result) => {
            let mut response = Response::ok();
            response.text = Some(result.text);
            response.provider = Some(result.provider);
            response.cached = Some(result.cached);
            response
        }
        Err(error) => Response::err(error),
    }
}

/// 读一帧：4 字节小端长度 + 正文。
fn read_frame(input: &mut impl Read) -> Option<Vec<u8>> {
    let mut header = [0u8; 4];
    if input.read_exact(&mut header).is_err() {
        return None;
    }
    let len = u32::from_le_bytes(header) as usize;
    // 上限 1 MB：协议本身不会传这么大的东西，防坏帧把内存吃掉
    if len == 0 || len > 1024 * 1024 {
        return None;
    }
    let mut body = vec![0u8; len];
    input.read_exact(&mut body).ok()?;
    Some(body)
}

fn write_frame(output: &std::sync::Mutex<std::io::Stdout>, payload: &[u8]) -> std::io::Result<()> {
    // 长度头 + 正文必须一次写完，否则两个线程交错会把帧写坏
    let mut stdout = output
        .lock()
        .map_err(|_| std::io::Error::other("stdout 锁被污染"))?;
    stdout.write_all(&(payload.len() as u32).to_le_bytes())?;
    stdout.write_all(payload)?;
    stdout.flush()
}
