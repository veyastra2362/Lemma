pub mod commands;
pub mod core;
pub mod platform;
pub mod probe;

use tauri::Manager;

use crate::core::settings::SettingsStore;
use crate::core::{logs, perf::Perf, shell, tray, trigger};
use crate::platform::{PlatformWindow, window::WindowOps};

/// 一次性迁移：把改名前的数据目录（`dev.lunar.trans`）里的设置与词库搬过来。
///
/// 换 `identifier` 就换了数据目录。不迁移的话用户会以为设置丢了、词库没了，
/// 而 128 MB 词库重新导入也不是零成本。只搬已知的两个文件，不做整目录拷贝。
fn migrate_legacy_data() {
    let (Some(new_dir), Some(legacy_dir)) = (
        crate::core::paths::app_data_dir(),
        crate::core::paths::legacy_app_data_dir(),
    ) else {
        return;
    };
    if !legacy_dir.exists() || legacy_dir == new_dir {
        return;
    }

    let _ = std::fs::create_dir_all(&new_dir);
    for name in ["settings.json", "dict.redb"] {
        let from = legacy_dir.join(name);
        let to = new_dir.join(name);
        if !from.exists() || to.exists() {
            continue;
        }
        let moved = std::fs::rename(&from, &to).is_ok() || std::fs::copy(&from, &to).is_ok();
        logs::line(format!(
            "migrate {name}: {}",
            if moved { "ok" } else { "failed" }
        ));
    }
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
            commands::frontend_ready,
            commands::perf_stats,
            commands::last_capture,
            commands::get_settings,
            commands::update_settings,
            commands::close_settings,
            commands::lookup_word,
            commands::translate_text,
            commands::resize_hud,
            commands::report_first_paint,
        ])
        .setup(|app| {
            let handle = app.handle();

            if let Ok(dir) = handle.path().app_data_dir() {
                logs::init(dir.join("logs").join("lemma.log"));
            }
            migrate_legacy_data();

            logs::line(format!(
                "startup setup-phase {:.1}ms",
                app.state::<Perf>().startup_ms()
            ));

            if let Err(error) = tray::install(handle) {
                logs::line(format!("tray failed: {error}"));
            }

            // 词库：存在才装。没导入过也不影响取词，只是没有释义。
            if let Some(path) = crate::core::paths::dict_path()
                && path.exists()
            {
                match crate::core::dict::DictStore::open(&path) {
                    Ok(store) => {
                        let count = store.len();
                        crate::core::dict::install(std::sync::Arc::new(store));
                        logs::line(format!("dict loaded {count} entries"));
                    }
                    Err(error) => logs::line(format!("dict open failed: {error}")),
                }
            } else {
                logs::line("dict missing (run --import-dict)");
            }

            let data_dir = handle.path().app_data_dir()?;
            app.manage(SettingsStore::load(&data_dir));
            if let Err(error) = trigger::install(handle) {
                logs::line(format!("trigger failed: {error}"));
            }

            // 后台预热翻译连接：延迟 1.5s，避开启动高峰，也不阻塞托盘就绪。
            {
                let settings = app.state::<SettingsStore>().get();
                let config = crate::core::translate::TranslateConfig {
                    provider: settings.translate_provider,
                    target: settings.target_lang,
                    base_url: settings.openai_base_url,
                    api_key: settings.openai_api_key,
                    model: settings.openai_model,
                };
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                    crate::core::translate::warm_up(&config).await;
                });
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
        .expect("Lemma 启动失败");
}
pub mod cli;
