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
    let stdout = std::io::stdout();
    let mut output = stdout.lock();

    loop {
        let Some(body) = read_frame(&mut input) else {
            break;
        };
        let response = match serde_json::from_slice::<Request>(&body) {
            Ok(request) => handle(&runtime, request),
            Err(error) => Response::err(format!("请求不是合法 JSON：{error}")),
        };
        let Ok(payload) = serde_json::to_vec(&response) else {
            break;
        };
        if write_frame(&mut output, &payload).is_err() {
            break;
        }
    }
    crate::core::logs::line("native host: stdin closed, exiting");
}

fn handle(runtime: &tokio::runtime::Runtime, request: Request) -> Response {
    match request.method.as_str() {
        "ping" => {
            let mut response = Response::ok();
            response.version = Some(env!("CARGO_PKG_VERSION").to_string());
            response
        }
        "translate" => {
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

    // 翻译是异步的；宿主没有 Tauri 运行时，这里自建一个单线程 runtime
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return Response::err(format!("创建运行时失败：{error}")),
    };
    crate::core::logs::line(format!(
        "native host translate start chars={} provider={:?} target={:?}",
        text.chars().count(),
        config.provider,
        config.target
    ));
    let started = std::time::Instant::now();
    // 上限 20 秒：宿主卡住会让浏览器一直在等，必须自己兜住
    let outcome = runtime.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            crate::core::translate::translate(&text, &config),
        )
        .await
    });
    crate::core::logs::line(format!(
        "native host translate done {:.0}ms",
        started.elapsed().as_secs_f64() * 1000.0
    ));
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

fn write_frame(output: &mut impl Write, payload: &[u8]) -> std::io::Result<()> {
    output.write_all(&(payload.len() as u32).to_le_bytes())?;
    output.write_all(payload)?;
    output.flush()
}
