//! 命令行入口：探针、词库导入、查词压测。
//!
//! 这些命令不启动 Tauri、不建窗口，跑完即退。`windows_subsystem = "windows"`
//! 的进程默认没有 stdout，所以先挂到父进程控制台（见 `platform::console`）。

use std::path::PathBuf;
use std::time::Instant;

use serde::Deserialize;

use crate::core::dict::{DictStore, Entry, normalize};
use crate::core::paths;

pub fn print_help() {
    println!(
        "\
Lemma 命令行

  lemma.exe                       启动应用（托盘常驻）
  lemma.exe --probe               取词探针：探测光标处的 UIA 能力
      --point 800,400               指定屏幕坐标
      --delay 1500                  等待毫秒数（切完窗口让目标应用稳住）
  lemma.exe --import-dict [csv]   把 ECDICT CSV 导入 redb 词库，缺省 data/ecdict.csv
  lemma.exe --lookup <word>       查一个词，并报告耗时
      --repeat 1000                 重复查询次数（默认 1），用于压测
  lemma.exe --help                显示本帮助

词库位置：%APPDATA%\\dev.lunar.lemma\\dict.redb"
    );
}

/// 返回 true 表示这是一次 CLI 调用，调用方不应再启动 GUI。
pub fn try_run() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        return false;
    };

    match command {
        "--help" | "-h" => {
            crate::platform::console::attach_parent();
            print_help();
            true
        }
        "--probe" => {
            crate::platform::console::attach_parent();
            crate::probe::run_cli();
            true
        }
        "--import-dict" => {
            crate::platform::console::attach_parent();
            import_dict(&args);
            true
        }
        "--lookup" => {
            crate::platform::console::attach_parent();
            lookup(&args);
            true
        }
        "--translate" => {
            crate::platform::console::attach_parent();
            translate(&args);
            true
        }
        _ => false,
    }
}

/// ECDICT 的列。用 `String` 接住所有字段，坏数据也不至于整行失败。
#[derive(Debug, Deserialize)]
struct Record {
    word: String,
    #[serde(default)]
    phonetic: String,
    #[serde(default)]
    definition: String,
    #[serde(default)]
    translation: String,
    #[serde(default)]
    pos: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    bnc: String,
    #[serde(default)]
    frq: String,
    #[serde(default)]
    exchange: String,
}

fn import_dict(args: &[String]) {
    let csv_path = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/ecdict.csv"));
    let Some(out) = paths::dict_path() else {
        eprintln!("拿不到 %APPDATA%，无法确定词库位置");
        return;
    };

    if !csv_path.exists() {
        eprintln!("找不到 CSV：{}", csv_path.display());
        return;
    }

    println!("源文件 : {}", csv_path.display());
    println!("目标   : {}", out.display());

    // 重建前先删掉旧库，避免旧条目残留。
    let _ = std::fs::remove_file(&out);

    let store = match DictStore::open(&out) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("打开词库失败：{error}（应用正在运行时词库被独占，请先退出托盘里的 Lemma）");
            return;
        }
    };

    let started = Instant::now();

    let reader = match csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(&csv_path)
    {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("读 CSV 失败：{error}");
            return;
        }
    };

    let mut skipped = 0u64;
    let mut total = 0u64;

    let records = reader.into_deserialize::<Record>().filter_map(|record| {
        let record = match record {
            Ok(record) => record,
            Err(_) => {
                skipped += 1;
                return None;
            }
        };

        let key = normalize(&record.word);
        if key.is_empty() {
            skipped += 1;
            return None;
        }

        let entry = Entry {
            word: record.word,
            phonetic: record.phonetic,
            // ECDICT 的释义里是**字面量** `\n`，要换成真换行前端才好渲染。
            translation: record.translation.replace("\\n", "\n"),
            definition: record.definition.replace("\\n", "\n"),
            pos: record.pos,
            tag: record.tag,
            exchange: record.exchange,
            bnc: record.bnc.parse().unwrap_or(0),
            frq: record.frq.parse().unwrap_or(0),
        };

        total += 1;
        if total.is_multiple_of(100_000) {
            eprintln!(
                "  已读 {total} 条（{:.1}s）",
                started.elapsed().as_secs_f64()
            );
        }

        Some((key, entry))
    });

    match store.insert_batch(records, 50_000) {
        Ok(written) => {
            let elapsed = started.elapsed().as_secs_f64();
            println!("导入完成：{written} 条，跳过 {skipped} 条，用时 {elapsed:.1}s");
            if let Ok(meta) = std::fs::metadata(&out) {
                println!("词库大小：{:.1} MB", meta.len() as f64 / (1024.0 * 1024.0));
            }
        }
        Err(error) => eprintln!("导入失败：{error}"),
    }
}

/// 翻译一段文本。不启动 GUI，用于验证翻译链路与测时。
fn translate(args: &[String]) {
    let text = args[1..]
        .iter()
        .filter(|arg| !arg.starts_with("--"))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    if text.trim().is_empty() {
        eprintln!("用法：lemma.exe --translate <text>");
        return;
    }

    let settings = paths::app_data_dir()
        .map(|dir| crate::core::settings::SettingsStore::load(&dir).get())
        .unwrap_or_default();

    let config = crate::core::translate::TranslateConfig {
        provider: settings.translate_provider,
        target: settings.target_lang,
        base_url: settings.openai_base_url,
        api_key: settings.openai_api_key,
        model: settings.openai_model,
    };

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("创建异步运行时失败：{error}");
            return;
        }
    };

    println!("原文：{text}");
    println!("provider={:?} target={:?}", config.provider, config.target);

    match runtime.block_on(crate::core::translate::translate(&text, &config)) {
        Ok(result) => {
            println!("译文：{}", result.text);
            println!(
                "来源：{} · {:.0} ms{}",
                result.provider,
                result.elapsed_ms,
                if result.cached { "（缓存）" } else { "" }
            );
        }
        Err(error) => eprintln!("翻译失败：{error}"),
    }
}

fn lookup(args: &[String]) {
    let Some(word) = args.get(1) else {
        eprintln!("用法：lemma.exe --lookup <word> [--repeat N]");
        return;
    };
    let repeat: u32 = args
        .iter()
        .position(|arg| arg == "--repeat")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);

    let Some(path) = paths::dict_path() else {
        eprintln!("拿不到 %APPDATA%");
        return;
    };
    if !path.exists() {
        eprintln!("词库不存在：{}，先跑 --import-dict", path.display());
        return;
    }

    let store = match DictStore::open(&path) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("打开词库失败：{error}（应用正在运行时词库被独占，请先退出托盘里的 Lemma）");
            return;
        }
    };

    println!("词库：{} 条（{}）", store.len(), path.display());

    let started = Instant::now();
    let mut hit = None;
    for _ in 0..repeat {
        hit = store.get(word).ok().flatten();
    }
    let elapsed = started.elapsed();
    let per_query_us = elapsed.as_secs_f64() * 1_000_000.0 / f64::from(repeat.max(1));

    match hit {
        Some(entry) => {
            println!("命中：{}  [{}]", entry.word, entry.phonetic);
            if !entry.pos.is_empty() {
                println!("词性占比：{}", entry.pos);
            }
            if !entry.tag.is_empty() {
                println!("标签：{}", entry.tag);
            }
            println!("释义：\n{}", entry.translation);
            if !entry.definition.is_empty() {
                println!("英释：{}", entry.definition);
            }
            if !entry.exchange.is_empty() {
                println!("词形：{}", entry.exchange);
            }
        }
        None => println!("未收录：{word}"),
    }

    println!(
        "{repeat} 次查询共 {:.2} ms，平均 {:.1} µs/次",
        elapsed.as_secs_f64() * 1000.0,
        per_query_us
    );
}
