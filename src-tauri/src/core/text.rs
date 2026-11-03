//! 文本上下文切分。
//!
//! 为什么自己写：UIA 的 `TextUnit` 只有 Character / Format / Word / Line /
//! Paragraph / Page / Document，**没有 Sentence**。所以「取词带句子上下文」这件事
//! 只能先拿整段再自己切。
//!
//! 纯函数，跨平台，可直接单测。

/// 句末终止符。中英混排都要照顾到，`\n` 也算（换行在 PDF / 代码里很常见）。
const TERMINATORS: &[char] = &['。', '！', '？', '；', '…', '.', '!', '?', ';', '\n'];

/// 切出段落里包含 `needle` 的那一句。
///
/// - 找不到 `needle`（比如词被 UIA 切断了、或跨句）时返回整段，让上层自己截断；
/// - 段落为空返回空串。
pub fn sentence_containing(paragraph: &str, needle: &str) -> String {
    let text = paragraph.trim();
    if text.is_empty() {
        return String::new();
    }

    let needle = needle.trim();
    if needle.is_empty() {
        // 没有锚点就返回第一句，总比整段好读。
        return sentence_ranges(text)
            .first()
            .map(|(start, end)| text[*start..*end].trim().to_string())
            .unwrap_or_else(|| text.to_string());
    }

    for (start, end) in sentence_ranges(text) {
        let slice = &text[start..end];
        if slice.contains(needle) {
            return slice.trim().to_string();
        }
    }

    text.to_string()
}

/// 按句末终止符切出字节区间。连续终止符（`……`、`?!`）合并成同一句的结尾。
fn sentence_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut chars = text.char_indices().peekable();

    while let Some((index, ch)) = chars.next() {
        if !TERMINATORS.contains(&ch) {
            continue;
        }

        let mut end = index + ch.len_utf8();
        // 连续的终止符并到同一句；换行不并入（它本身就是硬边界）。
        if ch != '\n' {
            while let Some(&(next_index, next_ch)) = chars.peek() {
                if next_ch != '\n' && TERMINATORS.contains(&next_ch) {
                    end = next_index + next_ch.len_utf8();
                    let _ = chars.next();
                } else {
                    break;
                }
            }
        }

        if end > start {
            ranges.push((start, end));
            start = end;
        }
    }

    if start < text.len() {
        ranges.push((start, text.len()));
    }

    ranges
}

/// 判断一个字符是不是"不该算进词里"的边界字符。
///
/// 为什么需要：UIA 的 `TextUnit_Word` 对中日韩文本只在空白/标点处断词，
/// 实测会把尾随标点（`当前进度：`）甚至替换符（`测智\u{FFFD}`）一起带出来。
fn is_boundary_char(ch: char) -> bool {
    if ch.is_whitespace() || ch.is_control() {
        return true;
    }

    match ch as u32 {
        // 零宽、双向控制、变体选择符、未分配替换
        0x200B..=0x200F | 0x202A..=0x202E | 0xFE00..=0xFE0F | 0xFFF0..=0xFFFD => true,
        // 通用标点（…、—、引号等）
        0x2000..=0x206F => true,
        // CJK 标点与兼容形式
        0x3000..=0x303F | 0xFE30..=0xFE6F => true,
        // 全角标点（保留全角数字 0xFF10..=0xFF19）
        0xFF01..=0xFF0F | 0xFF1A..=0xFF20 | 0xFF3B..=0xFF40 | 0xFF5B..=0xFF65 => true,
        // 箭头 / 数学 / 几何图形 / 杂项符号 / emoji
        0x2190..=0x2BFF | 0x1F000..=0x1FAFF => true,
        // ASCII 标点，但撇号与连字符是词的一部分（don't / e-mail）
        _ => ch.is_ascii_punctuation() && ch != '\'' && ch != '-',
    }
}

/// 去掉词首尾的标点、符号与不可见字符，只处理两端。
pub fn trim_word(raw: &str) -> String {
    raw.trim_matches(is_boundary_char).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_chinese_sentence_containing_the_word() {
        let paragraph = "第一句在这里。第二句包含 threshold 这个词。第三句收尾。";
        assert_eq!(
            sentence_containing(paragraph, "threshold"),
            "第二句包含 threshold 这个词。"
        );
    }

    #[test]
    fn picks_the_english_sentence_containing_the_word() {
        let paragraph = "First one. The threshold value matters! Last one?";
        assert_eq!(
            sentence_containing(paragraph, "threshold"),
            "The threshold value matters!"
        );
    }

    #[test]
    fn merges_consecutive_terminators() {
        let paragraph = "真的吗？！后面还有一句。";
        assert_eq!(sentence_containing(paragraph, "真的"), "真的吗？！");
    }

    #[test]
    fn newline_ends_a_sentence() {
        let paragraph = "第一行\n第二行有 target\n第三行";
        assert_eq!(sentence_containing(paragraph, "target"), "第二行有 target");
    }

    #[test]
    fn falls_back_to_the_whole_paragraph_when_the_word_is_missing() {
        let paragraph = "完全无关的一段话。";
        assert_eq!(
            sentence_containing(paragraph, "threshold"),
            "完全无关的一段话。"
        );
    }

    #[test]
    fn empty_inputs_are_safe() {
        assert_eq!(sentence_containing("", "x"), "");
        assert_eq!(sentence_containing("   ", "x"), "");
        assert_eq!(sentence_containing("只有一句。", ""), "只有一句。");
    }

    #[test]
    fn trim_word_drops_trailing_cjk_punctuation() {
        // 实测案例：UIA 把 `当前进度：多服务商` 切成 `当前进度：`
        assert_eq!(trim_word("当前进度："), "当前进度");
        assert_eq!(trim_word("门槛。"), "门槛");
        assert_eq!(trim_word("（括号）"), "括号");
    }

    #[test]
    fn trim_word_drops_ascii_punctuation_but_keeps_inner() {
        assert_eq!(trim_word("threshold,"), "threshold");
        assert_eq!(trim_word("(threshold)"), "threshold");
        assert_eq!(trim_word("don't"), "don't");
        assert_eq!(trim_word("e-mail"), "e-mail");
        assert_eq!(trim_word("well-known."), "well-known");
    }

    #[test]
    fn trim_word_drops_invisible_and_symbol_chars() {
        // 截图里的 `测智` 后面跟了个替换符方框
        assert_eq!(trim_word("测智\u{FFFD}"), "测智");
        assert_eq!(trim_word("测智□"), "测智");
        assert_eq!(trim_word("\u{200B}threshold"), "threshold");
        assert_eq!(trim_word("threshold\u{FE0F}"), "threshold");
    }

    #[test]
    fn trim_word_can_empty_out() {
        assert_eq!(trim_word("：。！"), "");
        assert_eq!(trim_word("  "), "");
    }

    #[test]
    fn handles_text_without_terminators() {
        assert_eq!(
            sentence_containing("没有任何标点的长句", "标点"),
            "没有任何标点的长句"
        );
    }
}
