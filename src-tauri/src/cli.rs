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
        "--import-cedict" => {
            crate::platform::console::attach_parent();
            import_cedict(&args);
            true
        }
        "--ocr-bench" => {
            crate::platform::console::attach_parent();
            ocr_bench(&args);
            true
        }
        "--ocr" => {
            crate::platform::console::attach_parent();
            ocr(&args);
            true
        }
        "--segment" => {
            crate::platform::console::attach_parent();
            segment(&args);
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
/// 无头验证分词：`lemma.exe --segment "当前进度：多服务商"`
/// OCR 基准：目录下每张图配一个同名 `.txt` 参考答案，算字符错误率（CER）。
///
/// 加 `LEMMA_OCR_SCALE=1.5` 可先放大再识别（小字号需要）。
fn ocr_bench(args: &[String]) {
    let Some(dir) = args.get(1) else {
        eprintln!("用法：lemma.exe --ocr-bench <dir>");
        return;
    };
    let scale: f32 = std::env::var("LEMMA_OCR_SCALE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1.0);

    let mut entries: Vec<std::path::PathBuf> = match std::fs::read_dir(dir) {
        Ok(read) => read
            .filter_map(|item| item.ok())
            .map(|item| item.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
            .collect(),
        Err(error) => {
            eprintln!("读取目录失败：{error}");
            return;
        }
    };
    entries.sort();
    if entries.is_empty() {
        eprintln!("目录里没有 png：{}", dir);
        return;
    }

    println!("基准集：{} 张，预放大 {scale}×", entries.len());
    let mut total_cer = 0.0;
    let mut total_norm = 0.0;
    let mut worst: Vec<(String, f64, String)> = Vec::new();
    let mut total_ms = 0.0;

    for path in &entries {
        let truth_path = path.with_extension("txt");
        let Ok(truth) = std::fs::read_to_string(&truth_path) else {
            eprintln!(
                "跳过（缺参考答案）：{}",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            continue;
        };
        let truth = truth.replace("\r\n", "\n").trim().to_string();

        let image = match image::open(path) {
            Ok(image) => image.to_rgb8(),
            Err(error) => {
                eprintln!("读图失败 {}：{error}", path.display());
                continue;
            }
        };
        let (width, height) = (image.width(), image.height());

        let started = Instant::now();
        let result = crate::core::ocr::recognize_scaled(image.as_raw(), width, height, scale);
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        total_ms += elapsed;

        match result {
            Ok(blocks) => {
                let text = crate::core::ocr::blocks_to_text(&blocks);
                let cer = crate::core::ocr::char_error_rate(&truth, &text);
                // 归一化后（抹平弯引号/全角标点）才是"内容"层面的错误率
                let cer_norm = crate::core::ocr::char_error_rate(
                    &crate::core::ocr::normalize_for_cer(&truth),
                    &crate::core::ocr::normalize_for_cer(&text),
                );
                total_cer += cer;
                total_norm += cer_norm;
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                println!(
                    "  {name:<28} CER={cer:.3}（内容 {cer_norm:.3}）  {elapsed:>6.0}ms  段={}",
                    blocks.len()
                );
                if cer > 0.0 {
                    worst.push((name, cer, text.replace('\n', " / ")));
                }
            }
            Err(error) => eprintln!("  {} 失败：{error}", path.display()),
        }
    }

    let count = entries.len().max(1) as f64;
    println!(
        "平均 CER={:.3}（内容层面 {:.3}）  平均耗时 {:.0} ms/张  合计 {:.0} ms",
        total_cer / count,
        total_norm / count,
        total_ms / count,
        total_ms
    );
    if !worst.is_empty() {
        worst.sort_by(|a, b| b.1.total_cmp(&a.1));
        println!("--- 最差的几张 ---");
        for (name, cer, text) in worst.iter().take(5) {
            println!("  {name} CER={cer:.3}\n    识别成：{text}");
        }
    }
}

/// 对一张图片跑端侧 OCR。不启动 GUI，用于验证 OCR 链路与测时。
fn ocr(args: &[String]) {
    let Some(path) = args.get(1) else {
        eprintln!("用法：lemma.exe --ocr <image>");
        return;
    };
    let Some(dir) = crate::core::ocr::model_dir() else {
        eprintln!("拿不到应用数据目录");
        return;
    };
    println!("模型目录：{}", dir.display());
    if !crate::core::ocr::models_ready() {
        eprintln!("模型不齐：需要 det.onnx / rec.onnx / ppocr_keys_v1.txt");
        return;
    }

    let image = match image::open(path) {
        Ok(image) => image.to_rgb8(),
        Err(error) => {
            eprintln!("读取图片失败：{error}");
            return;
        }
    };
    let (width, height) = (image.width(), image.height());
    println!("图片：{}×{}", width, height);

    // 热态复测：LEMMA_OCR_REPEAT=3 时连跑多次，第一次含会话创建
    let repeat: usize = std::env::var("LEMMA_OCR_REPEAT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);

    let started = Instant::now();
    let mut last_blocks = Vec::new();
    for round in 0..repeat {
        let round_started = Instant::now();
        match crate::core::ocr::recognize(image.as_raw(), width, height) {
            Ok(blocks) => {
                println!(
                    "第 {} 次：{:.0} ms",
                    round + 1,
                    round_started.elapsed().as_secs_f64() * 1000.0
                );
                last_blocks = blocks;
            }
            Err(error) => {
                eprintln!("OCR 失败：{error}");
                return;
            }
        }
    }
    let blocks = last_blocks;
    println!(
        "识别到 {} 段，总耗时 {:.0} ms",
        blocks.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    for block in &blocks {
        let x0 = block.quad[0].x;
        let y0 = block.quad[0].y;
        println!("  ({x0:.0},{y0:.0}) w={}  {}", block.weight, block.text);
    }
    println!("--- 拼接文本 ---");
    println!("{}", crate::core::ocr::blocks_to_text(&blocks));
}

fn segment(args: &[String]) {
    let text = args[1..].join(" ");
    if text.trim().is_empty() {
        eprintln!("用法：lemma.exe --segment <text>");
        return;
    }
    if let Err(error) = crate::core::dict::open_default() {
        eprintln!("打开词库失败：{error}");
        return;
    }

    let started = Instant::now();
    let Some(segmenter) = crate::core::segment::global() else {
        eprintln!("中文词表未导入（先跑 --import-cedict）");
        return;
    };
    let tokens = segmenter.segment(&text);
    println!(
        "词表规模：{}（最长词 {} 字）",
        segmenter.len(),
        segmenter.max_len()
    );
    println!(
        "分词结果：{}",
        tokens
            .iter()
            .map(|token| if token.known {
                format!("[{}]", token.text)
            } else {
                token.text.clone()
            })
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!("用时：{:.2} ms", started.elapsed().as_secs_f64() * 1000.0);
}

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

/// 导入 CC-CEDICT。
///
/// 行格式：`繁体 简体 [pin1 yin1] /gloss1/gloss2/`
/// 只取简体做键（阅读里几乎只会遇到简体），拼音与英文释义落进 `Entry`。
fn import_cedict(args: &[String]) {
    let path = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/cedict.txt"));
    let Some(out) = paths::dict_path() else {
        eprintln!("拿不到 %APPDATA%，无法确定词库位置");
        return;
    };
    if !path.exists() {
        eprintln!("找不到 CC-CEDICT 文件：{}", path.display());
        return;
    }

    // 注意：这里**不删除**已有词库——英文词库要保留，中文是追加一张表。
    let store = match DictStore::open(&out) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("打开词库失败：{error}（应用正在运行时词库被独占，请先退出托盘里的 Lemma）");
            return;
        }
    };

    println!("源文件 : {}", path.display());
    println!("目标   : {}", out.display());

    let started = Instant::now();
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) => {
            eprintln!("读取失败：{error}");
            return;
        }
    };

    let mut skipped = 0u64;
    let mut total = 0u64;
    let mut words: Vec<String> = Vec::new();
    let entries = content.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        match parse_cedict_line(line) {
            Some((word, entry)) => {
                words.push(word.clone());
                total += 1;
                if total.is_multiple_of(50_000) {
                    eprintln!("  已解析 {total} 条");
                }
                Some((word, entry))
            }
            None => {
                skipped += 1;
                None
            }
        }
    });

    let result = store.insert_zh_batch(entries, 20_000);
    // 必须先 drop 掉词库再写侧文件：redb 在 Database 关闭时才落盘，
    // 否则词库 mtime 会晚于侧文件，下次加载的新鲜度校验会失败、白走一遍 redb。
    drop(store);

    match result {
        Ok(written) => {
            println!(
                "导入完成：{written} 条中文词条，跳过 {skipped} 条，用时 {:.1}s",
                started.elapsed().as_secs_f64()
            );
            if let Ok(meta) = std::fs::metadata(&out) {
                println!("词库大小：{:.1} MB", meta.len() as f64 / (1024.0 * 1024.0));
            }
            // 另存一份纯词表：分词器加载时顺序读它，不遍历 redb 中文表（省约 23 MB 工作集）。
            if let Some(words_path) = paths::zh_words_path() {
                crate::core::segment::write_side_file(&words_path, &words);
                println!("词表侧文件：{}", words_path.display());
            }
        }
        Err(error) => eprintln!("导入失败：{error}"),
    }
}

/// 解析一行 CC-CEDICT。返回 `(简体词, 词条)`。
fn parse_cedict_line(line: &str) -> Option<(String, Entry)> {
    // 繁体 简体 [拼音] /释义1/释义2/
    let mut parts = line.splitn(2, '[');
    let head = parts.next()?.trim();
    let rest = parts.next()?;

    let mut head_parts = head.split_whitespace();
    let _traditional = head_parts.next()?;
    let simplified = head_parts.next()?.trim();
    if simplified.is_empty() {
        return None;
    }

    let (pinyin, glosses_raw) = rest.split_once(']')?;
    let glosses: Vec<&str> = glosses_raw
        .trim()
        .trim_matches('/')
        .split('/')
        .map(str::trim)
        .filter(|gloss| !gloss.is_empty())
        .collect();
    if glosses.is_empty() {
        return None;
    }

    let entry = Entry {
        word: simplified.to_string(),
        // CC-CEDICT 的拼音是 `ni3 hao3` 这种带声调数字的写法，原样保留
        // （转成 ā á ǎ à 需要查表，收益不大，先保留可读音形）。
        phonetic: pinyin.trim().to_string(),
        translation: glosses.join("\n"),
        ..Default::default()
    };
    Some((simplified.to_string(), entry))
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
