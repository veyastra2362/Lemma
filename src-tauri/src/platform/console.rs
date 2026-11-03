//! 控制台附加。`windows_subsystem = "windows"` 的进程默认没有 stdout，
//! 探针模式挂到父进程控制台上，这样在 PowerShell 里能直接看到输出。

pub fn attach_parent() {
    crate::platform::attach_parent_console();
}
