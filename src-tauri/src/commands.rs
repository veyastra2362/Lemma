//! IPC 适配层。只做参数转换与转发，不放业务逻辑。
//!
//! 这里的命令名是前端契约，改名等于破坏前端。

use tauri::{AppHandle, Emitter, State};

use crate::core::perf::{Perf, RuntimeStats};

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

/// 当前实际生效的呼出热键（候选键可能都被别的程序占用，此时为 None）。
#[tauri::command]
pub fn active_hotkey(state: State<'_, crate::core::shortcut::ActiveHotkey>) -> Option<String> {
    state.label()
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
