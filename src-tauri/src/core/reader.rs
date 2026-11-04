//! 双栏阅读窗口：数据与切块。
//!
//! 设计要点：
//! - **段落对齐靠切块**：把原文切成若干块，每块单独翻译，左原文右译文一一对应。
//!   整段一次性翻译再按标点硬切是行不通的——中英语序不同，行数对不上。
//! - 切块是纯函数，可单测；翻译走已有的 `core::translate`（同一套缓存）。
//! - 内容由 HUD 或热键塞进来，窗口本身只负责显示。

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// 一屏能读得动的块长上限（字符）。超过就按句子再切。
const MAX_BLOCK_CHARS: usize = 400;

/// 一个带坐标的文本块（图文的锚点）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderBlock {
    pub text: String,
    /// 四边形：左上、右上、右下、左下（**图片自身坐标系**）。
    pub quad: [(f32, f32); 4],
}

/// 一张随内容一起带过来的图（截图 OCR 场景）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderImage {
    pub width: u32,
    pub height: u32,
    /// PNG 的 base64（不带 data: 前缀），前端拼成 data URL 直接显示。
    pub png_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderPayload {
    /// 原始文本（可能是一段，也可能是多段）。
    pub text: String,
    /// 来源应用名，显示在标题栏。
    pub app: String,
    /// 带坐标的文本块；空表示纯文本模式（没有图）。
    #[serde(default)]
    pub blocks: Vec<ReaderBlock>,
    /// 截图；`None` 表示纯文本模式。
    #[serde(default)]
    pub image: Option<ReaderImage>,
}

impl ReaderPayload {
    pub fn text_only(text: String, app: String) -> Self {
        Self {
            text,
            app,
            blocks: Vec::new(),
            image: None,
        }
    }
}

/// 给前端的完整内容：原文 + 切好的块。
/// 给前端的完整内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderContent {
    pub text: String,
    pub app: String,
    /// 纯文本模式的段落块，左右两栏按顺序一一对应。
    pub blocks: Vec<String>,
    /// 图文模式的文本块（带坐标）；非空时前端走图文渲染。
    pub image_blocks: Vec<ReaderBlock>,
    pub image: Option<ReaderImage>,
}

/// 把 RGB 像素编码成 PNG 再 base64，供前端用 data URL 显示。
///
/// 不用原始像素：一张 700×300 的 RGB 是 630 KB，base64 后约 840 KB；
/// 同一张图编成 PNG 通常只有 30–200 KB，base64 也就几百 KB。
pub fn encode_png_base64(pixels: &[u8], width: u32, height: u32) -> Result<String, String> {
    let image = image::RgbImage::from_raw(width, height, pixels.to_vec())
        .ok_or_else(|| "图像数据长度不匹配".to_string())?;
    let mut png = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png);
    image
        .write_with_encoder(encoder)
        .map_err(|e| format!("PNG 编码失败：{e}"))?;
    Ok(base64_encode(&png))
}

/// 极简 base64（不引依赖）：标准字母表 + 补位。
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(triple >> 18) as usize & 63] as char);
        out.push(TABLE[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(triple >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[triple as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

static PAYLOAD: Mutex<Option<ReaderPayload>> = Mutex::new(None);

pub fn set_payload(payload: ReaderPayload) {
    if let Ok(mut slot) = PAYLOAD.lock() {
        *slot = Some(payload);
    }
}

pub fn payload() -> Option<ReaderPayload> {
    PAYLOAD.lock().ok().and_then(|slot| slot.clone())
}

/// 把原文切成段落块。
///
/// 优先按空行切（PDF / 网页复制出来的常见形态）；
/// 没有空行就按句末标点累积到 `MAX_BLOCK_CHARS`；
/// 再不行就整段一块。
pub fn split_blocks(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // 1) 空行分段
    let by_blank: Vec<String> = trimmed
        .split("\n\n")
        .map(|block| block.trim().to_string())
        .filter(|block| !block.is_empty())
        .collect();
    if by_blank.len() > 1 {
        return by_blank
            .into_iter()
            .flat_map(|block| split_long(&block))
            .collect();
    }

    split_long(trimmed)
}

/// 按行先切一刀（很多 PDF 复制出来是单换行），过长的再按句子切。
fn split_long(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    for line in text.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        blocks.extend(split_by_sentence(line));
    }
    if blocks.is_empty() {
        vec![text.trim().to_string()]
    } else {
        blocks
    }
}

/// 累积句子到块，直到接近上限。
fn split_by_sentence(line: &str) -> Vec<String> {
    if line.chars().count() <= MAX_BLOCK_CHARS {
        return vec![line.to_string()];
    }

    let mut blocks = Vec::new();
    let mut current = String::new();

    for sentence in sentences(line) {
        let length = sentence.chars().count();

        // 单句自己就超长（没有句读的长串，比如粘进来的表格或 URL）：
        // 只能按字数硬切，否则整段会变成一个巨大的块。
        if length > MAX_BLOCK_CHARS {
            if !current.trim().is_empty() {
                blocks.push(current.trim().to_string());
                current.clear();
            }
            let mut chunk = String::new();
            let mut count = 0usize;
            for ch in sentence.chars() {
                chunk.push(ch);
                count += 1;
                if count >= MAX_BLOCK_CHARS {
                    blocks.push(std::mem::take(&mut chunk));
                    count = 0;
                }
            }
            if !chunk.trim().is_empty() {
                blocks.push(chunk.trim().to_string());
            }
            continue;
        }

        if current.chars().count() + length > MAX_BLOCK_CHARS && !current.trim().is_empty() {
            blocks.push(current.trim().to_string());
            current.clear();
        }
        current.push_str(&sentence);
    }

    if !current.trim().is_empty() {
        blocks.push(current.trim().to_string());
    }
    blocks
}

/// 粗粒度分句：句末标点后切开，标点跟着前一句。
fn sentences(line: &str) -> Vec<String> {
    const ENDS: &[char] = &['。', '！', '？', '；', '.', '!', '?', ';'];
    let mut out = Vec::new();
    let mut current = String::new();

    for ch in line.chars() {
        current.push(ch);
        if ENDS.contains(&ch) {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_blank_lines() {
        let blocks = split_blocks("First paragraph.\n\nSecond paragraph.");
        assert_eq!(blocks, vec!["First paragraph.", "Second paragraph."]);
    }

    #[test]
    fn single_paragraph_stays_one_block() {
        let blocks = split_blocks("The threshold value matters.");
        assert_eq!(blocks, vec!["The threshold value matters."]);
    }

    #[test]
    fn single_newlines_also_split() {
        // PDF 复制出来常见单换行
        let blocks = split_blocks("Line one here.\nLine two here.");
        assert_eq!(blocks, vec!["Line one here.", "Line two here."]);
    }

    #[test]
    fn long_paragraph_splits_by_sentence() {
        let sentence = "这是一个很长的句子用来把段落撑开，".repeat(40);
        let blocks = split_blocks(&sentence);
        assert!(blocks.len() > 1, "长句应被切成多块");
        for block in &blocks {
            assert!(block.chars().count() <= MAX_BLOCK_CHARS + 40);
        }
    }

    /// 没有句读的超长串（粘进来的表格 / URL）必须硬切，不能整段一块。
    #[test]
    fn an_overlong_single_sentence_is_hard_cut() {
        let long = "啊".repeat(1000);
        let blocks = split_blocks(&long);
        assert!(blocks.len() >= 2, "应硬切成多块，实际 {}", blocks.len());
        for block in &blocks {
            assert!(block.chars().count() <= MAX_BLOCK_CHARS);
        }
    }

    #[test]
    fn keeps_chinese_sentence_endings_with_the_sentence() {
        let parts = sentences("第一句。第二句！第三句？");
        assert_eq!(parts, vec!["第一句。", "第二句！", "第三句？"]);
    }

    #[test]
    fn empty_input_yields_nothing() {
        assert!(split_blocks("").is_empty());
        assert!(split_blocks("   \n\n  ").is_empty());
    }

    #[test]
    fn payload_round_trips() {
        set_payload(ReaderPayload::text_only(
            "hello world".to_string(),
            "chrome.exe".to_string(),
        ));
        let payload = payload().expect("应能取回");
        assert_eq!(payload.text, "hello world");
        assert_eq!(payload.app, "chrome.exe");
    }
}
