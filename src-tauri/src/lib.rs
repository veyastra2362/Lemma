pub mod commands;
pub mod core;
pub mod platform;
pub mod probe;

use tauri::{Emitter, Manager};

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
            commands::log_frontend,
            commands::perf_stats,
            commands::last_capture,
            commands::get_settings,
            commands::update_settings,
            commands::close_settings,
            commands::lookup_word,
            commands::translate_text,
            commands::speak_text,
            commands::synthesize_speech,
            commands::play_audio,
            commands::stop_speaking,
            commands::speech_ready,
            commands::model_status,
            commands::open_model_dir,
            commands::open_reader,
            commands::run_region_ocr,
            commands::start_region_select,
            commands::cancel_region_select,
            commands::reader_content,
            commands::close_reader,
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

            // 恢复翻译缓存：重启后重复段落不再走网络
            crate::core::translate_cache::attach(data_dir.join("translate-cache.jsonl"));
            crate::core::translate::restore_cache(&data_dir.join("translate-cache.jsonl"));
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

            // 诊断：确认编译进二进制的配置里到底有哪些窗口。
            let labels: Vec<&str> = app
                .config()
                .app
                .windows
                .iter()
                .map(|window| window.label.as_str())
                .collect();
            logs::line(format!("config windows: {labels:?}"));

            // IPC 冒烟：延迟 2.5s 后让**前端**去调 open_reader，验证 JS→Rust 那条路。
            // 用法：$env:LEMMA_READER_SMOKE_IPC="一些文本"; lemma.exe
            if let Ok(text) = std::env::var("LEMMA_READER_SMOKE_IPC") {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
                    let first = text.clone();
                    if let Some(window) = handle.get_webview_window(shell::HUD_LABEL) {
                        let _ = window.emit("reader://smoke", first);
                    }
                    // 第二次换内容，用来验证"窗口复用后内容会刷新"。
                    tokio::time::sleep(std::time::Duration::from_millis(3500)).await;
                    if let Some(window) = handle.get_webview_window(shell::HUD_LABEL) {
                        let _ = window.emit("reader://smoke", format!("{text} SECOND-OPEN"));
                    }
                });
            }

            // 朗读冒烟：LEMMA_SPEAK_SMOKE="文本" 直接跑「合成→播放」并记录内存增量。
            // 内存是这一块的真问题（模型空载约 242 MB），所以必须量出来。
            if let Ok(text) = std::env::var("LEMMA_SPEAK_SMOKE") {
                if let Some(window) = app.get_webview_window("shell") {
                    let _ = window;
                }
                let before = process_rss_mb();
                let started = std::time::Instant::now();
                // 与 speak_text 命令一致：默认走子进程，LEMMA_TTS_INPROC=1 时进程内
                let in_process = std::env::var("LEMMA_TTS_INPROC").is_ok();
                let result = if in_process {
                    crate::core::tts::speak_text(&text, 1.0).map(|speech| {
                        let dir = std::env::temp_dir().join("lemma-tts");
                        let _ = std::fs::create_dir_all(&dir);
                        let path = dir.join("smoke.wav");
                        let _ = crate::core::tts::save_wav(&speech, &path);
                        (path, speech.duration_secs())
                    })
                } else {
                    crate::core::tts::phonemes_for_text(&text).and_then(|phonemes| {
                        crate::core::tts::worker::speak(&phonemes, 1.0)
                            .map(|speech| (speech.path, speech.duration_secs))
                    })
                };
                match result {
                    Ok((path, duration)) => {
                        let played = crate::platform::play_wav(&path);
                        let peak = process_rss_mb();
                        // 进程内路径要把模型卸掉；子进程路径本来就不在主进程里\r\n                        crate::core::tts::release_model();
                        std::thread::sleep(std::time::Duration::from_millis(800));
                        let after_release = process_rss_mb();
                        logs::line(format!(
                            "speak smoke inproc={in_process} audio={duration:.2}s total={:.0}ms rss {before:.0}→{peak:.0}MB(峰值)→{after_release:.0}MB(之后) play={played:?}",
                            started.elapsed().as_secs_f64() * 1000.0,
                        ));
                    }
                    Err(error) => logs::line(format!("speak smoke failed: {error}")),
                }
            }
            // 窗口截图冒烟：LEMMA_CAPTURE_WINDOW_SMOKE=settings|reader|shell
            // 为什么要有：PowerShell 不是 DPI 感知进程，GetWindowRect 给的是虚拟化坐标，
            // 用它去截物理像素会错位（本机 125% 缩放下踩过）。进程内取矩形才是准的。
            if let Ok(label) = std::env::var("LEMMA_CAPTURE_WINDOW_SMOKE") {
                let handle = handle.clone();
                // 延迟可调：需要在截图前用真实点击制造界面状态时（`LEMMA_CAPTURE_DELAY_MS`）
                let delay_ms = std::env::var("LEMMA_CAPTURE_DELAY_MS")
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(1800);
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    let Some(window) = handle.get_webview_window(&label) else {
                        logs::line(format!("capture window: 找不到 {label}"));
                        return;
                    };
                    let Ok(position) = window.outer_position() else {
                        return;
                    };
                    let Ok(size) = window.outer_size() else {
                        return;
                    };
                    let path = std::env::temp_dir().join(format!("lemma-window-{label}.png"));
                    match crate::platform::capture_region(position.x, position.y, size.width, size.height) {
                        Ok(shot) => {
                            let _ = image::save_buffer(
                                &path,
                                &shot.pixels,
                                shot.width,
                                shot.height,
                                image::ColorType::Rgb8,
                            );
                            logs::line(format!(
                                "capture window {label}: pos={:?} size={:?} → {}",
                                (position.x, position.y),
                                (size.width, size.height),
                                path.display()
                            ));
                        }
                        Err(error) => logs::line(format!("capture window failed: {error}")),
                    }
                });
            }
            // 设置窗口冒烟：用于截图核对布局（不点托盘也能看）
            if std::env::var("LEMMA_SETTINGS_SMOKE").is_ok() {
                match shell::present_settings(handle) {
                    Ok(()) => logs::line("settings smoke presented"),
                    Err(error) => logs::line(format!("settings smoke failed: {error}")),
                }
            }
            // HUD 定位冒烟：LEMMA_HUD_SMOKE=1 时启动后直接弹 HUD，用来验证多屏定位。
            if std::env::var("LEMMA_HUD_SMOKE").is_ok() {
                shell::present(handle);
                logs::line("hud smoke presented");
            }
            // 框选层冒烟：只验证"每屏一个窗口 + 铺满"的显示路径（不点击）。
            if std::env::var("LEMMA_REGION_UI_SMOKE").is_ok() {
                match shell::present_region_select(handle) {
                    Ok(()) => {
                        let windows: Vec<String> = app
                            .webview_windows()
                            .into_iter()
                            .filter(|(label, _)| label.starts_with("ocr-region"))
                            .map(|(label, window)| {
                                let url = window
                                    .url()
                                    .map(|url| url.to_string())
                                    .unwrap_or_else(|error| format!("<{error}>"));
                                format!("{label} → {url}")
                            })
                            .collect();
                        logs::line(format!("region ui smoke ok windows={windows:?}"));
                    }
                    Err(error) => logs::line(format!("region ui smoke failed: {error}")),
                }
            }
            // 区域 OCR 冒烟：LEMMA_REGION_SMOKE="x,y,w,h" 直接跑「截图→OCR→装取词」整条链路。
            if let Ok(rect) = std::env::var("LEMMA_REGION_SMOKE") {
                let parts: Vec<i32> = rect
                    .split(',')
                    .filter_map(|value| value.trim().parse().ok())
                    .collect();
                if parts.len() == 4 {
                    match commands::run_region_capture(
                        parts[0],
                        parts[1],
                        parts[2] as u32,
                        parts[3] as u32,
                    ) {
                        Ok(capture) => {
                            if std::env::var("LEMMA_REGION_SMOKE_READER").is_ok() {
                                match shell::present_reader(handle) {
                                    Ok(()) => logs::line("region smoke reader opened"),
                                    Err(error) => {
                                        logs::line(format!("region smoke reader failed: {error}"))
                                    }
                                }
                            }
                            logs::line(format!(
                                "region smoke ok chars={} text={}",
                                capture.text.chars().count(),
                                capture.text.replace('\n', " / ")
                            ));
                        }
                        Err(error) => logs::line(format!("region smoke failed: {error}")),
                    }
                }
            }
            // 冒烟入口：带上文本启动即可直接开双栏，不依赖窗口焦点。
            // 用法：$env:LEMMA_READER_SMOKE="some text"; lemma.exe
            if let Ok(text) = std::env::var("LEMMA_READER_SMOKE") {
                crate::core::reader::set_payload(crate::core::reader::ReaderPayload::text_only(
                    text,
                    "smoke".to_string(),
                ));
                match shell::present_reader(handle) {
                    Ok(()) => {
                        logs::line("reader opened (smoke)");
                        // 让前端自己发起一次朗读，用于验证"分句合成 + 预取"流水线
                        if std::env::var("LEMMA_SPEAK_STREAM_SMOKE").is_ok() {
                            let handle = handle.clone();
                            tauri::async_runtime::spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
                                if let Some(window) = handle.get_webview_window("reader") {
                                    let _ = window.emit("reader://autospeak", ());
                                }
                            });
                        }
                    }
                    Err(error) => logs::line(format!("reader smoke failed: {error}")),
                }
            }

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
/// 当前进程工作集（MB）。冒烟时用来量模型加载的内存增量。
fn process_rss_mb() -> f64 {
    use sysinfo::{Pid, System};
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let pid = Pid::from_u32(std::process::id());
    system
        .process(pid)
        .map(|p| p.memory() as f64 / (1024.0 * 1024.0))
        .unwrap_or(0.0)
}
