//! 剪贴板契约。
//!
//! 只为一件事服务：**T2 兜底取词**——模拟 Ctrl+C 把选区复制出来。
//! 这一步有副作用，所以契约里把「保存 / 恢复」也纳进来：
//! 调用方拿到文本的同时拿到一个 [`ClipboardLease`]，drop 时自动还原。
//!
//! 平台差异全部交给 `platform::mod.rs` 里的后端分派，本文件不含任何 `cfg`。
//!
//! 已知限制（M6 再补）：目前只保存/恢复**纯文本**。剪贴板里若是图片、文件、
//! 富文本格式，这次取词会把它们弄丢——所以只在真的需要兜底时才调用。

use std::time::Duration;

/// 剪贴板租约：持有期间视为「借用了剪贴板」，drop 时把原内容放回去。
pub struct ClipboardLease {
    pub text: String,
    pub(crate) previous: Option<String>,
    /// 复制发生后的剪贴板序号，用于避免覆盖用户期间新复制的内容。
    pub(crate) after_seq: u32,
}

impl ClipboardLease {
    pub(crate) fn new(text: String, previous: Option<String>, after_seq: u32) -> Self {
        Self {
            text,
            previous,
            after_seq,
        }
    }
}

impl Drop for ClipboardLease {
    fn drop(&mut self) {
        crate::platform::restore_clipboard(self.previous.as_deref(), self.after_seq);
    }
}

/// 复制当前前台窗口的选区到剪贴板并读回。
///
/// 返回后**请尽快 drop**，否则用户的剪贴板会一直停在中间状态。
pub fn copy_selection(timeout: Duration) -> Result<ClipboardLease, String> {
    crate::platform::copy_selection(timeout)
}
