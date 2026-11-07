//! 应用侧：把「回填」指令通过命名管道交给宿主进程。

use std::io::{BufRead, BufReader, Write};

/// 让宿主把 `text` 推到浏览器里替换当前选区。
///
/// 失败会带上人话原因（扩展没装 / 宿主没跑 / 页面没选区）。
pub fn replace(text: &str) -> Result<(), String> {
    let mut stream =
        connect().map_err(|error| format!("连不上宿主进程（{error}）。浏览器里装好扩展了吗？"))?;

    let payload = serde_json::json!({ "cmd": "replace", "text": text });
    writeln!(stream, "{payload}").map_err(|e| format!("写管道失败：{e}"))?;
    stream.flush().ok();

    let mut line = String::new();
    BufReader::new(&stream)
        .read_line(&mut line)
        .map_err(|e| format!("读管道失败：{e}"))?;
    let value: serde_json::Value =
        serde_json::from_str(line.trim()).map_err(|e| format!("宿主回了非法 JSON：{e}"))?;
    if value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        Ok(())
    } else {
        Err(value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("回填失败")
            .to_string())
    }
}

fn connect() -> std::io::Result<std::fs::File> {
    crate::platform::connect_writeback_pipe()
}
