//! 原地回填：把译文写回源应用。
//!
//! 两条路，按「能否不碰剪贴板」排序：
//!
//! 1. **原生编辑控件**（`Edit` / `RichEdit` / `Scintilla`）：向控件发 `EM_REPLACESEL`，
//!    直接替换当前选区。这条路是**可撤销的**（`wParam = TRUE` 让控件把这次替换记进自己的
//!    撤销栈），而且完全不碰剪贴板，是首选。
//! 2. **其它一切**（浏览器、Electron、Firefox…）：剪贴板 + 模拟 `Ctrl+V`。
//!    实测 UIA 对浏览器文档的 `ValuePattern` 是只读的，`ITextRangeProvider` 又没有 SetText，
//!    所以这条路绕不开。前置**全格式剪贴板快照**，做完立刻还原。
//!
//! 两条路都在写入前 `SetForegroundWindow` 抢回源窗口，否则按键/消息会打到我们自己的浮层上。

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetClassNameW, GetForegroundWindow, GetWindowTextW, IsWindow,
    SW_MINIMIZE, SW_RESTORE, SendMessageW, SetForegroundWindow, ShowWindow,
};

/// `EM_REPLACESEL`
const EM_REPLACESEL: u32 = 0x00C2;

/// 走的是哪条路。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Method {
    /// `EM_REPLACESEL`，可撤销。
    NativeReplace,
    /// 剪贴板 + 模拟 Ctrl+V。
    Paste,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::NativeReplace => "native",
            Method::Paste => "paste",
        }
    }
}

pub struct Outcome {
    pub method: Method,
    /// 写入后从控件里读回来的文本（尽力而为，读不到就是空串）。
    pub verified: String,
}

/// 把 `text` 写回源窗口的当前选区。
///
/// `source` 是源**顶层窗口**句柄（用来抢焦点）；`control` 是文本控件句柄，
/// 0 表示不知道（只能走粘贴）。
pub fn replace_selection(source: isize, control: isize, text: &str) -> Result<Outcome, String> {
    // 原生替换是往**指定句柄**发消息，不需要前台；粘贴则必须先把焦点还给源窗口。
    if control != 0 && unsafe { IsWindow(Some(HWND(control as *mut _))) }.as_bool() {
        let class = window_class(control);
        if is_edit_like(&class) {
            focus(source);
            send_replace(control, text)?;
            std::thread::sleep(std::time::Duration::from_millis(60));
            return Ok(Outcome {
                method: Method::NativeReplace,
                verified: window_text(control),
            });
        }
    }

    paste_into(source, text)
}

/// 把焦点还给源窗口；拿不回来就返回 false。
///
/// Windows 有前台锁定：后台进程直接 `SetForegroundWindow` 往往失败。
/// 实测可用的合法办法是「先最小化再还原」，两次调用之后系统会认可。
fn focus(source: isize) -> bool {
    if source == 0 || !unsafe { IsWindow(Some(HWND(source as *mut _))) }.as_bool() {
        return false;
    }
    let hwnd = HWND(source as *mut _);
    if unsafe { GetForegroundWindow() } == hwnd {
        return true;
    }
    if unsafe { SetForegroundWindow(hwnd) }.as_bool() && unsafe { GetForegroundWindow() } == hwnd {
        return true;
    }

    // SAFETY: 都是标准窗口操作；SW_MINIMIZE/SW_RESTORE 不会改变窗口内容。
    unsafe {
        let _ = ShowWindow(hwnd, SW_MINIMIZE);
        std::thread::sleep(std::time::Duration::from_millis(120));
        let _ = ShowWindow(hwnd, SW_RESTORE);
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    unsafe { SetForegroundWindow(hwnd) }.as_bool() && unsafe { GetForegroundWindow() } == hwnd
}

fn is_edit_like(class: &str) -> bool {
    class.contains("Edit")
        || class.contains("RichEdit")
        || class.contains("Scintilla")
        || class.contains("TextArea")
}

fn window_class(handle: isize) -> String {
    let mut buffer = [0u16; 256];
    // SAFETY: buffer 是本栈帧上的有效可写数组。
    let len = unsafe { GetClassNameW(HWND(handle as *mut _), &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

fn window_text(handle: isize) -> String {
    let mut buffer = [0u16; 512];
    // SAFETY: buffer 是本栈帧上的有效可写数组。
    let len = unsafe { GetWindowTextW(HWND(handle as *mut _), &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

fn send_replace(control: isize, text: &str) -> Result<(), String> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: wide 在整个调用期间有效；`wParam = TRUE` 让它进入控件撤销栈。
    unsafe {
        SendMessageW(
            HWND(control as *mut _),
            EM_REPLACESEL,
            Some(WPARAM(1)),
            Some(LPARAM(wide.as_ptr() as isize)),
        );
    }
    Ok(())
}

/// 剪贴板 + 模拟 Ctrl+V。全格式快照在前，还原在后。
fn paste_into(source: isize, text: &str) -> Result<Outcome, String> {
    let snapshot = crate::platform::windows::clipboard_snapshot()
        .ok_or_else(|| "剪贴板被别的程序占用，无法安全保存原内容".to_string())?;

    if !crate::platform::windows::write_clipboard_text_probe(text) {
        return Err("写剪贴板失败".to_string());
    }

    // 这一步是硬门槛：焦点没还给源窗口就粘贴，会把译文糊到别的应用里（实测踩到过）
    if !focus(source) {
        let _ = crate::platform::windows::clipboard_restore(&snapshot);
        return Err("无法把焦点还给原窗口，已取消回填（避免粘到错误的应用）".to_string());
    }
    send_ctrl_v();
    // 给目标应用一点时间处理 WM_PASTE / 输入事件
    std::thread::sleep(std::time::Duration::from_millis(200));

    let restored = crate::platform::windows::clipboard_restore(&snapshot);
    if !restored {
        crate::core::logs::line("writeback: 剪贴板还原失败（可能是目标应用改了剪贴板）");
    }

    Ok(Outcome {
        method: Method::Paste,
        verified: String::new(),
    })
}

fn send_ctrl_v() {
    let inputs = [
        key(VK_CONTROL, true),
        key(VK_V, true),
        key(VK_V, false),
        key(VK_CONTROL, false),
    ];
    // SAFETY: 数组长度即元素个数，结构体由 `key()` 完整构造。
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}

fn key(code: VIRTUAL_KEY, down: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: code,
                wScan: 0,
                dwFlags: if down {
                    KEYBD_EVENT_FLAGS(0)
                } else {
                    KEYEVENTF_KEYUP
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
/// 取某个控件的顶层窗口（抢焦点用 `GA_ROOT = 2`）。
pub fn top_level(handle: isize) -> isize {
    if handle == 0 {
        return 0;
    }
    // SAFETY: 纯查询，句柄由调用方保证来自 UIA。
    unsafe { GetAncestor(HWND(handle as *mut _), GA_ROOT).0 as isize }
}
