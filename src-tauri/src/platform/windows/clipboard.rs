//! Windows 剪贴板：保存 → 模拟 Ctrl+C → 读回 → 还原。
//!
//! 为什么不用现成的高层剪贴板库：我们需要**剪贴板序号**（`GetClipboardSequenceNumber`）
//! 来判断「Ctrl+C 到底成没成」，否则会把上一次的旧内容当成新选区。

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_C, VK_CONTROL,
};

use crate::platform::clipboard::ClipboardLease;

const CF_UNICODETEXT: u32 = 13;

pub(crate) fn copy_selection(timeout: Duration) -> Result<ClipboardLease, String> {
    let before_seq = unsafe { GetClipboardSequenceNumber() };
    let previous = read_clipboard_text();

    send_ctrl_c();

    // 等剪贴板序号变化：变化了才说明复制真的发生（而不是读到旧内容）。
    let deadline = Instant::now() + timeout;
    let mut after_seq = before_seq;
    while Instant::now() < deadline {
        after_seq = unsafe { GetClipboardSequenceNumber() };
        if after_seq != before_seq {
            break;
        }
        std::thread::sleep(Duration::from_millis(8));
    }

    if after_seq == before_seq {
        return Err("模拟 Ctrl+C 后剪贴板没有变化（可能没有选中文本，或被保护）".to_string());
    }

    match read_clipboard_text() {
        Some(text) if !text.trim().is_empty() => Ok(ClipboardLease::new(text, previous, after_seq)),
        Some(_) => Err("复制到的是空白内容".to_string()),
        None => Err("剪贴板里没有文本".to_string()),
    }
}

/// 还原剪贴板。只有当前序号仍是我们写入的那次，才覆盖——避免踩掉用户新复制的内容。
pub(crate) fn restore_clipboard(previous: Option<&str>, after_seq: u32) {
    let Some(previous) = previous else {
        return;
    };
    let current = unsafe { GetClipboardSequenceNumber() };
    if current != after_seq {
        return;
    }
    write_clipboard_text(previous);
}

fn send_ctrl_c() {
    let inputs = [
        key(VK_CONTROL, false),
        key(VK_C, false),
        key(VK_C, true),
        key(VK_CONTROL, true),
    ];
    // SAFETY: inputs 是本栈帧上的有效切片，大小与结构体定义一致。
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn read_clipboard_text() -> Option<String> {
    // SAFETY: 每次 Open 都配对 Close；句柄在解锁后即释放使用，不跨调用持有。
    unsafe {
        if OpenClipboard(None).is_err() {
            return None;
        }

        let text = (|| {
            let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
            if handle.is_invalid() {
                return None;
            }
            let global = HGLOBAL(handle.0);
            let pointer = GlobalLock(global) as *const u16;
            if pointer.is_null() {
                return None;
            }
            let mut len = 0usize;
            while *pointer.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(pointer, len);
            let text = String::from_utf16_lossy(slice);
            let _ = GlobalUnlock(global);
            Some(text)
        })();

        let _ = CloseClipboard();
        text
    }
}

fn write_clipboard_text(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();

    // SAFETY: 全局内存的所有权在 SetClipboardData 成功后移交给系统；
    // 失败路径自己 GlobalFree，不会泄漏。
    unsafe {
        let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, bytes) else {
            return false;
        };
        let pointer = GlobalLock(global) as *mut u16;
        if pointer.is_null() {
            let _ = GlobalFree(Some(global));
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), pointer, wide.len());
        let _ = GlobalUnlock(global);

        if OpenClipboard(None).is_err() {
            let _ = GlobalFree(Some(global));
            return false;
        }
        let _ = EmptyClipboard();
        let ok = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(global.0))).is_ok();
        let _ = CloseClipboard();

        if !ok {
            let _ = GlobalFree(Some(global));
        }
        ok
    }
}
