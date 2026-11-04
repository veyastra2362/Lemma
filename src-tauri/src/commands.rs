//! IPC 适配层。只做参数转换与转发，不放业务逻辑。
//!
//! 这里的命令名是前端契约，改名等于破坏前端。

use tauri::{AppHandle, Emitter, State};

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

#[tauri::command]
pub fn open_settings(app: AppHandle) -> Result<(), String> {
    crate::core::shell::present_settings(&app)
}

/// 前端某个视图挂载完成。用于区分「窗口没出来」和「前端没跑起来」，
/// 同时是 HUD 预热的收尾信号。
#[tauri::command]
pub fn frontend_ready(app: AppHandle, label: String) {
    crate::core::logs::line(format!("frontend ready {label}"));
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

    let saved = store.set(next)?;
    if let Err(error) = crate::core::trigger::apply(&app) {
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

/// 打开双栏窗口并显示这段文本（HUD 的「双栏」按钮走这里）。
#[tauri::command]
pub fn open_reader(app: AppHandle, text: String, source: String) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("没有可对照的文本".to_string());
    }
    crate::core::reader::set_payload(crate::core::reader::ReaderPayload { text, app: source });
    crate::core::shell::present_reader(&app)
}

/// 双栏窗口启动后拉内容。切块在 Rust 侧做（已有单测），前端只管渲染与译文。
#[tauri::command]
pub fn reader_content() -> Option<crate::core::reader::ReaderContent> {
    let payload = crate::core::reader::payload()?;
    Some(crate::core::reader::ReaderContent {
        blocks: crate::core::reader::split_blocks(&payload.text),
        text: payload.text,
        app: payload.app,
    })
}

#[tauri::command]
pub fn close_reader(app: AppHandle) {
    crate::core::shell::close_reader(&app);
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
    crate::core::dict::lookup_global(&word)
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
