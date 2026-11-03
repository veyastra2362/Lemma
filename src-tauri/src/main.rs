// 发布版不带控制台窗口；调试版保留，便于看 stderr 里的性能打点。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `--probe` 走独立的取词探针：不启动 Tauri，不建窗口，只量 UIA 能力。
    // 见 src/probe.rs。控制台附加走 platform 层，main 里不出现任何 cfg。
    if std::env::args().any(|arg| arg == "--probe") {
        trans_lib::platform::console::attach_parent();
        trans_lib::probe::run_cli();
        return;
    }

    trans_lib::run()
}
