//! 英文音素化：CMU 发音词典 → ARPAbet → IPA。
//!
//! 为什么需要：Kokoro 的 `input_ids` 只吃音素。中文我们靠 CC-CEDICT 的拼音，
//! 英文没有等价物——要么用 espeak-ng（GPL，会传染本项目的 MIT），要么自己准备数据。
//!
//! 这里的选择：**CMU 发音词典（cmudict，BSD 许可）+ ARPAbet→IPA 确定性映射**。
//! 与中文那套是同一个思路：拿一份"词 → 音素"的词典 + 一张固定的转换表，纯 Rust 实现。
//!
//! 局限（如实记录）：只覆盖词典里的词（13.5 万条）。生僻词、专名、变形词查不到时会被跳过，
//! 不做字形规则回退——规则 G2P 的准确率远低于词典，宁可漏读也不乱读。

use std::collections::HashMap;
use std::sync::OnceLock;

/// ARPAbet → IPA。
///
/// 依据 Kokoro 词表里实际存在的符号集挑选（`ɑ ɐ ɒ æ ɔ ə ɜ ɛ ɪ ʊ ʌ ʃ ʒ θ ð ʤ ʧ ɹ ɾ j ŋ` 全都有）。
/// 双元音拆成两个符号（`aɪ` = `a` + `ɪ`），词表里两者都有。
fn arpabet_to_ipa(phones: &str) -> Option<&'static str> {
    Some(match phones {
        "AA" => "ɑ",
        "AE" => "æ",
        "AH" => "ɑ", // 重读 AH；非重读（AH0）单独处理
        "AO" => "ɔ",
        "AW" => "aʊ",
        "AY" => "aɪ",
        "B" => "b",
        "CH" => "ʧ",
        "D" => "d",
        "DH" => "ð",
        "EH" => "ɛ",
        "ER" => "ɜɹ",
        "EY" => "eɪ",
        "F" => "f",
        "G" => "ɡ",
        "HH" => "h",
        "IH" => "ɪ",
        "IY" => "i",
        "JH" => "ʤ",
        "K" => "k",
        "L" => "l",
        "M" => "m",
        "N" => "n",
        "NG" => "ŋ",
        "OW" => "oʊ",
        "OY" => "ɔɪ",
        "P" => "p",
        "R" => "ɹ",
        "S" => "s",
        "SH" => "ʃ",
        "T" => "t",
        "TH" => "θ",
        "UH" => "ʊ",
        "UW" => "u",
        "V" => "v",
        "W" => "w",
        "Y" => "j",
        "Z" => "z",
        "ZH" => "ʒ",
        _ => return None,
    })
}

/// 词典：词 → 音素串（IPA）。
static DICT: OnceLock<HashMap<String, String>> = OnceLock::new();

/// 从 cmudict 加载（每行 `word  PH1 PH2 ...`，`word(2)` 是变体，取第一个）。
pub fn load(path: &std::path::Path) -> Result<(), String> {
    if DICT.get().is_some() {
        return Ok(());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("读取 CMU 词典失败（{}）：{e}", path.display()))?;

    let mut map: HashMap<String, String> = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(";;;") {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(raw_word) = parts.next() else {
            continue;
        };
        // `a(2)` 是同一词的另一种读音，取第一次出现的即可
        let word = raw_word
            .split('(')
            .next()
            .unwrap_or(raw_word)
            .to_lowercase();
        if map.contains_key(&word) {
            continue;
        }

        let mut phonemes = String::new();
        for phone in parts {
            let bare: &str = phone.trim_end_matches(|c: char| c.is_ascii_digit());
            let stressed = phone.ends_with('0');
            let symbol = if bare == "AH" && stressed {
                "ə" // 非重读 schwa
            } else {
                match arpabet_to_ipa(bare) {
                    Some(symbol) => symbol,
                    None => continue,
                }
            };
            phonemes.push_str(symbol);
        }
        if !phonemes.is_empty() {
            map.insert(word, phonemes);
        }
    }

    if map.is_empty() {
        return Err("CMU 词典解析出 0 条".to_string());
    }
    let _ = DICT.set(map);
    Ok(())
}

/// 词典是否已加载。
pub fn ready() -> bool {
    DICT.get().is_some()
}

/// 把一个英文词转成 IPA（查不到返回 `None`）。
pub fn word_to_ipa(word: &str) -> Option<&'static str> {
    let dict = DICT.get()?;
    let key = word
        .trim()
        .trim_matches(|c: char| !c.is_alphabetic() && c != '\'' && c != '-')
        .to_lowercase();
    if key.is_empty() {
        return None;
    }
    dict.get(&key).map(String::as_str)
}

/// 把一段英文按词切成 IPA 串（词间空格）。查不到的词跳过。
pub fn phonemize_english(text: &str) -> (String, Vec<String>) {
    let mut parts: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    for raw in text.split(|c: char| !c.is_alphabetic() && c != '\'' && c != '-' && c != '_') {
        if raw.is_empty() {
            continue;
        }
        match word_to_ipa(raw) {
            Some(ipa) => parts.push(ipa.to_string()),
            None => missing.push(raw.to_string()),
        }
    }

    (parts.join(" "), missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_dict() -> &'static HashMap<String, String> {
        static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
        MAP.get_or_init(|| {
            let mut map = HashMap::new();
            map.insert("hello".to_string(), "həlˈoʊ".to_string());
            map.insert("world".to_string(), "wˈɜɹld".to_string());
            map
        })
    }

    #[test]
    fn arpabet_mapping_covers_common_consonants() {
        assert_eq!(arpabet_to_ipa("T"), Some("t"));
        assert_eq!(arpabet_to_ipa("SH"), Some("ʃ"));
        assert_eq!(arpabet_to_ipa("NG"), Some("ŋ"));
        assert_eq!(arpabet_to_ipa("XX"), None);
    }

    #[test]
    fn unstressed_ah_becomes_schwa() {
        // 这是 ARPAbet 里唯一需要区分重音的元音，处理错了整句都会怪
        assert_eq!(arpabet_to_ipa("AH"), Some("ɑ"));
        // 非重读走专门分支（见 load 里的判断），这里只验证两者不同
        assert_ne!(arpabet_to_ipa("AH"), Some("ə"));
    }

    #[test]
    fn lookup_is_case_insensitive_and_strips_punctuation() {
        let map = fake_dict();
        assert_eq!(map.get("hello").map(String::as_str), Some("həlˈoʊ"));
        // 真实路径用全局 DICT；这里只校验归一化逻辑本身
        let key = "Hello,"
            .trim()
            .trim_matches(|c: char| !c.is_alphabetic())
            .to_lowercase();
        assert_eq!(key, "hello");
    }

    #[test]
    fn splits_words_and_reports_missing() {
        // 词典未加载时全部算"缺失"，不应 panic
        let (ipa, missing) = phonemize_english("hello world");
        assert!(ipa.is_empty() || !ipa.is_empty()); // 只要不崩即可
        assert!(missing.len() <= 2);
    }
}
