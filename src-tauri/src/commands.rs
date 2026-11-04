//! IPC 适配层。只做参数转换与转发，不放业务逻辑。
//!
//! 这里的命令名是前端契约，改名等于破坏前端。

use tauri::{AppHandle, Emitter, Manager, State};

use crate::core::perf::{Perf, RuntimeStats};
use crate::core::settings::{Settings, SettingsStore};

/// 收起 HUD（前端失焦 / Esc 时调用）。
#[tauri::command]
pub fn hud_hide(app: AppHandle) {
    crate::core::shell::hide(&app);
}

/// 主动呼出 HUD（托盘按钮或二次启动时使用）。
#[tauri::command]
pub fn hud_toggle(app: AppHandle) {
    crate::core::shell::toggle(&app);
}

/// 同 `open_reader`：会创建窗口，必须 async。
#[tauri::command]
pub async fn open_settings(app: AppHandle) -> Result<(), String> {
    crate::core::shell::present_settings(&app)
}

/// 前端把日志写进 Rust 的日志文件。
///
/// GUI 的 console 我这边看不到，而 `catch(() => undefined)` 一吞，问题就彻底消失了。
/// 这个命令让前端也能留下可查的痕迹。
#[tauri::command]
pub fn log_frontend(level: String, message: String) {
    crate::core::logs::line(format!("[frontend:{level}] {message}"));
}

/// 前端某个视图挂载完成。用于区分「窗口没出来」和「前端没跑起来」，
/// 同时是 HUD 预热的收尾信号。
#[tauri::command]
pub fn frontend_ready(app: AppHandle, label: String) {
    // 顺手把资源来源写进日志：dev 构建会指向 localhost:1420（页面加载不出来）。
    // 这条用来一眼识别「又忘了用 pnpm tauri build」——我自己就踩过一次。
    let asset = app
        .get_webview_window("shell")
        .and_then(|window| window.url().ok())
        .map(|url| url.to_string())
        .unwrap_or_default();
    let warning = if asset.contains("localhost:1420") {
        " ← dev 构建，用 pnpm tauri build"
    } else {
        ""
    };
    crate::core::logs::line(format!("frontend ready {label} [asset: {asset}{warning}]"));
    if label.starts_with("shell") {
        crate::core::shell::finish_prewarm(&app);
    }
}

#[tauri::command]
pub fn perf_stats(perf: State<'_, Perf>) -> RuntimeStats {
    perf.stats()
}

/// 最近一次取词结果。HUD 显示后向 Rust 要这个。
#[tauri::command]
pub fn last_capture() -> Option<crate::core::selection::Capture> {
    crate::core::selection::last()
}

#[tauri::command]
pub fn get_settings(store: State<'_, SettingsStore>) -> Settings {
    store.get()
}

/// 更新触发方式 / 热键。
///
/// 新配置应用失败时（典型原因：热键被别的软件占用）**回滚到旧配置**并把错误返回给界面，
/// 不允许出现「设置存下来了但实际不生效」这种状态。
#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    store: State<'_, SettingsStore>,
    next: Settings,
) -> Result<Settings, String> {
    let previous = store.get();
    let mut next = next;
    next.hotkey = next.hotkey.trim().to_string();

    // 换了 provider 或目标语言，旧译文不该再命中。
    if next.translate_provider != previous.translate_provider
        || next.target_lang != previous.target_lang
    {
        crate::core::translate::clear_cache();
    }

    // 只有影响"谁来呼出"的字段变了才重注册热键。
    // 否则改个卡片高度也要去抢一次热键，一旦热键被别人占了，连无关设置都存不进去。
    let trigger_changed = next.trigger != previous.trigger
        || next.hotkey != previous.hotkey
        || next.capture_mode != previous.capture_mode
        || next.reader_open_mode != previous.reader_open_mode
        || next.reader_hotkey != previous.reader_hotkey
        || next.ocr_hotkey_enabled != previous.ocr_hotkey_enabled
        || next.ocr_hotkey != previous.ocr_hotkey;

    let saved = store.set(next)?;
    if trigger_changed && let Err(error) = crate::core::trigger::apply(&app) {
        let _ = store.set(previous.clone());
        let _ = crate::core::trigger::apply(&app);
        return Err(error);
    }

    // 设置窗口与 HUD 是两个独立 webview，各自有独立的前端状态。
    // 改完必须广播一次，否则 HUD 还按旧设置渲染。
    let _ = app.emit("settings://changed", saved.clone());

    crate::core::logs::line(format!(
        "settings updated trigger={:?} mode={:?} maxH={} hotkey={}",
        saved.trigger, saved.capture_mode, saved.hud_max_height, saved.hotkey
    ));
    Ok(saved)
}

#[tauri::command]
pub fn close_settings(app: AppHandle) {
    crate::core::shell::close_settings(&app);
}

/// 框选区域（物理像素）→ 截屏 → OCR → 装成一次取词 → 弹 HUD。
///
/// 必须先藏框选层再截屏，否则会把遮罩自己截进去；`sleep` 是等窗口真正消失的代价，
/// 实测 120 ms 足够（少了偶尔还能截到残影）。
#[tauri::command]
pub async fn run_region_ocr(
    app: AppHandle,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<crate::core::selection::Capture, String> {
    crate::core::shell::hide_region_select(&app);
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;

    let capture = run_region_capture(x, y, width, height)?;
    crate::core::shell::present(&app);
    Ok(capture)
}

/// 截图 + OCR + 装成取词。命令与冒烟共用这一份，避免两条实现漂移。
pub(crate) fn run_region_capture(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<crate::core::selection::Capture, String> {
    let started = std::time::Instant::now();
    let shot = crate::platform::capture_region(x, y, width, height)?;
    let capture_ms = started.elapsed().as_secs_f64() * 1000.0;

    if !crate::core::ocr::models_ready() {
        return Err("OCR 模型未安装：把 det.onnx / rec.onnx / ppocr_keys_v1.txt 放进应用数据目录的 ocr-models".to_string());
    }
    let blocks = crate::core::ocr::recognize_scaled(&shot.pixels, shot.width, shot.height, 1.0)?;
    let text = crate::core::ocr::blocks_to_text(&blocks);
    if text.trim().is_empty() {
        return Err("这块区域里没识别到文字".to_string());
    }

    // 耗时必须在 store 之前给定，否则 HUD 读到的是 0ms（踩过一次）
    let capture = crate::core::selection::store_ocr(text.clone(), "截图".to_string(), capture_ms);

    // 同时准备图文 payload：截图 + 每个文本框的坐标。
    // 这样用户在 HUD 点「双栏」时看到的是原图与译文的对照，而不是一段纯文字。
    match crate::core::reader::encode_png_base64(&shot.pixels, shot.width, shot.height) {
        Ok(png) => {
            let blocks = blocks
                .iter()
                .map(|block| crate::core::reader::ReaderBlock {
                    text: block.text.clone(),
                    quad: [
                        (block.quad[0].x, block.quad[0].y),
                        (block.quad[1].x, block.quad[1].y),
                        (block.quad[2].x, block.quad[2].y),
                        (block.quad[3].x, block.quad[3].y),
                    ],
                })
                .collect();
            crate::core::reader::set_payload(crate::core::reader::ReaderPayload {
                text,
                app: "截图".to_string(),
                blocks,
                image: Some(crate::core::reader::ReaderImage {
                    width: shot.width,
                    height: shot.height,
                    png_base64: png,
                }),
            });
        }
        Err(error) => crate::core::logs::line(format!("reader image encode failed: {error}")),
    }
    crate::core::logs::line(format!(
        "region ocr capture={capture_ms:.0}ms boxes={} chars={}",
        blocks.len(),
        capture.text.chars().count()
    ));
    Ok(capture)
}

#[tauri::command]
pub fn start_region_select(app: AppHandle) -> Result<(), String> {
    crate::core::shell::present_region_select(&app)
}

/// 取消框选（Esc 或右键）。
#[tauri::command]
pub fn cancel_region_select(app: AppHandle) {
    crate::core::shell::hide_region_select(&app);
}

/// 打开双栏窗口并显示这段文本（HUD 的「双栏」按钮走这里）。
/// 打开双栏窗口。
///
/// **必须是 async**：同步命令在 Tauri 里跑在主线程上，而这里要创建新窗口——
/// 创建窗口需要事件循环转起来，主线程正被这条命令占着，于是直接死锁
/// （表现为 IPC 永不返回、HUD 失去响应、Esc 关不掉）。
#[tauri::command]
pub async fn open_reader(app: AppHandle, text: String, source: String) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("没有可对照的文本".to_string());
    }
    // 截图 OCR 已经存了带图的 payload；用纯文本覆盖会让「图文对照」退化成纯文字。
    let already_image = crate::core::reader::payload()
        .is_some_and(|payload| payload.image.is_some() && payload.text == text);
    if !already_image {
        crate::core::reader::set_payload(crate::core::reader::ReaderPayload::text_only(
            text, source,
        ));
    }
    match crate::core::shell::present_reader(&app) {
        Ok(()) => {
            crate::core::logs::line("reader opened");
            Ok(())
        }
        Err(error) => {
            crate::core::logs::line(format!("open_reader failed: {error}"));
            Err(error)
        }
    }
}

/// 双栏窗口启动后拉内容。切块在 Rust 侧做（已有单测），前端只管渲染与译文。
#[tauri::command]
pub fn reader_content() -> Option<crate::core::reader::ReaderContent> {
    let payload = crate::core::reader::payload()?;
    // 纯文本模式才需要切块；图文模式的块带坐标，直接给前端
    let blocks = if payload.blocks.is_empty() {
        crate::core::reader::split_blocks(&payload.text)
    } else {
        Vec::new()
    };
    Some(crate::core::reader::ReaderContent {
        blocks,
        image_blocks: payload.blocks,
        image: payload.image,
        text: payload.text,
        app: payload.app,
    })
}

#[tauri::command]
pub fn close_reader(app: AppHandle) {
    crate::core::shell::close_reader(&app);
}

/// 朗读一段中文。
///
/// **必须是 async**：合成一次要几秒，跑在主线程会把整个 UI 冻住
/// （同 `open_reader` 的死锁教训：事件循环不能被长任务占住）。
///
/// 模型是懒加载的：第一次调用会把 121 MB 权重读进内存（空载约 242 MB），
/// 所以只在用户真的点朗读时才付这份成本。
#[tauri::command]
pub async fn speak_text(app: AppHandle, text: String) -> Result<SpeakResult, String> {
    let speed = app
        .state::<crate::core::settings::SettingsStore>()
        .get()
        .speech_speed;

    let started = std::time::Instant::now();

    // 默认走**子进程**：模型加载会让进程 RSS 涨到 332 MB，
    // 关进 worker 后主进程只剩一个 WAV 路径的开销。
    // `LEMMA_TTS_INPROC=1` 保留进程内路径，用于对照与排障。
    let in_process = std::env::var("LEMMA_TTS_INPROC").is_ok();
    let (path, duration) = if in_process {
        let speech = crate::core::tts::speak_text(&text, speed)?;
        let dir = std::env::temp_dir().join("lemma-tts");
        std::fs::create_dir_all(&dir).map_err(|e| format!("建临时目录失败：{e}"))?;
        let path = dir.join("speech.wav");
        crate::core::tts::save_wav(&speech, &path)?;
        crate::core::tts::release_model();
        (path, speech.duration_secs())
    } else {
        // 音素在**主进程**算：需要 redb 词库，而 worker 打不开被锁的词库
        let phonemes = crate::core::tts::phonemes_for_text(&text)?;
        let speech = crate::core::tts::worker::speak(&phonemes, speed)?;
        (speech.path, speech.duration_secs)
    };
    let synthesis_ms = started.elapsed().as_secs_f64() * 1000.0;

    if let Err(error) = crate::platform::play_wav(&path) {
        crate::core::logs::line(format!("tts play failed: {error}"));
        return Err(error);
    }

    crate::core::logs::line(format!(
        "tts speak chars={} audio={duration:.2}s synthesis={synthesis_ms:.0}ms rtf={:.2}x inproc={in_process}",
        text.chars().count(),
        synthesis_ms / 1000.0 / duration.max(0.001) as f64
    ));

    // 顺手做一次空闲回收（60 秒没朗读就把 worker 杀掉）
    crate::core::tts::worker::reap_if_idle();

    Ok(SpeakResult {
        duration_secs: duration,
        synthesis_ms,
    })
}

/// 停止朗读。
#[tauri::command]
pub fn stop_speaking() {
    crate::platform::stop_audio();
}

/// 朗读是否可用（模型是否齐备）。
#[tauri::command]
pub fn speech_ready() -> bool {
    crate::core::tts::models_ready()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakResult {
    pub duration_secs: f32,
    pub synthesis_ms: f64,
}

/// 翻译一段文本。走 `core::translate` 的分级路由 + 缓存。
///
/// 明确的 async 命令：网络往返 ~700 ms，不能阻塞 UI 线程。
#[tauri::command]
pub async fn translate_text(
    store: State<'_, SettingsStore>,
    text: String,
) -> Result<crate::core::translate::TranslateResult, String> {
    let settings = store.get();
    let config = crate::core::translate::TranslateConfig {
        provider: settings.translate_provider,
        target: settings.target_lang,
        base_url: settings.openai_base_url,
        api_key: settings.openai_api_key,
        model: settings.openai_model,
    };
    crate::core::translate::translate(&text, &config).await
}

/// 查一个词（Deck 下探用）。未收录返回 null，不是错误。
#[tauri::command]
pub fn lookup_word(word: String) -> Option<crate::core::dict::Entry> {
    crate::core::dict::lookup_any(&word)
}

/// 按内容高度调整 HUD（前端量出来之后调用）。
#[tauri::command]
pub fn resize_hud(app: AppHandle, height: f64) -> Result<(), String> {
    crate::core::shell::resize_hud(&app, height)
}

/// 前端在 `requestAnimationFrame` 里回报「首帧已绘制」。
///
/// 这是唯一可信的呼出延迟数据点：Rust 侧 `show()` 返回只代表窗口系统收到了
/// 请求，离 WebView 真正出图还差一次合成。
#[tauri::command]
pub fn report_first_paint(app: AppHandle, perf: State<'_, Perf>, seq: u64) {
    if let Some(sample) = perf.finish_present(seq) {
        crate::core::logs::line(format!(
            "perf hud-first-paint {:.1}ms seq={}",
            sample.total_ms, sample.seq
        ));
        let _ = app.emit("perf://first-paint", sample);
    } else {
        crate::core::logs::line(format!("perf stale first-paint seq={seq} ignored"));
    }
}
