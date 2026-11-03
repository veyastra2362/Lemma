//! 取词探针 CLI：`trans.exe --probe`。
//!
//! 存在的意义：**先量，再写**。UIA 在各应用上的表现差别极大，靠文档猜没用。
//! 这个命令对光标（或 `--point x,y`）所在控件做一次三档探测，输出人能读的报告，
//! 同时把 JSON 落盘，方便攒成支持矩阵。
//!
//! 用法：
//! ```text
//! trans.exe --probe                 # 探测光标处
//! trans.exe --probe --point 800,400 # 探测指定屏幕坐标
//! trans.exe --probe --delay 1500    # 等 1.5s 再测（切完窗口让目标应用稳住）
//! ```

use std::path::PathBuf;
use std::time::Duration;

use crate::platform::uia::ProbeReport;

pub fn run_cli() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(delay) = numeric_arg(&args, "--delay") {
        std::thread::sleep(Duration::from_millis(delay));
    }

    let point = point_arg(&args);
    let report = crate::platform::uia_probe(point);

    let text = render(&report);
    let json = serde_json::to_string_pretty(&report)
        .unwrap_or_else(|error| format!("{{\"error\":\"序列化失败：{error}\"}}"));

    match output_dir() {
        Some(dir) => {
            let _ = std::fs::create_dir_all(&dir);
            let json_path = dir.join("last.json");
            let text_path = dir.join("last.txt");
            let _ = std::fs::write(&json_path, &json);
            let _ = std::fs::write(&text_path, &text);

            println!("{text}");
            println!();
            println!("JSON: {}", json_path.display());
            println!("TXT : {}", text_path.display());
        }
        None => {
            println!("{text}");
            println!();
            println!("(拿不到应用数据目录，JSON 未落盘)");
            println!("{json}");
        }
    }

    // 顺带把结果写进主日志，方便和 app 的运行日志放在一起看。
    crate::core::logs::line(format!(
        "probe cursor=({},{}) element={:?} tiers={}",
        report.cursor[0],
        report.cursor[1],
        report
            .element
            .as_ref()
            .map(|element| element.process_name.as_str())
            .unwrap_or("-"),
        report
            .tiers
            .iter()
            .map(|tier| format!("{}:{}", tier.tier, if tier.hit { "hit" } else { "miss" }))
            .collect::<Vec<_>>()
            .join(",")
    ));
}

fn output_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    Some(base.join("dev.lunar.trans").join("probe"))
}

fn numeric_arg(args: &[String], flag: &str) -> Option<u64> {
    let index = args.iter().position(|arg| arg == flag)?;
    args.get(index + 1)?.parse().ok()
}

fn point_arg(args: &[String]) -> Option<(i32, i32)> {
    let index = args.iter().position(|arg| arg == "--point")?;
    let raw = args.get(index + 1)?;
    let (x, y) = raw.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

fn render(report: &ProbeReport) -> String {
    let mut lines = Vec::new();
    lines.push("=== Trans 取词探针 ===".to_string());
    lines.push(format!(
        "探测点   : ({}, {})",
        report.cursor[0], report.cursor[1]
    ));

    match &report.element {
        Some(element) => {
            lines.push(format!(
                "进程     : {} (pid {})",
                if element.process_name.is_empty() {
                    "?"
                } else {
                    element.process_name.as_str()
                },
                element.process_id
            ));
            lines.push(format!("控件类型 : {}", element.control_type));
            lines.push(format!("类名     : {}", element.class_name));
            if !element.automation_id.is_empty() {
                lines.push(format!("AutomationId: {}", element.automation_id));
            }
            if !element.name.is_empty() {
                lines.push(format!("Name     : {}", truncate(&element.name, 80)));
            }
            if let Some(bounds) = element.bounds {
                lines.push(format!(
                    "控件矩形 : ({}, {}) - ({}, {})",
                    bounds[0], bounds[1], bounds[2], bounds[3]
                ));
            }
        }
        None => lines.push("控件     : 未取到".to_string()),
    }
    lines.push(format!(
        "支持Pattern: {}",
        if report.patterns.is_empty() {
            "（无）".to_string()
        } else {
            report.patterns.join(", ")
        }
    ));

    lines.push(String::new());
    lines.push("档位            命中   耗时        内容 / 原因".to_string());
    for tier in &report.tiers {
        lines.push(format!(
            "{:<14} {:<6} {:>7.1}ms   {}",
            tier.tier,
            if tier.hit { "命中" } else { "未中" },
            tier.elapsed_ms,
            tier.detail
        ));
    }

    if let Some(context) = &report.context {
        // UIA 没有 Sentence 单位，句子由段落切出来（见 core::text）。
        let sentence = crate::core::text::sentence_containing(&context.paragraph, &context.text);
        lines.push(String::new());
        lines.push(format!("句子     : {}", truncate(&sentence, 160)));
        lines.push(format!("段落     : {}", truncate(&context.paragraph, 240)));
    }

    if let Some(error) = &report.error {
        lines.push(String::new());
        lines.push(format!("错误     : {error}"));
    }

    lines.join("\n")
}

fn truncate(text: &str, max: usize) -> String {
    let trimmed = text.trim().replace(['\r', '\n'], " ");
    let mut clipped: String = trimmed.chars().take(max).collect();
    if trimmed.chars().count() > max {
        clipped.push('…');
    }
    clipped
}
