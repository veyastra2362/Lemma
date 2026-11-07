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

    // 浏览器拉起 Native Messaging 宿主时，**会把扩展标识作为第一个参数**传进来：
    //   Chrome: lemma.exe "chrome-extension://<id>/" --parent-window=0
    //   Firefox: lemma.exe <extension-id>
    // 所以不能只认 `--native-host`，否则会被当成未知参数去启动 GUI
    // （表现为"宿主没有响应"，且每次重连都弹一个被单实例踢掉的进程）。
    if is_native_messaging_invocation(command) {
        crate::native_host::run();
        return true;
    }

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
        "--clipboard-roundtrip" => {
            crate::platform::console::attach_parent();
            clipboard_roundtrip();
            true
        }
        "--install-extension" => {
            crate::platform::console::attach_parent();
            install_extension_cli();
            true
        }
        "--package-extension" => {
            crate::platform::console::attach_parent();
            package_extension_cli(&args);
            true
        }
        "--speak-wav" => {
            crate::platform::console::attach_parent();
            speak_wav_cli(&args);
            true
        }
        "--phonemes" => {
            crate::platform::console::attach_parent();
            print_phonemes(&args);
            true
        }
        "--native-host" => {
            // Chrome/Firefox 通过 stdio 与本进程通信，不挂控制台
            crate::native_host::run();
            true
        }
        "--write-probe" => {
            crate::platform::console::attach_parent();
            write_probe();
            true
        }
        "--import-zh-defs" => {
            crate::platform::console::attach_parent();
            import_zh_defs(&args);
            true
        }
        "--import-cedict" => {
            crate::platform::console::attach_parent();
            import_cedict(&args);
            true
        }
        "--tts-worker" => {
            tts_worker();
            true
        }
        "--tts" => {
            crate::platform::console::attach_parent();
            tts(&args);
            true
        }
        "--capture" => {
            crate::platform::console::attach_parent();
            capture(&args);
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
        other => {
            // 未知的 `--xxx` 不能默默启动 GUI：那会表现为"命令行没输出、0 秒退出"
            // （单实例插件把第二个实例踢掉），排查起来极其费劲。踩过两次。
            if other.starts_with('-') {
                crate::platform::console::attach_parent();
                eprintln!("未知参数：{other}\n");
                print_help();
                true
            } else {
                false
            }
        }
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
/// 朗读子进程：把模型关在自己的进程里，主进程只负责播放。
///
/// 协议：每行一个请求 `{"phonemes":"ㄕㄣ1…","speed":1.0}`，回一行
/// `{"ok":true,"path":"…wav","durationSecs":3.8}` 或 `{"ok":false,"error":"…"}`。
/// stdin 关闭（EOF）就退出；主进程还会在空闲超时后杀掉本进程。
fn tts_worker() {
    use std::io::{BufRead, Write};

    // stdout 是本进程的协议通道，日志必须先改道到 stderr
    crate::core::logs::silence_stdout();

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let dir = std::env::temp_dir().join("lemma-tts");
    let _ = std::fs::create_dir_all(&dir);
    // 每次请求写不同文件：流式朗读时"下一句的合成"会与"当前句的播放"重叠，
    // 复用同一个路径会把正在播的文件覆盖掉。
    let mut sequence: u64 = 0;

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<serde_json::Value>(line) {
            Ok(request) => {
                let phonemes = request
                    .get("phonemes")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let speed = request
                    .get("speed")
                    .and_then(|value| value.as_f64())
                    .unwrap_or(1.0) as f32;
                match crate::core::tts::speak_phonemes(phonemes, speed) {
                    Ok(speech) => {
                        sequence += 1;
                        let path = dir.join(format!("worker-{sequence}.wav"));
                        match crate::core::tts::save_wav(&speech, &path) {
                            Ok(()) => serde_json::json!({
                                "ok": true,
                                "path": path.to_string_lossy(),
                                "durationSecs": speech.duration_secs(),
                            }),
                            Err(error) => serde_json::json!({ "ok": false, "error": error }),
                        }
                    }
                    Err(error) => serde_json::json!({ "ok": false, "error": error }),
                }
            }
            Err(error) => {
                serde_json::json!({ "ok": false, "error": format!("请求解析失败：{error}") })
            }
        };

        if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
            break;
        }
    }
}

/// 合成中文语音并写 WAV，用于验证 TTS 链路与测时。
fn tts(args: &[String]) {
    let Some(text) = args.get(1) else {
        eprintln!("用法：lemma.exe --tts <text> <out.wav>");
        return;
    };
    let out = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "tts-out.wav".to_string());
    if !crate::core::tts::models_ready() {
        eprintln!("TTS 模型不齐：需要 kokoro-zh-q8.onnx / tokenizer.json / voices/*.bin");
        return;
    }

    let speed: f32 = std::env::var("LEMMA_TTS_SPEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1.0);

    // 复跑用于区分冷热态
    let repeat: usize = std::env::var("LEMMA_TTS_REPEAT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);

    if std::env::var("LEMMA_TTS_DEBUG").is_ok() {
        match crate::core::tts::phonemes_for_text(text) {
            Ok(phonemes) => println!("音素：{phonemes}"),
            Err(error) => println!("音素化失败：{error}"),
        }
    }

    // 流式对照：分句逐句合成，量"首句延迟"与总耗时
    if std::env::var("LEMMA_TTS_STREAM").is_ok() {
        let sentences: Vec<String> = text
            .split_inclusive(['。', '！', '？', '；', '.', '!', '?', ';'])
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect();
        println!("分句：{} 句", sentences.len());
        let overall = Instant::now();
        let mut first_ms = 0.0;
        for (index, sentence) in sentences.iter().enumerate() {
            let t = Instant::now();
            match crate::core::tts::speak_text(sentence, speed) {
                Ok(speech) => {
                    let ms = t.elapsed().as_secs_f64() * 1000.0;
                    if index == 0 {
                        first_ms = ms;
                    }
                    println!(
                        "  第 {} 句 {:.0} ms → {:.2}s 音频  {}",
                        index + 1,
                        ms,
                        speech.duration_secs(),
                        sentence.chars().take(18).collect::<String>()
                    );
                }
                Err(error) => println!("  第 {} 句失败：{error}", index + 1),
            }
        }
        println!(
            "首句延迟 {first_ms:.0} ms（整段一次合成需要 {:.0} ms）",
            overall.elapsed().as_secs_f64() * 1000.0
        );
        return;
    }

    let started = Instant::now();
    let mut last = None;
    for _ in 0..repeat {
        match crate::core::tts::speak_text(text, speed) {
            Ok(speech) => last = Some(speech),
            Err(error) => {
                eprintln!("合成失败：{error}");
                return;
            }
        }
    }
    let elapsed_total = started.elapsed().as_secs_f64() / repeat as f64;
    let Some(speech) = last else {
        eprintln!("没有合成结果");
        return;
    };
    let duration = speech.duration_secs();
    match crate::core::tts::save_wav(&speech, std::path::Path::new(&out)) {
        Ok(()) => {
            let peak = speech
                .samples
                .iter()
                .fold(0.0f32, |acc, sample| acc.max(sample.abs()));
            println!(
                "合成 {:.2}s 音频，平均用时 {:.0} ms（RTF {:.2}x），峰值 {:.3} → {out}",
                duration,
                elapsed_total * 1000.0,
                elapsed_total / duration.max(0.001) as f64,
                peak
            );
        }
        Err(error) => eprintln!("{error}"),
    }
}

/// 截取屏幕矩形并保存，用于无头验证截图链路。
fn capture(args: &[String]) {
    let (Some(rect), Some(out)) = (args.get(1), args.get(2)) else {
        eprintln!("用法：lemma.exe --capture <x,y,w,h> <out.png>");
        return;
    };
    let parts: Vec<i32> = rect
        .split(',')
        .filter_map(|value| value.trim().parse().ok())
        .collect();
    if parts.len() != 4 {
        eprintln!("矩形格式应为 x,y,w,h");
        return;
    }

    let started = Instant::now();
    match crate::platform::capture_region(parts[0], parts[1], parts[2] as u32, parts[3] as u32) {
        Ok(shot) => {
            match image::save_buffer(
                out,
                &shot.pixels,
                shot.width,
                shot.height,
                image::ColorType::Rgb8,
            ) {
                Ok(()) => println!(
                    "已保存 {}×{} → {out}（{:.0} ms）",
                    shot.width,
                    shot.height,
                    started.elapsed().as_secs_f64() * 1000.0
                ),
                Err(error) => eprintln!("写文件失败：{error}"),
            }
        }
        Err(error) => eprintln!("截图失败：{error}"),
    }
}

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

    // 与 GUI 共用同一份落盘缓存：CLI 也要 attach，否则 --translate 永远不写缓存
    if let Some(dir) = paths::app_data_dir() {
        let cache_path = dir.join("translate-cache.jsonl");
        crate::core::translate_cache::attach(cache_path.clone());
        crate::core::translate::restore_cache(&cache_path);
    }

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
    // 先记下权威词条数：侧文件的 #count 必须与它一致，加载时才判得"新鲜"
    let dict_count = store.zh_len();
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
                crate::core::segment::write_side_file(&words_path, &words, dict_count);
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

    println!(
        "词库：{} 英文 / {} 中文 / {} 汉语释义（{}）",
        store.len(),
        store.zh_len(),
        store.zh_def_len(),
        path.display()
    );

    let started = Instant::now();
    let mut hit = None;
    for _ in 0..repeat {
        // `--no-zh-defs` 用来对照"关闭中文释义"时的结果（等于只走 CEDICT 的英文）
        let zh_defs = !args.iter().any(|arg| arg == "--no-zh-defs");
        hit = store.lookup_any_with(word, zh_defs);
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
/// 导入汉语词典（chinese-xinhua 的 `word.json` 汉字 + `ci.json` 词语）。
///
/// 数据不进仓库：用户自己下载后指定目录，导入进 `dict.redb` 的 `zhDefs` 表。
/// 只保留有解释的条目；词语用 `ci` 字段做键，汉字用 `word`。
fn import_zh_defs(args: &[String]) {
    let dir = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/zh-dict"));
    let Some(out) = paths::dict_path() else {
        eprintln!("拿不到 %APPDATA%，无法确定词库位置");
        return;
    };
    let word_path = dir.join("word.json");
    let ci_path = dir.join("ci.json");
    if !word_path.exists() && !ci_path.exists() {
        eprintln!(
            "在 {} 下既没找到 word.json（汉字）也没找到 ci.json（词语）",
            dir.display()
        );
        return;
    }

    let store = match DictStore::open_for_bulk(&out) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("打开词库失败：{error}（应用正在运行时词库被独占，请先退出托盘里的 Lemma）");
            return;
        }
    };

    #[derive(serde::Deserialize)]
    struct RawWord {
        word: String,
        #[serde(default)]
        pinyin: String,
        #[serde(default)]
        explanation: String,
    }
    #[derive(serde::Deserialize)]
    struct RawCi {
        ci: String,
        #[serde(default)]
        explanation: String,
    }

    let started = Instant::now();
    // 汉字解释很长（含例句、多音字分列），只留前 600 字；词语解释本来就短，全留
    // 600 字时词库要多占 30 MB 左右；实测释义的主要义项都在前 300 字内。
    const WORD_KEEP: usize = 300;

    let mut items: Vec<(String, crate::core::dict::ZhDef)> = Vec::new();
    for path in [&ci_path, &word_path] {
        if !path.exists() {
            continue;
        }
        println!(
            "读取 {}（已用 {:.1}s）",
            path.display(),
            started.elapsed().as_secs_f64()
        );
        if path == &ci_path {
            // 注意：`from_reader` 吃未缓冲的 File 会逐**字节**读（每个字节一次系统调用），
            // 25 MB 就要 130 秒。整文件读成字符串再 from_str 快两个数量级。
            match std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|text| {
                    serde_json::from_str::<Vec<RawCi>>(&text).map_err(|e| e.to_string())
                }) {
                Ok(list) => {
                    for item in list {
                        let text = normalize_explanation(&item.explanation);
                        if !text.is_empty() {
                            items.push((
                                item.ci,
                                crate::core::dict::ZhDef {
                                    pinyin: String::new(),
                                    definition: text,
                                },
                            ));
                        }
                    }
                }
                Err(error) => eprintln!("解析 ci.json 失败：{error}"),
            }
        } else {
            // 同上：整文件读入再解析，避免 from_reader 逐字节读
            match std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|text| {
                    serde_json::from_str::<Vec<RawWord>>(&text).map_err(|e| e.to_string())
                }) {
                Ok(list) => {
                    for item in list {
                        let text = normalize_explanation(&item.explanation);
                        if text.is_empty() {
                            continue;
                        }
                        let trimmed: String = text.chars().take(WORD_KEEP).collect();
                        items.push((
                            item.word,
                            crate::core::dict::ZhDef {
                                pinyin: item.pinyin,
                                definition: trimmed,
                            },
                        ));
                    }
                }
                Err(error) => eprintln!("解析 word.json 失败：{error}"),
            }
        }
    }

    println!(
        "解析完成：{} 条，已用 {:.1}s",
        items.len(),
        started.elapsed().as_secs_f64()
    );
    // 一批写完：分批提交会让每一批都把相关页面重写一遍（实测 14 批 = 267 秒）。
    // 28 万条一次性写入约 50 MB 内存，可以接受。
    let total = items.len();
    match store.insert_zh_defs_batch(items.into_iter(), total.max(1)) {
        Ok(written) => {
            let compact_started = Instant::now();
            if let Err(error) = store.compact() {
                eprintln!("{error}");
            }
            println!(
                "导入完成：{written} 条汉语词条，用时 {:.1}s（压缩 {:.1}s）",
                started.elapsed().as_secs_f64(),
                compact_started.elapsed().as_secs_f64()
            );
            if let Ok(meta) = std::fs::metadata(&out) {
                println!("词库大小：{:.1} MB", meta.len() as f64 / (1024.0 * 1024.0));
            }
        }
        Err(error) => eprintln!("写入失败：{error}"),
    }
}

/// 把原始解释里的多余空白压掉：`\r`、连续空行都收成一个换行。
fn normalize_explanation(raw: &str) -> String {
    // 快速路径：绝大多数条目只是行尾空白，逐字符重建太浪费（28 万条会白花几十秒）
    let trimmed = raw.trim();
    if !trimmed.contains('\r') && !trimmed.contains("\n\n") {
        return trimmed.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut last_newline = true;
    for ch in raw.replace('\r', "").chars() {
        if ch == '\n' {
            if !last_newline {
                out.push('\n');
                last_newline = true;
            }
            continue;
        }
        if ch == ' ' && last_newline {
            continue;
        }
        out.push(ch);
        last_newline = false;
    }
    out.trim().to_string()
}
/// 回填可行性探针：报告**当前焦点控件**能不能被程序写入。
fn write_probe() {
    let report = crate::platform::uia_write_probe();
    print_target("焦点控件", report.focused.as_ref());
    print_target("光标处控件", report.at_point.as_ref());
    println!("耗时     : {:.1} ms", report.elapsed_ms);
}

fn print_target(label: &str, target: Option<&crate::platform::uia::WriteTarget>) {
    println!("--- {label} ---");
    let Some(target) = target else {
        println!("  拿不到");
        return;
    };
    let element = &target.element;
    println!("  应用   : {}", element.process_name);
    println!("  控件   : {} / {}", element.name, element.class_name);
    println!("  类型   : {}", element.control_type);
    println!("  Pattern: {}", target.patterns.join(", "));
    println!(
        "  Value  : {}",
        match target.value_readonly {
            Some(true) => "只读（不可写）",
            Some(false) => "可写",
            None => "无 ValuePattern",
        }
    );
    println!("  选区   : {} 字符", target.selection_chars);
    if !target.value_preview.is_empty() {
        println!("  Value 内容: {:?}", target.value_preview);
    }
    println!("  结论   : {}", target.writable);
}
/// 剪贴板快照 / 还原自检：存快照 → 覆写 → 还原 → 比较文本是否回来。
fn clipboard_roundtrip() {
    let before = crate::platform::clipboard_snapshot();
    let Some(before) = before else {
        println!("快照失败（剪贴板被占用？）");
        return;
    };
    let original_text = crate::platform::clipboard_is_empty();
    println!(
        "快照：{} 个格式，跳过 {} 个（{:?}），原本{}",
        before.items.len(),
        before.skipped.len(),
        before.skipped,
        if original_text { "为空" } else { "有内容" }
    );

    // 覆写成一个标记值
    let _ = crate::platform::write_clipboard_text_probe("LEMMA-ROUNDTRIP-MARKER");
    let during = crate::platform::read_clipboard_text_probe().unwrap_or_default();
    println!("覆写后读到：{during:?}");

    let restored = crate::platform::clipboard_restore(&before);
    let after = crate::platform::read_clipboard_text_probe().unwrap_or_default();
    println!("还原：{restored}，还原后文本：{after:?}");
    println!(
        "结论：{}",
        if before.items.is_empty() || restored {
            "还原流程走通"
        } else {
            "还原失败（检查格式支持）"
        }
    );
}
/// 安装浏览器扩展的宿主清单与注册表（命令行验证用）。
fn install_extension_cli() {
    let report = crate::core::extension::install();
    println!("扩展目录   : {}", report.extension_dir);
    println!("Chrome 清单: {}", report.host_manifest);
    println!("Firefox 清单: {}", report.firefox_manifest);
    println!("注册表     : {}", report.registry);
    if let Some(error) = report.error {
        println!("失败：{error}");
    }
}
/// 把扩展打包成 zip（发 Release 用）。
fn package_extension_cli(args: &[String]) {
    let dir = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dist"));
    match crate::core::extension::package_both(&dir) {
        Ok((chrome, firefox)) => println!(
            "已生成 {}/lemma-extension-chrome.zip（{:.1} KB）与 lemma-extension-firefox.xpi（{:.1} KB）",
            dir.display(),
            chrome as f64 / 1024.0,
            firefox as f64 / 1024.0
        ),
        Err(error) => println!("失败：{error}"),
    }
}
/// 浏览器是不是在把我们当 Native Messaging 宿主启动。
///
/// Chrome 传 `chrome-extension://<id>/`，Edge 同；Firefox 传扩展 ID 本身。
fn is_native_messaging_invocation(first: &str) -> bool {
    let first = first.trim();
    first.starts_with("chrome-extension://")
        || first.starts_with("moz-extension://")
        || first == "lemma-writeback@lunar.dev"
}
/// 打印一段文本的音素串（诊断朗读用）。
fn print_phonemes(args: &[String]) {
    let Some(text) = args.get(1) else {
        eprintln!("用法：lemma.exe --phonemes \"<文本>\"");
        return;
    };
    match crate::core::tts::phonemes_for_text(text) {
        Ok(phonemes) => {
            println!("原文：{text}");
            println!("音素：{phonemes}");
            let missing = crate::core::tts::unmapped_chars(&phonemes);
            if missing.is_empty() {
                println!("词表：全部可编码");
            } else {
                println!("词表：读不出来的字符 → {:?}", missing);
            }
        }
        Err(error) => println!("失败：{error}"),
    }
}
/// 合成一句话并存成 wav（诊断朗读用）。第二个参数是输出路径。
fn speak_wav_cli(args: &[String]) {
    let Some(text) = args.get(1) else {
        eprintln!("用法：lemma.exe --speak-wav \"<文本>\" <输出.wav>");
        return;
    };
    let out = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("speak.wav"));
    let started = Instant::now();
    match crate::core::tts::speak_text(text, 1.0) {
        Ok(speech) => {
            let seconds = speech.duration_secs();
            if let Err(error) = std::fs::write(&out, speech.to_wav()) {
                println!("写文件失败：{error}");
                return;
            }
            println!(
                "合成 {:.2}s 音频，用时 {}ms → {}",
                seconds,
                started.elapsed().as_millis(),
                out.display()
            );
        }
        Err(error) => println!("失败：{error}"),
    }
}
