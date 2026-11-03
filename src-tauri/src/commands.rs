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
    trigger: crate::core::settings::Trigger,
    hotkey: String,
) -> Result<Settings, String> {
    let previous = store.get();
    let next = Settings {
        trigger,
        hotkey: hotkey.trim().to_string(),
    };

    let saved = store.set(next)?;
    if let Err(error) = crate::core::trigger::apply(&app) {
        let _ = store.set(previous.clone());
        let _ = crate::core::trigger::apply(&app);
        return Err(error);
    }

    crate::core::logs::line(format!(
        "settings updated trigger={:?} hotkey={}",
        saved.trigger, saved.hotkey
    ));
    Ok(saved)
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
