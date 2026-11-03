// 发布版不带控制台窗口；调试版保留，便于看 stderr 里的性能打点。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `--probe` 走独立的取词探针：不启动 Tauri，不建窗口，只量 UIA 能力。
    // 见 src/probe.rs。
    if std::env::args().any(|arg| arg == "--probe") {
        attach_parent_console();
        trans_lib::probe::run_cli();
        return;
    }

    trans_lib::run()
}

/// `windows_subsystem = "windows"` 的进程默认没有 stdout。
/// 探针模式挂到父进程的控制台上，这样在 PowerShell 里能直接看到输出；
/// 挂不上也没关系，结果同时写进 JSON 文件。
#[cfg(target_os = "windows")]
fn attach_parent_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: 纯附加操作，失败返回错误码，无副作用。
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

#[cfg(not(target_os = "windows"))]
fn attach_parent_console() {}
