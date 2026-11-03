//! 非 Windows 平台的剪贴板兜底占位。

use std::time::Duration;

use crate::platform::clipboard::ClipboardLease;

pub(crate) fn copy_selection(_timeout: Duration) -> Result<ClipboardLease, String> {
    Err("当前平台未实现剪贴板取词".to_string())
}

pub(crate) fn restore_clipboard(_previous: Option<&str>, _after_seq: u32) {}
