//! Windows 剪贴板：保存 → 模拟 Ctrl+C → 读回 → 还原。
//!
//! 为什么不用现成的高层剪贴板库：我们需要**剪贴板序号**（`GetClipboardSequenceNumber`）
//! 来判断「Ctrl+C 到底成没成」，否则会把上一次的旧内容当成新选区。

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, CountClipboardFormats, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
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

pub(crate) fn read_text() -> Option<String> {
    read_clipboard_text()
}

/// 自检用：写文本。
pub(crate) fn write_text(text: &str) -> bool {
    write_clipboard_text(text)
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
/// 剪贴板全格式快照。
///
/// 回填要用「译文进剪贴板 → 模拟 Ctrl+V」这条路，必须先把用户原有内容原样存下来再放回去。
/// M6 的硬约束是**全格式**：文本、HTML、位图、文件列表都要能还原，否则用户丢的可能是
/// 一张刚截的图或一批文件。
pub struct ClipboardSnapshot {
    /// 快照时的剪贴板序号，用来判断期间有没有被别的程序改过。
    pub seq: u32,
    /// (格式 id, 原始字节)
    pub items: Vec<(u32, Vec<u8>)>,
    /// 存不下来的格式（位图句柄、图元文件句柄等），用于日志告警。
    pub skipped: Vec<u32>,
}

/// 可以按字节原样存取的标准格式。
///
/// GDI 句柄类格式（CF_BITMAP 2 / CF_METAFILEPICT 3 / CF_PALETTE 9 / CF_ENHMETAFILE 14 /
/// CF_DSPBITMAP 0x0082 …）拿到的是句柄而不是字节，跨 OpenClipboard 会话已失效，
/// 这里明确跳过并记进 `skipped`，不假装成功。
fn is_byte_format(format: u32) -> bool {
    !matches!(format, 2 | 3 | 9 | 14 | 0x0080..=0x008F)
}

pub(crate) fn snapshot() -> Option<ClipboardSnapshot> {
    unsafe {
        if OpenClipboard(None).is_err() {
            return None;
        }
        let count = CountClipboardFormats();
        let mut items = Vec::with_capacity(count as usize);
        let mut skipped = Vec::new();

        let mut format = 0u32;
        loop {
            format = EnumClipboardFormats(format);
            if format == 0 {
                break;
            }
            if !is_byte_format(format) {
                skipped.push(format);
                continue;
            }
            let Ok(handle) = GetClipboardData(format) else {
                skipped.push(format);
                continue;
            };
            let bytes = read_global_bytes(handle.0 as isize);
            match bytes {
                Some(bytes) if !bytes.is_empty() => items.push((format, bytes)),
                _ => skipped.push(format),
            }
        }

        let seq = GetClipboardSequenceNumber();
        let _ = CloseClipboard();
        Some(ClipboardSnapshot {
            seq,
            items,
            skipped,
        })
    }
}

/// 把快照写回剪贴板。只在剪贴板仍是我们写的那份时才还原（序号没变）。
pub(crate) fn restore(snapshot: &ClipboardSnapshot) -> bool {
    unsafe {
        if GetClipboardSequenceNumber() == snapshot.seq {
            // 期间没人动过剪贴板？那也没必要还原
            return true;
        }
        if OpenClipboard(None).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let mut restored = 0usize;
        for (format, bytes) in &snapshot.items {
            if let Some(global) = alloc_global(bytes) {
                if SetClipboardData(*format, Some(HANDLE(global.0))).is_err() {
                    let _ = GlobalFree(Some(global));
                } else {
                    restored += 1;
                }
            }
        }
        let _ = CloseClipboard();
        !snapshot.items.is_empty() && restored == snapshot.items.len()
    }
}

/// 读全局内存块的全部字节（含可能的尾部 NUL，保持原样）。
unsafe fn read_global_bytes(raw: isize) -> Option<Vec<u8>> {
    let global = HGLOBAL(raw as *mut core::ffi::c_void);
    let size = unsafe { windows::Win32::System::Memory::GlobalSize(global) };
    if size == 0 {
        return None;
    }
    let pointer = unsafe { GlobalLock(global) } as *const u8;
    if pointer.is_null() {
        return None;
    }
    let mut bytes = vec![0u8; size];
    unsafe { std::ptr::copy_nonoverlapping(pointer, bytes.as_mut_ptr(), size) };
    let _ = unsafe { GlobalUnlock(global) };
    Some(bytes)
}

/// 把字节复制进一块可移交的全局内存。
fn alloc_global(bytes: &[u8]) -> Option<HGLOBAL> {
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).ok()?;
        let pointer = GlobalLock(global) as *mut u8;
        if pointer.is_null() {
            let _ = GlobalFree(Some(global));
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len());
        let _ = GlobalUnlock(global);
        Some(global)
    }
}

/// 当前剪贴板有没有内容（用于「快照失败时干脆不写」的判断）。
pub(crate) fn is_empty() -> bool {
    unsafe {
        if OpenClipboard(None).is_err() {
            return true;
        }
        let count = CountClipboardFormats();
        let _ = CloseClipboard();
        count == 0
    }
}
