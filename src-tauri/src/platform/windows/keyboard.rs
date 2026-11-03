//! Windows 键盘观察：`WH_KEYBOARD_LL` 低级钩子。
//!
//! 为什么必须用钩子：`RegisterHotKey` 注册的是「修饰键 + 主键」的组合，
//! 而「双击 Ctrl」是时序，注册热键无法表达。
//!
//! 安全边界（很重要，改这个文件前先读）：
//! - 回调里只做一件事：把信号 `send` 进 channel，然后立刻 `CallNextHookEx`
//!   交还给系统。不做 I/O、不加锁、不阻塞——低级钩子超时会被系统直接卸载。
//! - 不吞键：无论识别到什么，都调用 `CallNextHookEx` 让按键继续传播。
//! - 不注入、不改键、不缓存按键内容。
//!
//! 低级钩子的回调在**安装它的线程**的消息泵里执行，所以必须有一个专属线程
//! 跑 `GetMessageW` 循环，线程退出钩子即失效。

use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
    SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

use crate::platform::keyboard::KeySignal;

const VK_CONTROL: u32 = 0x11;
const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;

static KEY_TX: OnceLock<Sender<KeySignal>> = OnceLock::new();

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: 系统保证 code == HC_ACTION 时 lparam 指向有效的 KBDLLHOOKSTRUCT。
    if code == HC_ACTION as i32 {
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let message = wparam.0 as u32;
        let is_down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
        let is_up = matches!(message, WM_KEYUP | WM_SYSKEYUP);

        let signal = if matches!(info.vkCode, VK_CONTROL | VK_LCONTROL | VK_RCONTROL) {
            if is_down {
                Some(KeySignal::ControlDown)
            } else if is_up {
                Some(KeySignal::ControlUp)
            } else {
                None
            }
        } else if is_down {
            Some(KeySignal::Other)
        } else {
            None
        };

        if let (Some(signal), Some(tx)) = (signal, KEY_TX.get()) {
            let _ = tx.send(signal);
        }
    }

    // SAFETY: 原样转发，保证按键继续传播（不吞键）。
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

pub(crate) fn watch(callback: impl Fn(KeySignal) + Send + 'static) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    KEY_TX
        .set(tx)
        .map_err(|_| "键盘监听已经安装过了".to_string())?;

    // 消费线程：双击判定在 core 层做，这里只负责把信号转出去。
    std::thread::spawn(move || {
        for signal in rx {
            callback(signal);
        }
    });

    // 钩子线程：安装成功后才进消息泵，安装结果同步回主线程。
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || {
        // SAFETY: 钩子在本线程安装并在本线程消息泵里回调，进程结束自动回收。
        unsafe {
            match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) {
                Ok(_hook) => {
                    let _ = ready_tx.send(Ok(()));
                    let mut message = MSG::default();
                    while GetMessageW(&mut message, None, 0, 0).as_bool() {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("安装键盘钩子失败：{error}")));
                }
            }
        }
    });

    ready_rx
        .recv()
        .map_err(|_| "键盘监听线程启动失败".to_string())?
}
