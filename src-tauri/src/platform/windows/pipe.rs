//! 命名管道服务端：让 GUI 能把「回填」指令推给宿主。
//!
//! 方向是 **GUI → 宿主 → 扩展**。Native Messaging 只能由扩展发起，宿主主动推送要借
//! 这条管道。管道由宿主创建（它随浏览器存活），GUI 每次点按钮连一次、发一行 JSON、
//! 收一行结果，随即断开——没有常驻连接负担。

use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::FromRawHandle;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_WAIT,
};
use windows::core::PCWSTR;

use crate::platform::pipe::{PIPE_NAME, Reply};

/// 起一个线程当服务端；每来一个连接调一次 `handle`，把结果写回去。
pub(crate) fn serve(mut handle: impl FnMut(u64, String) -> Reply + Send + 'static) {
    std::thread::spawn(move || {
        let name: Vec<u16> = PIPE_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        let mut next_id = 1u64;

        loop {
            // SAFETY: name 是本线程内以 NUL 结尾的宽字符串。
            let pipe = unsafe {
                CreateNamedPipeW(
                    PCWSTR(name.as_ptr()),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    1,
                    8192,
                    8192,
                    0,
                    None,
                )
            };
            if pipe.is_invalid() {
                crate::core::logs::line("writeback pipe: CreateNamedPipeW 失败");
                std::thread::sleep(std::time::Duration::from_secs(3));
                continue;
            }

            // SAFETY: 等待 GUI 连上来。返回错误通常表示"已经连上了"（ERROR_PIPE_CONNECTED），
            // 这种情况也要继续处理。
            unsafe {
                let _ = ConnectNamedPipe(pipe, None);
            }

            let id = next_id;
            next_id += 1;
            {
                // SAFETY: 句柄交给 File 托管，离开作用域即关闭。
                let file = unsafe { std::fs::File::from_raw_handle(pipe.0 as _) };
                serve_one(&file, id, &mut handle);
            }

            // SAFETY: 断开本轮的管道实例，让下一次 CreateNamedPipeW 能重新等待。
            unsafe {
                let _ = DisconnectNamedPipe(pipe);
                let _ = CloseHandle(HANDLE(pipe.0));
            }
        }
    });
}

fn serve_one(file: &std::fs::File, id: u64, handle: &mut impl FnMut(u64, String) -> Reply) {
    let mut line = String::new();
    {
        // `&File` 实现了 Read，可以直接包进 BufReader
        let mut reader = BufReader::new(file);
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            return;
        }
    }

    let reply = match serde_json::from_str::<serde_json::Value>(line.trim()) {
        Err(_) => Reply::err("请求不是合法 JSON"),
        Ok(value) => {
            if value.get("cmd").and_then(|v| v.as_str()) != Some("replace") {
                Reply::err("未知指令")
            } else {
                let text = value
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                if text.trim().is_empty() {
                    Reply::err("text 为空")
                } else {
                    handle(id, text)
                }
            }
        }
    };

    let payload = serde_json::json!({ "ok": reply.ok, "error": reply.error });
    let mut out = file;
    let _ = writeln!(out, "{payload}");
}
/// GUI 侧：连上宿主创建的管道。
pub(crate) fn connect_client() -> std::io::Result<std::fs::File> {
    use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let name: Vec<u16> = PIPE_NAME.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: name 是本栈帧内以 NUL 结尾的宽字符串。
    let handle = unsafe {
        CreateFileW(
            PCWSTR(name.as_ptr()),
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }?;
    // SAFETY: 句柄交给 File 托管。
    Ok(unsafe { std::fs::File::from_raw_handle(handle.0 as _) })
}
