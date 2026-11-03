//! Windows 控制台附加。

use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

pub(crate) fn attach_parent() {
    // SAFETY: 纯附加操作，失败只返回错误码，无副作用。
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}
