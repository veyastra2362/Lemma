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

#[tauri::command]
pub fn perf_stats(perf: State<'_, Perf>) -> RuntimeStats {
    perf.stats()
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
    }
}
