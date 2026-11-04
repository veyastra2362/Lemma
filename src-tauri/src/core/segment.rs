//! 中文分词：逆向最大匹配（RMM）。
//!
//! 为什么需要：UIA 的 `TextUnit_Word` 对中文只在标点/空白处断词，实测会返回单字（`第`）
//! 或整段（`当前进度：`）。词库是英→中，中文取词因此基本只能得到"未收录"。
//!
//! 为什么不引 jieba 词频表：
//! - 词频 + 最短路径 DP 的准确率更高，但要额外 5 MB 内存与第二份数据源；
//! - CC-CEDICT 的 12 万词条已经覆盖绝大多数阅读场景，RMM 在这上面表现稳定；
//! - 内存预算按"主进程 30–60 MB"算，能省 5 MB 就省。
//!
//! 纯逻辑，词表由调用方注入，因此可以直接单测。

// 用排序 Vec + 二分而不是 HashSet：121k 词条下 HashSet 的每项开销实测让进程多占 30 MB，

/// 单个分词结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    /// 该词是否在词表里（`false` 表示按单字/整段兜底切出来的）。
    pub known: bool,
}

/// 判断一个字符是不是汉字（含扩展 A 区）。
pub fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

/// 文本里是否含汉字。
pub fn contains_cjk(text: &str) -> bool {
    text.chars().any(is_cjk)
}

/// 有序词表（按字典序）。二分查找命中率与 HashSet 等价，内存小一个数量级。
pub struct Segmenter {
    words: Vec<String>,
    /// 词表里最长词的字符数，RMM 的尝试上界。
    max_len: usize,
}

impl Segmenter {
    pub fn new<I: IntoIterator<Item = String>>(words: I) -> Self {
        let mut list: Vec<String> = words
            .into_iter()
            .filter_map(|word| {
                let trimmed = word.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .collect();
        list.sort_unstable();
        list.dedup();

        let max_len = list
            .iter()
            .map(|word| word.chars().count())
            .max()
            .unwrap_or(1);

        Self {
            words: list,
            max_len,
        }
    }

    pub fn is_word(&self, word: &str) -> bool {
        self.words
            .binary_search_by(|candidate| candidate.as_str().cmp(word))
            .is_ok()
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn max_len(&self) -> usize {
        self.max_len
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// 把文本切成词。汉字走 RMM，非汉字（英文/数字/标点）原样成段保留。
    pub fn segment(&self, text: &str) -> Vec<Token> {
        let mut tokens = Vec::new();

        for (run, is_cjk_run) in split_runs(text) {
            if !is_cjk_run {
                tokens.push(Token {
                    text: run.to_string(),
                    known: false,
                });
                continue;
            }
            self.segment_cjk_run(run, &mut tokens);
        }

        tokens
    }

    /// 逆向最大匹配：从右往左，尽量取最长的已知词。
    fn segment_cjk_run(&self, run: &str, out: &mut Vec<Token>) {
        let chars: Vec<char> = run.chars().collect();
        let mut end = chars.len();
        let mut reversed: Vec<Token> = Vec::new();

        while end > 0 {
            let mut matched = None;
            let upper = self.max_len.min(end);
            for len in (1..=upper).rev() {
                let start = end - len;
                let candidate: String = chars[start..end].iter().collect();
                if self.is_word(&candidate) {
                    matched = Some((len, candidate));
                    break;
                }
            }

            match matched {
                Some((len, word)) => {
                    reversed.push(Token {
                        text: word,
                        known: true,
                    });
                    end -= len;
                }
                None => {
                    // 词表里没有：单字兜底，再去一次空白
                    let start = end - 1;
                    let ch: String = chars[start..end].iter().collect();
                    if !ch.trim().is_empty() {
                        reversed.push(Token {
                            text: ch,
                            known: false,
                        });
                    }
                    end -= 1;
                }
            }
        }

        reversed.reverse();
        out.extend(reversed);
    }
}

/// 把文本按「汉字段 / 非汉字段」切开。
fn split_runs(text: &str) -> Vec<(&str, bool)> {
    let mut runs = Vec::new();
    let mut start = 0usize;
    let mut current: Option<bool> = None;
    let mut offset = 0usize;

    for ch in text.chars() {
        let is_cjk = is_cjk(ch);
        match current {
            None => {
                current = Some(is_cjk);
                start = offset;
            }
            Some(prev) if prev != is_cjk => {
                runs.push((&text[start..offset], prev));
                start = offset;
                current = Some(is_cjk);
            }
            _ => {}
        }
        offset += ch.len_utf8();
    }

    if let Some(prev) = current {
        runs.push((&text[start..], prev));
    }
    runs
}

static GLOBAL: std::sync::OnceLock<Option<Segmenter>> = std::sync::OnceLock::new();

/// 懒加载的全局分词器。
///
/// 第一次遇到中文取词时才从词库读出 12 万词条建表（约 2–3 MB 内存）。
/// 只读中文的用户完全不会付这份成本。
pub fn global() -> Option<&'static Segmenter> {
    GLOBAL
        .get_or_init(|| {
            let started = std::time::Instant::now();
            let words = match load_words() {
                Ok(words) => words,
                Err(error) => {
                    crate::core::logs::line(format!("segmenter load failed: {error}"));
                    return None;
                }
            };
            if words.is_empty() {
                return None;
            }
            let segmenter = Segmenter::new(words);
            crate::core::logs::line(format!(
                "segmenter ready words={} maxLen={} load={:.0}ms",
                segmenter.len(),
                segmenter.max_len(),
                started.elapsed().as_secs_f64() * 1000.0
            ));
            Some(segmenter)
        })
        .as_ref()
}

/// 读词表：优先侧文件（快、不碰 redb 页），缺失或过期则从词库重建并补写。
fn load_words() -> Result<Vec<String>, String> {
    let words_path = crate::core::paths::zh_words_path();

    let dict_count = crate::core::dict::global()
        .map(|store| store.zh_len())
        .unwrap_or(0);

    if let Some(words_path) = &words_path
        && words_path.exists()
        && let Ok(content) = std::fs::read_to_string(words_path)
        && is_fresh(&content, dict_count)
    {
        let words: Vec<String> = content
            .lines()
            .skip(1)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        if !words.is_empty() {
            return Ok(words);
        }
    }

    // 回退：从词库读，并补写侧文件供下次使用。
    let store = crate::core::dict::open_default()?;
    let words = store.zh_words()?;
    if let Some(words_path) = words_path {
        write_side_file(&words_path, &words, dict_count);
    }
    Ok(words)
}

/// 侧文件是否还能用。
///
/// 原来比较 mtime，但**打开 redb 本身就会更新词库文件时间**，导致每次启动都判定过期、
/// 白遍历一遍中文表（正是这套优化要避免的 23 MB 工作集）。
/// 改成把词条数写在侧文件第一行，与词库 meta 里的 `zhCount` 比对——导入变了才失效。
fn is_fresh(side_content: &str, dict_count: u64) -> bool {
    let Some(first) = side_content.lines().next() else {
        return false;
    };
    let Some(value) = first.strip_prefix("#count=") else {
        return false;
    };
    value
        .trim()
        .parse::<u64>()
        .is_ok_and(|count| count == dict_count)
}

/// 写侧文件（排序去重，每行一词）。
/// 写侧文件。`dict_count` 必须与词库 meta 的 `zhCount` 一致（用 `DictStore::zh_len()` 取），
/// 否则下次加载会判定过期、白重建一遍。
pub fn write_side_file(path: &std::path::Path, words: &[String], dict_count: u64) {
    let mut sorted: Vec<&str> = words.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    // 第一行写词条数：加载时与词库 meta 比对，判断侧文件是否还能用
    let mut content = format!("#count={dict_count}\n");
    content.push_str(&sorted.join("\n"));
    content.push('\n');
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(path, content) {
        Ok(()) => crate::core::logs::line(format!("zh words side file written: {}", sorted.len())),
        Err(error) => crate::core::logs::line(format!("zh words side file failed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segmenter() -> Segmenter {
        Segmenter::new(
            [
                "当前",
                "进度",
                "服务",
                "多",
                "商",
                "助手",
                "人工智能",
                "阅读",
                "深度",
            ]
            .iter()
            .map(|word| word.to_string()),
        )
    }

    fn words(text: &str) -> Vec<String> {
        segmenter()
            .segment(text)
            .into_iter()
            .filter(|token| token.known)
            .map(|token| token.text)
            .collect()
    }

    #[test]
    fn splits_a_two_word_phrase() {
        assert_eq!(words("当前进度"), vec!["当前", "进度"]);
    }

    #[test]
    fn reverse_max_match_prefers_longer_words_from_the_right() {
        // 从右往左：商 → 服务 → 多
        assert_eq!(words("多服务商"), vec!["多", "服务", "商"]);
    }

    #[test]
    fn keeps_punctuation_out_of_words() {
        let tokens = segmenter().segment("当前进度：多服务商");
        assert_eq!(
            tokens.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            vec!["当前", "进度", "：", "多", "服务", "商"]
        );
        // 标点不算已知词
        assert!(!tokens.iter().find(|t| t.text == "：").unwrap().known);
    }

    #[test]
    fn unknown_chinese_falls_back_to_single_chars() {
        let tokens = segmenter().segment("龘靐");
        assert_eq!(tokens.len(), 2);
        assert!(tokens.iter().all(|token| !token.known));
    }

    #[test]
    fn non_chinese_runs_are_passed_through() {
        let tokens = segmenter().segment("用 Lemma 阅读");
        assert_eq!(
            tokens.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            vec!["用", " Lemma ", "阅读"]
        );
    }

    #[test]
    fn picks_the_longest_known_word() {
        assert_eq!(words("人工智能助手"), vec!["人工智能", "助手"]);
    }

    #[test]
    fn detects_cjk() {
        assert!(contains_cjk("中文"));
        assert!(contains_cjk("Lemma 中文"));
        assert!(!contains_cjk("Lemma only"));
        assert!(is_cjk('汉'));
        assert!(!is_cjk('A'));
    }

    #[test]
    fn empty_input_is_safe() {
        assert!(segmenter().segment("").is_empty());
        assert!(Segmenter::new(std::iter::empty()).segment("中文").len() == 2);
    }
}
