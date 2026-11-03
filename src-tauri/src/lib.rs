pub mod commands;
pub mod core;
pub mod platform;

use tauri::{AppHandle, Manager, Runtime};

use crate::core::{logs, perf::Perf, shell, shortcut, tray};
use crate::platform::{PlatformWindow, window::WindowOps};

/// 设置窗口关闭时只隐藏，不退出进程——应用常驻托盘。
fn wire_settings_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(shell::SETTINGS_LABEL) else {
        return;
    };
    let win = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = win.hide();
            PlatformWindow::new(win.clone()).force_hide();
        }
    });
}

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
            commands::perf_stats,
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
            if let Err(error) = shortcut::register_defaults(handle) {
                logs::line(format!("shortcut failed: {error}"));
            }
            wire_settings_window(handle);

            if let Some(window) = app.get_webview_window(shell::HUD_LABEL) {
                PlatformWindow::new(window).configure_transparency();
            }

            logs::line(format!(
                "startup tray-ready {:.1}ms",
                app.state::<Perf>().startup_ms()
            ));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Trans 启动失败");
}
