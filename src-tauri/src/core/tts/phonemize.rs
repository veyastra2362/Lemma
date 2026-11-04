//! 中文音素化：拼音 → 注音（Bopomofo）+ 声调数字。
//!
//! 为什么要自己写：Kokoro 的 `input_ids` 吃的是**音素**不是文字，中文这一层
//! 成熟实现都在 Python（misaki / pypinyin）。为了保住「纯 Rust、无 Python 运行时、MIT 许可」，
//! 这里自己实现映射。
//!
//! 数据来源不是新词库：**CC-CEDICT 的每个词条本来就带拼音**（`杜甫 [du4 fu3]`），
//! 我们已经把 12 万条导进 redb 了（见 `core::dict::get_zh`）。所以这一层只做"拼音 → 注音"。
//!
//! 拼音 → 注音是确定性映射：21 个声母 + 37 个韵母 + 声调数字。

/// 一个已解析的拼音音节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Syllable {
    /// 注音字符串（不含声调）。
    pub bopomofo: String,
    /// 声调 1–5（5 = 轻声）。
    pub tone: u8,
}

const INITIALS: &[(&str, char)] = &[
    ("zh", 'ㄓ'),
    ("ch", 'ㄔ'),
    ("sh", 'ㄕ'),
    ("b", 'ㄅ'),
    ("p", 'ㄆ'),
    ("m", 'ㄇ'),
    ("f", 'ㄈ'),
    ("d", 'ㄉ'),
    ("t", 'ㄊ'),
    ("n", 'ㄋ'),
    ("l", 'ㄌ'),
    ("g", 'ㄍ'),
    ("k", 'ㄎ'),
    ("h", 'ㄏ'),
    ("j", 'ㄐ'),
    ("q", 'ㄑ'),
    ("x", 'ㄒ'),
    ("r", 'ㄖ'),
    ("z", 'ㄗ'),
    ("c", 'ㄘ'),
    ("s", 'ㄙ'),
];

/// 韵母表。键是拼音韵母（已把 `iu/ui/un` 这类缩写还原成全称）。
const FINALS: &[(&str, &str)] = &[
    ("iang", "ㄧㄤ"),
    ("iong", "ㄩㄥ"),
    ("uang", "ㄨㄤ"),
    ("ueng", "ㄨㄥ"),
    ("ong", "ㄨㄥ"),
    ("ang", "ㄤ"),
    ("eng", "ㄥ"),
    ("ian", "ㄧㄢ"),
    ("iao", "ㄧㄠ"),
    ("iou", "ㄧㄡ"),
    ("uai", "ㄨㄞ"),
    ("uei", "ㄨㄟ"),
    ("uan", "ㄨㄢ"),
    ("uen", "ㄨㄣ"),
    ("van", "ㄩㄢ"),
    ("ong", "ㄨㄥ"),
    ("ang", "ㄤ"),
    ("ai", "ㄞ"),
    ("ei", "ㄟ"),
    ("ao", "ㄠ"),
    ("ou", "ㄡ"),
    ("an", "ㄢ"),
    ("en", "ㄣ"),
    ("er", "ㄦ"),
    ("ia", "ㄧㄚ"),
    ("ie", "ㄧㄝ"),
    ("in", "ㄧㄣ"),
    ("ing", "ㄧㄥ"),
    ("ua", "ㄨㄚ"),
    ("uo", "ㄨㄛ"),
    ("ve", "ㄩㄝ"),
    ("vn", "ㄩㄣ"),
    ("a", "ㄚ"),
    ("o", "ㄛ"),
    ("e", "ㄜ"),
    ("i", "ㄧ"),
    ("u", "ㄨ"),
    ("v", "ㄩ"),
];

/// 零声母音节（y/w 开头与单韵母）单独处理：它们不是"声母 + 韵母"。
fn zero_initial(raw: &str) -> Option<&'static str> {
    Some(match raw {
        "yi" => "ㄧ",
        "ya" => "ㄧㄚ",
        "ye" => "ㄧㄝ",
        "yao" => "ㄧㄠ",
        "you" => "ㄧㄡ",
        "yan" => "ㄧㄢ",
        "yin" => "ㄧㄣ",
        "yang" => "ㄧㄤ",
        "ying" => "ㄧㄥ",
        "yong" => "ㄩㄥ",
        "yu" => "ㄩ",
        "yue" => "ㄩㄝ",
        "yuan" => "ㄩㄢ",
        "yun" => "ㄩㄣ",
        "wu" => "ㄨ",
        "wa" => "ㄨㄚ",
        "wo" => "ㄨㄛ",
        "wai" => "ㄨㄞ",
        "wei" => "ㄨㄟ",
        "wan" => "ㄨㄢ",
        "wen" => "ㄨㄣ",
        "wang" => "ㄨㄤ",
        "weng" => "ㄨㄥ",
        "a" => "ㄚ",
        "o" => "ㄛ",
        "e" => "ㄜ",
        "ai" => "ㄞ",
        "ei" => "ㄟ",
        "ao" => "ㄠ",
        "ou" => "ㄡ",
        "an" => "ㄢ",
        "en" => "ㄣ",
        "ang" => "ㄤ",
        "eng" => "ㄥ",
        "er" => "ㄦ",
        _ => return None,
    })
}

/// 归一化拼音写法：CC-CEDICT 用 `u:` 表示 ü，也有人写 `v`。
fn normalize(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .replace("u:", "v")
        .replace('ü', "v")
}

/// 把单个拼音音节（如 `du4`、`zhong1`、`nu:3`）转成注音 + 声调。
pub fn syllable(raw: &str) -> Option<Syllable> {
    let normalized = normalize(raw);
    let (body, tone) = match normalized.chars().last() {
        Some(last) if last.is_ascii_digit() => {
            let tone = last.to_digit(10)? as u8;
            (&normalized[..normalized.len() - 1], tone)
        }
        // 没有声调数字：按轻声处理
        _ => (normalized.as_str(), 5),
    };
    if body.is_empty() {
        return None;
    }

    // 零声母优先
    if let Some(bopomofo) = zero_initial(body) {
        return Some(Syllable {
            bopomofo: bopomofo.to_string(),
            tone,
        });
    }

    // 声母 + 韵母
    for (initial, symbol) in INITIALS {
        let Some(rest) = body.strip_prefix(initial) else {
            continue;
        };
        if rest.is_empty() {
            // 只有声母（极少见，如"嗯"的 ng）：直接给声母本身
            return Some(Syllable {
                bopomofo: symbol.to_string(),
                tone,
            });
        }

        // 还原缩写：iu→iou, ui→uei, un→uen（j/q/x/y 后的 un 实际是 ün）
        let expanded = match rest {
            "iu" => "iou",
            "ui" => "uei",
            "un" if matches!(*initial, "j" | "q" | "x") => "vn",
            "un" => "uen",
            other => other,
        };
        // **j/q/x 后面的 u 一律是 ü**（xu=ㄒㄩ、jue=ㄐㄩㄝ、juan=ㄐㄩㄢ）。
        // 这条不处理的话"需"会被念成 ㄒㄨ。
        let expanded = if matches!(*initial, "j" | "q" | "x") && expanded.starts_with('u') {
            format!("v{}", &expanded[1..])
        } else {
            expanded.to_string()
        };
        let expanded = if matches!(*initial, "j" | "q" | "x") && expanded == "v" {
            "v"
        } else {
            expanded.as_str()
        };

        // ㄭ：zhi/chi/shi/ri/zi/ci/si 里的 i 不是普通韵母
        if expanded == "i" && matches!(*initial, "zh" | "ch" | "sh" | "r" | "z" | "c" | "s") {
            return Some(Syllable {
                bopomofo: format!("{symbol}ㄭ"),
                tone,
            });
        }

        let final_symbol = FINALS
            .iter()
            .find(|(final_name, _)| *final_name == expanded)
            .map(|(_, symbol)| *symbol)?;
        return Some(Syllable {
            bopomofo: format!("{symbol}{final_symbol}"),
            tone,
        });
    }

    // 没有声母也没有匹配：当做独立韵母
    FINALS
        .iter()
        .find(|(final_name, _)| *final_name == body)
        .map(|(_, symbol)| Syllable {
            bopomofo: symbol.to_string(),
            tone,
        })
}

/// 拼接方式。
///
/// misaki（Kokoro 官方音素化）是 `' '.join(word2ipa(w) for w in words)`：
/// **词内音节直接相连、词间才留空格**。我们一开始逐音节加空格，等于把每个字当成一个词，
/// 听感就是"一个字一个字念"（用户试听反馈）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spacing {
    /// 词内相连、词间空格（默认，与 misaki 一致）
    Word,
    /// 每个音节都空格（旧行为，留作 A/B 对照）
    Syllable,
    /// 完全不留空格
    None,
}

impl Spacing {
    /// 从环境变量读，便于试听对比：`LEMMA_TTS_SPACING=word|syllable|none`
    pub fn from_env() -> Self {
        match std::env::var("LEMMA_TTS_SPACING").as_deref() {
            Ok("syllable") => Self::Syllable,
            Ok("none") => Self::None,
            _ => Self::Word,
        }
    }
}

/// 把一个词内部的音节拼起来（不加分隔）。
pub fn phonemize_word(syllables: &[Syllable]) -> String {
    syllables
        .iter()
        .map(|syllable| format!("{}{}", syllable.bopomofo, syllable.tone))
        .collect()
}

/// 把一串音节拼成音素串（旧接口：逐音节空格）。
pub fn phonemize(syllables: &[Syllable]) -> String {
    syllables
        .iter()
        .map(|syllable| format!("{}{}", syllable.bopomofo, syllable.tone))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 中文标点 → Kokoro 词表里的标点。
///
/// 韵律很大程度靠标点驱动（逗号短停、句号长停），词表里有 `, . ! ? ; : — …`。
/// 不映射的话标点会被词表过滤掉，整段就没有停顿了。
pub fn punctuation(ch: char) -> Option<char> {
    Some(match ch {
        '，' | '、' | '；' | '：' => ',',
        '。' | '．' => '.',
        '！' => '!',
        '？' => '?',
        '…' => '.',
        '—' | '–' | '－' => '-',
        ',' | '.' | '!' | '?' | ';' | ':' => ch,
        _ => return None,
    })
}

/// 把 CC-CEDICT 的拼音串（`du4 fu3`）整体转成音素串。
pub fn phonemize_pinyin(pinyin: &str) -> String {
    let syllables: Vec<Syllable> = pinyin.split_whitespace().filter_map(syllable).collect();
    phonemize(&syllables)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(raw: &str) -> (String, u8) {
        let syllable = syllable(raw).unwrap_or_else(|| panic!("解析失败：{raw}"));
        (syllable.bopomofo, syllable.tone)
    }

    #[test]
    fn simple_syllables() {
        assert_eq!(one("du4"), ("ㄉㄨ".to_string(), 4));
        assert_eq!(one("fu3"), ("ㄈㄨ".to_string(), 3));
        assert_eq!(one("zhong1"), ("ㄓㄨㄥ".to_string(), 1));
        assert_eq!(one("wen2"), ("ㄨㄣ".to_string(), 2));
    }

    #[test]
    fn retroflex_and_sibilant_apical_vowel() {
        // zhi/chi/shi/ri/zi/ci/si 的 i 是 ㄭ，不是 ㄧ
        assert_eq!(one("shi4"), ("ㄕㄭ".to_string(), 4));
        assert_eq!(one("zi5"), ("ㄗㄭ".to_string(), 5));
        assert_eq!(one("ri4"), ("ㄖㄭ".to_string(), 4));
    }

    #[test]
    fn zero_initial_syllables() {
        assert_eq!(one("yi1"), ("ㄧ".to_string(), 1));
        assert_eq!(one("yu3"), ("ㄩ".to_string(), 3));
        assert_eq!(one("wu3"), ("ㄨ".to_string(), 3));
        assert_eq!(one("ai4"), ("ㄞ".to_string(), 4));
        assert_eq!(one("er4"), ("ㄦ".to_string(), 4));
    }

    #[test]
    fn abbreviated_finals_are_expanded() {
        assert_eq!(one("liu2"), ("ㄌㄧㄡ".to_string(), 2));
        assert_eq!(one("gui1"), ("ㄍㄨㄟ".to_string(), 1));
        // j/q/x 后面的 un 实际是 ün
        assert_eq!(one("jun1"), ("ㄐㄩㄣ".to_string(), 1));
        // 普通声母后面的 un 是 uen
        assert_eq!(one("lun2"), ("ㄌㄨㄣ".to_string(), 2));
    }

    #[test]
    fn u_umlaut_spellings() {
        // CC-CEDICT 用 u:，也有人写 v
        assert_eq!(one("nu:3"), ("ㄋㄩ".to_string(), 3));
        assert_eq!(one("lv4"), ("ㄌㄩ".to_string(), 4));
        assert_eq!(one("lve4"), ("ㄌㄩㄝ".to_string(), 4));
    }

    #[test]
    fn u_after_j_q_x_is_umlaut() {
        // 拼音规则：j/q/x 后的 u 就是 ü
        assert_eq!(one("xu1"), ("ㄒㄩ".to_string(), 1));
        assert_eq!(one("ju2"), ("ㄐㄩ".to_string(), 2));
        assert_eq!(one("jue2"), ("ㄐㄩㄝ".to_string(), 2));
        assert_eq!(one("juan3"), ("ㄐㄩㄢ".to_string(), 3));
        assert_eq!(one("qu4"), ("ㄑㄩ".to_string(), 4));
    }

    #[test]
    fn missing_tone_is_neutral() {
        assert_eq!(one("de"), ("ㄉㄜ".to_string(), 5));
    }

    #[test]
    fn phonemize_joined_with_spaces_and_tone_digits() {
        assert_eq!(phonemize_pinyin("du4 fu3"), "ㄉㄨ4 ㄈㄨ3");
        assert_eq!(phonemize_pinyin("wo3 yao4 yong4"), "ㄨㄛ3 ㄧㄠ4 ㄩㄥ4");
    }

    #[test]
    fn all_bopomofo_symbols_exist_in_the_kokoro_vocab() {
        // 这一步防的是"映射表拼出了词表里没有的符号"——那会静默丢音。
        // 词表见 data/tts-models/tokenizer.json（172 项，含注音与数字声调）。
        const VOCAB: &str = "ㄅㄆㄇㄈㄉㄊㄋㄌㄍㄎㄏㄐㄑㄒㄓㄔㄕㄖㄗㄘㄙ\
ㄚㄛㄜㄝㄞㄟㄠㄡㄢㄣㄤㄥㄦㄧㄨㄩㄭ";
        let mut seen = std::collections::HashSet::new();
        for (_, symbol) in INITIALS {
            seen.insert(*symbol);
        }
        for (_, symbols) in FINALS {
            for ch in symbols.chars() {
                seen.insert(ch);
            }
        }
        for ch in seen {
            assert!(VOCAB.contains(ch), "映射表产生了词表里没有的符号：{ch}");
        }
    }
}
