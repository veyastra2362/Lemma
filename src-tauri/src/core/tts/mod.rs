//! 端侧朗读（M5）：Kokoro-82M 中文模型 + 自建音素化。
//!
//! 三个决定（见 `.agents/notes/`）：
//! 1. **纯 Rust 音素化**：拼音来自 CC-CEDICT（已有词库自带），拼音→注音自己实现，
//!    不引 Python 运行时、不碰 espeak-ng 的 GPL；
//! 2. **进程内按需加载**：模型空载约 242 MB，远超主进程口径，所以只在用户点朗读时加载，
//!    用完尝试释放（`ort` 释放不及时的问题见文档）；
//! 3. **音素串走 tokenizer 白名单**：Kokoro 的 zh 词表只有 172 个符号，
//!    任何不在表里的字符都会被静默丢弃——所以先按白名单过滤再查 id。

pub mod phonemize;
pub mod worker;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use ndarray::{Array1, Array2};
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;

use phonemize::Syllable;

/// Kokoro 输出采样率。
pub const SAMPLE_RATE: u32 = 24_000;
/// 音色文件的 style 行数（`voices/*.bin` 是 [510, 256] f32）。
const STYLE_ROWS: usize = 510;
const STYLE_DIM: usize = 256;

/// 合成的音频。
pub struct Speech {
    /// 单声道 f32，24 kHz，范围约 [-1, 1]。
    pub samples: Vec<f32>,
}

impl Speech {
    pub fn duration_secs(&self) -> f32 {
        self.samples.len() as f32 / SAMPLE_RATE as f32
    }

    /// 编码成 16-bit PCM 的 WAV 字节。
    pub fn to_wav(&self) -> Vec<u8> {
        let data_len = (self.samples.len() * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes()); // fmt 块长度
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // 单声道
        out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // 字节率
        out.extend_from_slice(&2u16.to_le_bytes()); // 块对齐
        out.extend_from_slice(&16u16.to_le_bytes()); // 位深
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for sample in &self.samples {
            let clamped = sample.clamp(-1.0, 1.0);
            out.extend_from_slice(&((clamped * 32767.0) as i16).to_le_bytes());
        }
        out
    }
}

/// TTS 模型目录（与 OCR 模型分开）。
pub fn model_dir() -> Option<PathBuf> {
    crate::core::paths::app_data_dir().map(|dir| dir.join("tts-models"))
}

fn model_path(name: &str) -> Result<PathBuf, String> {
    let dir = model_dir().ok_or_else(|| "拿不到应用数据目录".to_string())?;
    let path = dir.join(name);
    if !path.exists() {
        return Err(format!(
            "缺少 TTS 文件：{}（需要 kokoro-zh-q8.onnx / tokenizer.json / 一个 voices/*.bin）",
            path.display()
        ));
    }
    Ok(path)
}

/// 模型是否就绪。
pub fn models_ready() -> bool {
    model_dir().is_some_and(|dir| {
        dir.join("kokoro-zh-q8.onnx").exists()
            && dir.join("tokenizer.json").exists()
            && voice_file().is_some()
    })
}

/// 当前选用的音色文件（默认 `zf_001.bin`）。
fn voice_file() -> Option<PathBuf> {
    let dir = model_dir()?;
    let preferred = dir.join("zf_001.bin");
    if preferred.exists() {
        return Some(preferred);
    }
    std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|item| item.ok())
        .map(|item| item.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "bin"))
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static VOCAB: OnceLock<Vec<(String, i64)>> = OnceLock::new();
static STYLES: OnceLock<Vec<f32>> = OnceLock::new();

/// 会话是否已加载（用于界面提示"已占内存"）。
pub fn model_loaded() -> bool {
    SESSION.lock().map(|guard| guard.is_some()).unwrap_or(false)
}

/// 卸载模型，把内存还给系统。
///
/// 实测：加载后主进程 RSS 从 30 MB 涨到 **355 MB**——这是主进程口径的 10 倍，
/// 所以"用完就卸"是默认行为，代价是下次朗读要重新加载（约 1.2 s）。
pub fn release_model() {
    if let Ok(mut guard) = SESSION.lock()
        && guard.is_some()
    {
        *guard = None;
        crate::core::logs::line("tts model released");
    }
}

/// 拿到（必要时创建）会话并执行一次推理。
///
/// 回调只能返回**拥有所有权**的数据（不能返回 `SessionOutputs`——它借用 session），
/// 所以调用方在闭包里就把张量抽成 `Vec<f32>`。
fn with_session<T>(run: impl FnOnce(&mut Session) -> Result<T, String>) -> Result<T, String> {
    let mut guard = SESSION.lock().map_err(|_| "TTS 会话被毒化".to_string())?;
    if guard.is_none() {
        let path = model_path("kokoro-zh-q8.onnx")?;
        let created = Session::builder()
            .map_err(|e| format!("创建 TTS 会话失败：{e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| e.to_string())?
            .with_intra_threads(4)
            .map_err(|e| e.to_string())?
            .commit_from_file(&path)
            .map_err(|e| format!("加载 TTS 模型失败：{e}"))?;
        *guard = Some(created);
    }
    let session = guard
        .as_mut()
        .ok_or_else(|| "TTS 会话初始化失败".to_string())?;
    run(session)
}

/// 读 Kokoro 的符号词表（`tokenizer.json` 里的 `model.vocab`）。
///
/// 只取"单字符符号"：多字符条目在实际音素串里不会出现，留着反而干扰匹配。
fn vocab() -> Result<&'static [(String, i64)], String> {
    if let Some(existing) = VOCAB.get() {
        return Ok(existing);
    }
    let path = model_path("tokenizer.json")?;
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("读取 tokenizer.json 失败：{e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析 tokenizer.json 失败：{e}"))?;
    let map = json
        .get("model")
        .and_then(|model| model.get("vocab"))
        .and_then(|vocab| vocab.as_object())
        .ok_or_else(|| "tokenizer.json 里没有 model.vocab".to_string())?;

    let entries: Vec<(String, i64)> = map
        .iter()
        .filter_map(|(symbol, id)| {
            let id = id.as_i64()?;
            // 只保留单字符符号；空格本身由词表里的 ' ' 承担
            if symbol.chars().count() == 1 {
                Some((symbol.clone(), id))
            } else {
                None
            }
        })
        .collect();
    let _ = VOCAB.set(entries);
    VOCAB
        .get()
        .map(Vec::as_slice)
        .ok_or_else(|| "词表加载失败".to_string())
}

/// 读音色文件（[510, 256] f32）。
fn styles() -> Result<&'static [f32], String> {
    if let Some(existing) = STYLES.get() {
        return Ok(existing);
    }
    let path = voice_file().ok_or_else(|| "没有可用的音色文件（voices/*.bin）".to_string())?;
    let bytes = std::fs::read(&path).map_err(|e| format!("读取音色失败：{e}"))?;
    let expected = STYLE_ROWS * STYLE_DIM * 4;
    if bytes.len() != expected {
        return Err(format!(
            "音色文件大小异常：期望 {expected} 字节，实际 {}",
            bytes.len()
        ));
    }
    let mut floats = Vec::with_capacity(STYLE_ROWS * STYLE_DIM);
    for chunk in bytes.as_chunks::<4>().0 {
        floats.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    let _ = STYLES.set(floats);
    STYLES
        .get()
        .map(Vec::as_slice)
        .ok_or_else(|| "音色加载失败".to_string())
}

/// 把音素串按词表映射成 token id。表外字符丢弃（与官方 tokenizer 的 normalizer 行为一致）。
pub fn encode_phonemes(phonemes: &str) -> Result<Vec<i64>, String> {
    let vocab = vocab()?;
    let lookup = |ch: char| -> Option<i64> {
        vocab
            .iter()
            .find(|(symbol, _)| symbol.starts_with(ch))
            .map(|(_, id)| *id)
    };
    let ids: Vec<i64> = phonemes.chars().filter_map(lookup).collect();
    if ids.is_empty() {
        return Err("音素为空：文本里没有可朗读的拼音".to_string());
    }
    Ok(ids)
}

/// 从中文词条的拼音合成语音。
pub fn speak_pinyin(pinyin: &str, speed: f32) -> Result<Speech, String> {
    let phonemes = phonemize::phonemize_pinyin(pinyin);
    if phonemes.trim().is_empty() {
        return Err(format!("拼音无法转写：{pinyin}"));
    }
    speak_phonemes(&phonemes, speed)
}

/// 从音素串合成语音。
pub fn speak_phonemes(phonemes: &str, speed: f32) -> Result<Speech, String> {
    let ids = encode_phonemes(phonemes)?;
    let styles = styles()?;

    // Kokoro 的 style 是"按 token 数取行"：同一段文本用同一行，太短/太长就夹到边界
    let row = ids.len().min(STYLE_ROWS - 1);
    let style_slice = &styles[row * STYLE_DIM..(row + 1) * STYLE_DIM];

    let input_ids =
        Array2::<i64>::from_shape_vec((1, ids.len()), ids.clone()).map_err(|e| e.to_string())?;
    let style = Array2::<f32>::from_shape_vec((1, STYLE_DIM), style_slice.to_vec())
        .map_err(|e| e.to_string())?;
    let speed = Array1::<f32>::from_vec(vec![speed]);

    let samples = with_session(|session| {
        let outputs = session
            .run(ort::inputs![
                "input_ids" => Tensor::from_array(input_ids).map_err(|e| e.to_string())?,
                "style" => Tensor::from_array(style).map_err(|e| e.to_string())?,
                "speed" => Tensor::from_array(speed).map_err(|e| e.to_string())?,
            ])
            .map_err(|e| format!("TTS 推理失败：{e}"))?;
        let (_, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;
        Ok(data.to_vec())
    })?;

    Ok(Speech { samples })
}

/// 把一段中文朗读出来。
///
/// **先分词、按词查拼音**，查不到再退到单字。
/// 为什么：CC-CEDICT 对多音字是"一个词条一个读音"，按字取第一条会取错——
/// 实测「读」拿到的是 `dou4`（句读），而「阅读」里应该是 `du2`。
pub fn speak_text(text: &str, speed: f32) -> Result<Speech, String> {
    let phonemes = phonemes_for_text(text)?;
    speak_phonemes(phonemes.trim(), speed)
}

/// 中文 → 音素串（先按词查拼音，查不到退到单字）。
///
/// 抽成公开函数是为了让 `--tts` 的调试输出与真实合成**走同一条路**——
/// 之前调试那份是另写的按字逻辑，结果"读"在调试里显示 dou4、实际合成却是 du2，对不上。
pub fn phonemes_for_text(text: &str) -> Result<String, String> {
    let store = crate::core::dict::open_default()?;

    // 把原文按分词结果顺序展开，标点单独成段——这样韵律才由"词边界 + 标点"驱动。
    // misaki 的做法就是 `' '.join(word2ipa(w) for w in words)`，词内不加分隔。
    let mut units: Vec<String> = Vec::new();

    let tokens = match crate::core::segment::global() {
        Some(segmenter) => segmenter.segment(text),
        // 没有分词器：按字切开（最保守，不会把词黏在一起）
        None => text
            .chars()
            .map(|ch| crate::core::segment::Token {
                text: ch.to_string(),
                known: crate::core::segment::is_cjk(ch),
            })
            .collect(),
    };

    for token in tokens {
        if token.known {
            let pinyin = store
                .get_zh(&token.text)
                .ok()
                .flatten()
                .map(|entry| entry.phonetic)
                .unwrap_or_default();
            let syllables: Vec<phonemize::Syllable> = pinyin
                .split_whitespace()
                .filter_map(phonemize::syllable)
                .collect();
            if !syllables.is_empty() {
                units.push(phonemize::phonemize_word(&syllables));
                continue;
            }
            // 词表里没有这个"词"：逐字兜底
            for ch in token
                .text
                .chars()
                .filter(|ch| crate::core::segment::is_cjk(*ch))
            {
                let fallback = store
                    .get_zh(&ch.to_string())
                    .ok()
                    .flatten()
                    .map(|entry| entry.phonetic)
                    .unwrap_or_default();
                let converted = phonemize::phonemize_pinyin(&fallback);
                if !converted.is_empty() {
                    units.push(converted);
                }
            }
            continue;
        }

        // 非汉字段：把标点挑出来当独立单元（顿号、逗号、句号……）
        for ch in token.text.chars() {
            if let Some(mark) = phonemize::punctuation(ch) {
                units.push(mark.to_string());
            }
        }
    }

    if units.is_empty() {
        return Err("没有可朗读的中文".to_string());
    }

    let joined = units.join(" ");
    // 标点前不留空格（"ㄉㄨ4 ," → "ㄉㄨ4,"），与自然书写一致
    let mut cleaned = String::with_capacity(joined.len());
    for ch in joined.chars() {
        if matches!(ch, ',' | '.' | '!' | '?' | ';' | ':') && cleaned.ends_with(' ') {
            cleaned.pop();
        }
        cleaned.push(ch);
    }
    Ok(cleaned.trim().to_string())
}
/// 保存为 WAV 文件。
pub fn save_wav(speech: &Speech, path: &Path) -> Result<(), String> {
    std::fs::write(path, speech.to_wav()).map_err(|e| format!("写 WAV 失败：{e}"))
}

/// 单测用：把音节列表直接转音素串。
pub fn phonemes_for(syllables: &[Syllable]) -> String {
    phonemize::phonemize(syllables)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_well_formed() {
        let speech = Speech {
            samples: vec![0.0, 0.5, -0.5],
        };
        let wav = speech.to_wav();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 6);
        // 采样率写在 fmt 块里
        let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
        assert_eq!(rate, SAMPLE_RATE);
    }

    #[test]
    fn duration_matches_sample_count() {
        let speech = Speech {
            samples: vec![0.0; SAMPLE_RATE as usize * 2],
        };
        assert!((speech.duration_secs() - 2.0).abs() < 0.001);
    }
}
