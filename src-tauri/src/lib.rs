pub mod commands;
pub mod core;
pub mod platform;
pub mod probe;

use tauri::Manager;

use crate::core::settings::SettingsStore;
use crate::core::{logs, perf::Perf, shell, tray, trigger};
use crate::platform::{PlatformWindow, window::WindowOps};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Perf::new())
        // single-instance 必须最先注册，否则后面的插件会先把事件吃掉。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            shell::present(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::hud_hide,
            commands::hud_toggle,
            commands::open_settings,
            commands::frontend_ready,
            commands::perf_stats,
            commands::last_capture,
            commands::get_settings,
            commands::update_settings,
            commands::report_first_paint,
        ])
        .setup(|app| {
            let handle = app.handle();

            if let Ok(dir) = handle.path().app_data_dir() {
                logs::init(dir.join("logs").join("trans.log"));
            }
            logs::line(format!(
                "startup setup-phase {:.1}ms",
                app.state::<Perf>().startup_ms()
            ));

            if let Err(error) = tray::install(handle) {
                logs::line(format!("tray failed: {error}"));
            }

            // 设置要在 trigger 之前就位：trigger 装配时需要读它。
            let data_dir = handle.path().app_data_dir()?;
            app.manage(SettingsStore::load(&data_dir));
            if let Err(error) = trigger::install(handle) {
                logs::line(format!("trigger failed: {error}"));
            }

            if let Some(window) = app.get_webview_window(shell::HUD_LABEL) {
                PlatformWindow::new(window).configure_transparency();
            }
            shell::prewarm(handle);

            logs::line(format!(
                "startup tray-ready {:.1}ms",
                app.state::<Perf>().startup_ms()
            ));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Trans 启动失败");
}
