//! 取词编排：UIA 两档 → 剪贴板兜底。
//!
//! 档位决策是纯逻辑（[`pick_uia`]），可以直接单测；真正的 OS 调用都在 `platform`。
//!
//! **顺序很重要**：必须在呼出 HUD **之前**取词。HUD 一显示就会抢走前台焦点，
//! 而 T2 剪贴板兜底是给「前台窗口」发 Ctrl+C 的——先弹窗就等于对空发。
//! 所以调用方（`core::trigger`）的顺序是：capture() → 再 present()。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::core::dict::{self, Entry};
use crate::core::segment::{self, contains_cjk};
use crate::core::settings::CaptureMode;
use crate::core::text::{sentence_containing, trim_word};
use crate::core::translate::is_single_word;
use crate::platform::{self, clipboard, uia::UiaOutcome};

/// 等剪贴板变化的上限。给低了会在慢应用上误判失败，给高了用户会觉得卡。
const CLIPBOARD_TIMEOUT: Duration = Duration::from_millis(450);

/// 命中档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tier {
    /// 屏幕区域 OCR。
    Ocr,
    /// UIA 选区直读。
    Selection,
    /// UIA 光标所在词。
    Word,
    /// 模拟 Ctrl+C 兜底。
    Clipboard,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::Ocr => "截图",
            Tier::Selection => "选区",
            Tier::Word => "光标词",
            Tier::Clipboard => "剪贴板",
        }
    }
}

/// 一次取词的结果，前端直接渲染这个。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capture {
    pub text: String,
    pub sentence: String,
    pub paragraph: String,
    /// None 表示这次没取到。
    pub tier: Option<Tier>,
    /// 词库命中的词条；没装词库或未收录时为 None。
    pub entry: Option<Entry>,
    /// 这段文本是不是「该走翻译」而不是查词。判定规则在 `translate::is_single_word`。
    pub needs_translation: bool,
    /// 中文多词时的候选词（点一个就下探）。空表示没有候选。
    pub candidates: Vec<String>,
    /// 来源进程名，纯诊断用。
    pub app: String,
    pub elapsed_ms: f64,
    pub error: Option<String>,
    /// 文本控件句柄（0 = 不可用，回填只能走粘贴或者不支持）。
    #[serde(default)]
    pub control_hwnd: i64,
    /// 源窗口顶层句柄（回填前要抢焦点）。
    #[serde(default)]
    pub source_hwnd: i64,
}

/// 从 UIA 结果里挑文本：选区优先，其次光标词。空白一律视为没有。
///
/// 纯函数——这是整个取词链路里最该被单测覆盖的部分。
pub fn pick_uia(outcome: &UiaOutcome, mode: CaptureMode) -> Option<(Tier, String)> {
    fn clean(value: &Option<String>) -> Option<String> {
        value
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    }

    let selection = clean(&outcome.selection).map(|text| (Tier::Selection, text));

    // 光标词必须洗掉首尾标点：UIA 的 WordUnit 对中文只按标点/空白断词，
    // 会带出 `当前进度：` 这种尾随标点，甚至 `测智\u{FFFD}` 这种替换符。
    let word = outcome
        .word
        .as_deref()
        .map(trim_word)
        .filter(|text| !text.is_empty())
        .map(|text| (Tier::Word, text));

    match mode {
        // 只认选区：浏览器推荐这一档——实测 Chrome 选区 8ms 且稳定，悬停取词不稳定。
        CaptureMode::SelectionOnly => selection,
        // 只认光标词：原生应用里"悬停即查"。
        CaptureMode::HoverOnly => word,
        CaptureMode::Auto => selection.or(word),
    }
}

/// 把 OCR 结果装成一次「取词」，复用后面所有链路（HUD / 翻译 / 双栏）。
///
/// 注意：`text` 用整段 OCR 文本，`sentence` 取第一行——
/// HUD 的句卡片只显示一句，多了会挤爆。
pub fn store_ocr(text: String, app: String, elapsed_ms: f64) -> Capture {
    let first_line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string();
    let needs_translation = !is_single_word(&text);
    let capture = Capture {
        text: text.clone(),
        sentence: first_line,
        paragraph: text,
        tier: Some(Tier::Ocr),
        entry: None,
        needs_translation,
        candidates: Vec::new(),
        app,
        elapsed_ms,
        error: None,
        // OCR 是屏幕区域，没有可写的文本控件
        control_hwnd: 0,
        source_hwnd: 0,
    };
    if let Ok(mut slot) = slot().lock() {
        *slot = Some(capture.clone());
    }
    capture
}

/// 把一张现成的 `Capture` 放进"最近一次取词"槽。
///
/// 冒烟与测试用：`store_ocr` 存的是它自己构造的那一份，之后再改字段不会生效。
pub fn store_capture(capture: &Capture) {
    store(capture);
}

/// 中文取词：分词出候选。
///
/// 返回 `(用于查词的文本, 候选列表)`：
/// - 词表里只切出一个词 → 直接查那个词；
/// - 切出多个 → 返回候选，让用户点（不猜他想查哪个）；
/// - 没切出（或本来就是句子）→ 原样返回文本。
fn cjk_candidates(text: &str, needs_translation: bool) -> (String, Vec<String>) {
    if needs_translation || !contains_cjk(text) {
        return (text.to_string(), Vec::new());
    }
    let Some(segmenter) = segment::global() else {
        return (text.to_string(), Vec::new());
    };

    let words: Vec<String> = segmenter
        .segment(text)
        .into_iter()
        .filter(|token| token.known)
        .map(|token| token.text)
        .collect();

    match words.len() {
        0 => (text.to_string(), Vec::new()),
        1 => (words[0].clone(), Vec::new()),
        _ => (text.to_string(), words),
    }
}

/// 查词：含汉字走中文词表，否则走英文词库。
fn lookup_entry(text: &str) -> Option<Entry> {
    let store = dict::global()?;
    if contains_cjk(text) {
        store.get_zh(text).ok().flatten()
    } else {
        store.get(text).ok().flatten()
    }
}

static LAST: OnceLock<Mutex<Option<Capture>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Capture>> {
    LAST.get_or_init(|| Mutex::new(None))
}

/// 最近一次取词结果（HUD 打开后向 Rust 要这个）。
pub fn last() -> Option<Capture> {
    slot().lock().ok().and_then(|capture| capture.clone())
}

fn store(capture: &Capture) {
    if let Ok(mut slot) = slot().lock() {
        *slot = Some(capture.clone());
    }
    // 只记前 40 字符：够定位问题，又不至于把用户读的东西整段留在日志里。
    let preview: String = capture.text.chars().take(40).collect();
    crate::core::logs::line(format!(
        "capture tier={} app={} elapsed={:.0}ms dict={} err={} text={preview}",
        capture.tier.map(Tier::label).unwrap_or("-"),
        if capture.app.is_empty() {
            "?"
        } else {
            &capture.app
        },
        capture.elapsed_ms,
        if capture.entry.is_some() { "hit" } else { "-" },
        capture.error.as_deref().unwrap_or("-")
    ));
}

/// 取词：UIA 优先，拿不到再走剪贴板兜底。
pub fn capture(mode: CaptureMode) -> Capture {
    let started = Instant::now();
    let uia = platform::uia_capture();

    if let Some((tier, text)) = pick_uia(&uia, mode) {
        let needs_translation = !is_single_word(&text);
        let (lookup_text, candidates) = cjk_candidates(&text, needs_translation);
        let entry = lookup_entry(&lookup_text);
        let result = Capture {
            sentence: sentence_containing(&uia.paragraph, &text),
            paragraph: uia.paragraph,
            entry,
            needs_translation,
            candidates,
            text,
            tier: Some(tier),
            app: uia.app,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            control_hwnd: uia.control_hwnd,
            source_hwnd: uia.source_hwnd,
            error: None,
        };
        store(&result);
        return result;
    }

    let uia_note = uia.note;

    let result = match clipboard::copy_selection(CLIPBOARD_TIMEOUT) {
        Ok(lease) => {
            let text = trim_word(&lease.text);
            // 立刻还剪贴板，别让用户的剪贴板停在中间状态。
            drop(lease);
            if text.is_empty() {
                Capture {
                    app: uia.app,
                    elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                    error: Some("剪贴板兜底也没拿到文本".to_string()),
                    ..Default::default()
                }
            } else {
                Capture {
                    // 剪贴板里只有选区文本，没有段落，句子就取它自己。
                    sentence: text.clone(),
                    paragraph: text.clone(),
                    entry: lookup_entry(&text),
                    needs_translation: !is_single_word(&text),
                    candidates: cjk_candidates(&text, !is_single_word(&text)).1,
                    text,
                    tier: Some(Tier::Clipboard),
                    app: uia.app,
                    elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                    control_hwnd: uia.control_hwnd,
                    source_hwnd: uia.source_hwnd,
                    error: None,
                }
            }
        }
        Err(error) => Capture {
            app: uia.app,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            error: Some(if uia_note.is_empty() {
                format!("没有选中文本（{error}）")
            } else {
                format!("{uia_note}；剪贴板兜底也失败：{error}")
            }),
            ..Default::default()
        },
    };

    store(&result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(selection: Option<&str>, word: Option<&str>) -> UiaOutcome {
        UiaOutcome {
            selection: selection.map(str::to_string),
            word: word.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn prefers_the_selection_over_the_word() {
        let picked = pick_uia(
            &outcome(Some("selected text"), Some("word")),
            CaptureMode::Auto,
        )
        .unwrap();
        assert_eq!(picked.0, Tier::Selection);
        assert_eq!(picked.1, "selected text");
    }

    #[test]
    fn falls_back_to_the_word_when_the_selection_is_empty() {
        let picked = pick_uia(&outcome(Some("   "), Some("threshold")), CaptureMode::Auto).unwrap();
        assert_eq!(picked.0, Tier::Word);
        assert_eq!(picked.1, "threshold");
    }

    #[test]
    fn nothing_available_yields_none() {
        assert!(pick_uia(&outcome(None, None), CaptureMode::Auto).is_none());
        assert!(pick_uia(&outcome(Some(""), Some("  \t\n")), CaptureMode::Auto).is_none());
    }

    #[test]
    fn text_is_trimmed() {
        let picked = pick_uia(&outcome(Some("  padded  "), None), CaptureMode::Auto).unwrap();
        assert_eq!(picked.1, "padded");
    }

    #[test]
    fn cursor_word_loses_trailing_punctuation() {
        let picked = pick_uia(&outcome(None, Some("当前进度：")), CaptureMode::Auto)
            .expect("应该命中光标词");
        assert_eq!(picked.0, Tier::Word);
        assert_eq!(picked.1, "当前进度");
    }

    #[test]
    fn a_word_that_is_only_punctuation_counts_as_missing() {
        assert!(pick_uia(&outcome(None, Some("：。")), CaptureMode::Auto).is_none());
    }

    #[test]
    fn selection_only_mode_ignores_the_cursor_word() {
        let picked = pick_uia(&outcome(None, Some("hovered")), CaptureMode::SelectionOnly);
        assert!(picked.is_none(), "只认选区时不应回退到光标词");
    }

    #[test]
    fn hover_only_mode_ignores_the_selection() {
        let picked = pick_uia(
            &outcome(Some("selected text"), Some("hovered")),
            CaptureMode::HoverOnly,
        );
        assert_eq!(picked.unwrap().0, Tier::Word);
    }

    #[test]
    fn auto_mode_prefers_the_selection() {
        let picked = pick_uia(
            &outcome(Some("selected text"), Some("hovered")),
            CaptureMode::Auto,
        );
        assert_eq!(picked.unwrap().0, Tier::Selection);
    }

    #[test]
    fn cjk_segmentation_produces_candidates() {
        // 词表没导入时不应崩，只是没有候选
        let (lookup, candidates) = super::cjk_candidates("多服务商", false);
        assert_eq!(lookup, "多服务商");
        let _ = candidates;
    }

    #[test]
    fn sentences_never_become_candidates() {
        // 需要翻译的长句不参与分词
        let (lookup, candidates) = super::cjk_candidates("这是一句需要翻译的话。", true);
        assert_eq!(lookup, "这是一句需要翻译的话。");
        assert!(candidates.is_empty());
    }

    #[test]
    fn english_text_is_not_segmented() {
        let (lookup, candidates) = super::cjk_candidates("threshold", false);
        assert_eq!(lookup, "threshold");
        assert!(candidates.is_empty());
    }

    #[test]
    fn last_capture_round_trips() {
        let capture = Capture {
            text: "threshold".to_string(),
            tier: Some(Tier::Word),
            ..Default::default()
        };
        store(&capture);
        assert_eq!(last().unwrap().text, "threshold");
        assert_eq!(last().unwrap().tier, Some(Tier::Word));
    }
}
