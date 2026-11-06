//! 原地回填：把译文写回源应用。
//!
//! **只走原生编辑控件**（`Edit` / `RichEdit` / `Scintilla` / `TextArea` 类名）：
//! 向控件发 `EM_REPLACESEL`，`wParam = TRUE` 让它进控件自己的撤销栈。
//!
//! 浏览器（Chrome / Edge / Firefox / Electron）**不支持**：它们把网页内容交给
//! UIA 的 `TextPattern` 只读暴露，而 `ITextRangeProvider` 根本没有 SetText。
//! 曾经用「剪贴板 + 模拟 Ctrl+V」兜底，实测两个问题无法接受：
//! 焦点抢不回来时会把译文粘到别的应用里；粘贴后还可能污染用户剪贴板。
//! 所以这条路**整体删除**，宁可提示"不支持"。

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
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

    Err("这个应用不支持原地回填（浏览器等只能靠模拟按键，按设置已停用）".to_string())
}

/// 这个控件能不能被程序直接写入。
pub fn can_replace(control: isize) -> bool {
    control != 0
        && unsafe { IsWindow(Some(HWND(control as *mut _))) }.as_bool()
        && is_edit_like(&window_class(control))
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

/// 取某个控件的顶层窗口（抢焦点用 `GA_ROOT = 2`）。
pub fn top_level(handle: isize) -> isize {
    if handle == 0 {
        return 0;
    }
    // SAFETY: 纯查询，句柄由调用方保证来自 UIA。
    unsafe { GetAncestor(HWND(handle as *mut _), GA_ROOT).0 as isize }
}
