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
    fn handles_text_without_terminators() {
        assert_eq!(
            sentence_containing("没有任何标点的长句", "标点"),
            "没有任何标点的长句"
        );
    }
}
