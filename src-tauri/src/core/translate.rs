//! 翻译层：分级路由 + 语义缓存。
//!
//! 设计原则（对应 `docs/PLAN.md` 的 D1「混合分级」）：
//!
//! | 档 | 用途 | 代价 |
//! | --- | --- | --- |
//! | 本地词典 | 单词 | 2 µs，零网络 |
//! | 在线免密（Google gtx） | 句子 / 段落 | ~700 ms，零配置 |
//! | OpenAI 兼容接口 | 质量优先（可选） | 需要用户填 base_url + key |
//!
//! **缓存放在最前面**：深度阅读时同一段被反复看是常态，命中率不低；
//! 缓存键 = `目标语言 + 原文`，进程内 LRU（重启即失效，够用）。

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// 目标语言。目前只做中英互译这两个方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    /// Google gtx 用的语言码。
    pub fn google_code(self) -> &'static str {
        match self {
            Lang::Zh => "zh-CN",
            Lang::En => "en",
        }
    }

    /// 给模型用的自然语言名（OpenAI 提示里更好用）。
    pub fn prompt_name(self) -> &'static str {
        match self {
            Lang::Zh => "简体中文",
            Lang::En => "English",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    /// 免密在线接口，零配置。默认。
    Google,
    /// OpenAI 兼容接口（OpenAI / DeepSeek / 本地 llama.cpp 均可）。
    OpenAi,
}

/// 一次翻译要用的运行时配置，由命令层从设置里取出来传进来。
#[derive(Debug, Clone)]
pub struct TranslateConfig {
    pub provider: ProviderKind,
    pub target: Lang,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl Default for TranslateConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Google,
            target: Lang::Zh,
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: "gpt-4o-mini".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateResult {
    pub text: String,
    /// 实际使用的 provider（`cache` 表示命中缓存）。
    pub provider: String,
    pub elapsed_ms: f64,
    pub cached: bool,
}

/// 判断一段文本是不是「一个词」——决定走词典卡片还是翻译卡片。
///
/// 规则刻意保守：只要含空白或句末/句中标点，就当句子处理。
/// 中文没有空格，所以单字/双字词按长度上限兜住。
pub fn is_single_word(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 40 {
        return false;
    }
    if trimmed.chars().any(char::is_whitespace) {
        return false;
    }
    // 句读与连接符出现即视为句子/短语
    const PUNCT: &[char] = &[
        '。', '，', '！', '？', '；', '：', '、', '…', '—', '「', '」', '『', '』', '（', '）',
        '《', '》', ',', '.', '!', '?', ';', ':', '"', '(', ')', '[', ']',
    ];
    !trimmed.chars().any(|ch| PUNCT.contains(&ch))
}

// ---------------------------------------------------------------- 缓存

const CACHE_CAPACITY: usize = 256;

struct CacheEntry {
    result: TranslateResult,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

struct Cache {
    map: HashMap<String, CacheEntry>,
    order: VecDeque<String>,
}

impl Cache {
    fn get(&mut self, key: &str) -> Option<TranslateResult> {
        let entry = self.map.get(key)?;
        let mut result = entry.result.clone();
        result.provider = "cache".to_string();
        result.cached = true;
        result.elapsed_ms = 0.0;
        Some(result)
    }

    fn put(&mut self, key: String, result: TranslateResult) {
        if self.map.contains_key(&key) {
            return;
        }
        while self.map.len() >= CACHE_CAPACITY {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            } else {
                break;
            }
        }
        self.map.insert(key.clone(), CacheEntry { result });
        self.order.push_back(key);
    }
}

fn cache_key(text: &str, target: Lang) -> String {
    format!("{:?}:{}", target, text.trim())
}

fn with_cache<T>(f: impl FnOnce(&mut Cache) -> T) -> T {
    let mut guard = CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache = guard.get_or_insert_with(|| Cache {
        map: HashMap::new(),
        order: VecDeque::new(),
    });
    f(cache)
}

/// 清空缓存（设置里换了 provider / 目标语言时调用，避免拿到旧结果）。
pub fn clear_cache() {
    with_cache(|cache| {
        cache.map.clear();
        cache.order.clear();
    });
}

// ---------------------------------------------------------------- 路由

/// 翻译一段文本。命中缓存直接返回，否则按 provider 走网络。
pub async fn translate(text: &str, config: &TranslateConfig) -> Result<TranslateResult, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("没有要翻译的内容".to_string());
    }

    let key = cache_key(text, config.target);
    if let Some(hit) = with_cache(|cache| cache.get(&key)) {
        crate::core::logs::line(format!(
            "translate cache hit chars={}",
            text.chars().count()
        ));
        return Ok(hit);
    }

    let started = Instant::now();
    // 中文原文 + 译成中文 = 原样吐回来，纯属浪费一次往返。
    // 读中文文档时用户想看的显然是英文，所以这里直接反向。
    let mut effective = config.clone();
    effective.target = effective_target(text, config);

    let (translated, provider_name) = call_provider(text, &effective).await?;

    let result = TranslateResult {
        text: translated,
        provider: provider_name.to_string(),
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        cached: false,
    };
    with_cache(|cache| cache.put(key, result.clone()));
    crate::core::logs::line(format!(
        "translate ok provider={} chars={} elapsed={:.0}ms",
        result.provider,
        text.chars().count(),
        result.elapsed_ms
    ));
    Ok(result)
}

/// 决定实际的目标语言：中文原文 + 译成中文 = 原样返回，没有意义，直接反向成英文。
fn effective_target(text: &str, config: &TranslateConfig) -> Lang {
    let has_cjk = text.chars().any(crate::core::segment::is_cjk);
    if config.target == Lang::Zh && has_cjk {
        Lang::En
    } else {
        config.target
    }
}

/// 调一次 provider；失败重试一次。
///
/// 免密端点偶发超时/限流，一次失败就报错太脆。重试前等 400ms，
/// 但**不重试配置类错误**（没填 Key 之类重试也没用）。
async fn call_provider(
    text: &str,
    config: &TranslateConfig,
) -> Result<(String, &'static str), String> {
    let mut last_error = String::new();
    for round in 0..2 {
        if round > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }

        let result = match config.provider {
            ProviderKind::Google => google::translate(text, config.target)
                .await
                .map(|value| (value, "google")),
            ProviderKind::OpenAi => openai::translate(text, config)
                .await
                .map(|value| (value, "openai")),
        };

        match result {
            Ok(pair) => {
                if round > 0 {
                    crate::core::logs::line("translate retry ok");
                }
                return Ok(pair);
            }
            Err(error) => {
                last_error = error;
                // 配置类错误重试无意义
                if last_error.contains("API Key")
                    || last_error.contains("请求模型接口失败：builder")
                {
                    break;
                }
                crate::core::logs::line(format!(
                    "translate attempt {} failed: {}",
                    round + 1,
                    truncate(&last_error, 120)
                ));
            }
        }
    }

    Err(last_error)
}

/// 预热翻译链路：把 DNS 解析 + TLS 握手的成本从「用户第一次翻译」挪到启动后。
///
/// 实测首次翻译 4.3 s、第二次 1.0 s，差的 3 s 全是握手。启动后后台打一次，
/// 用户真正用时就走热连接。
///
/// 只对免密 provider 做：OpenAI 兼容接口按 token 计费，替用户发一次无意义请求不合适。
/// **直接调 provider、不走 `translate()`**，避免把预热文本写进缓存。
pub async fn warm_up(config: &TranslateConfig) {
    if config.provider != ProviderKind::Google {
        crate::core::logs::line("translate warmup skipped (non-google provider)");
        return;
    }

    let started = Instant::now();
    match google::translate("warm up", Lang::En).await {
        Ok(_) => crate::core::logs::line(format!(
            "translate warmup ok {:.0}ms",
            started.elapsed().as_secs_f64() * 1000.0
        )),
        Err(error) => crate::core::logs::line(format!(
            "translate warmup failed: {}",
            truncate(&error, 100)
        )),
    }
}

// ---------------------------------------------------------------- Google gtx

pub(crate) mod google {
    use super::{Lang, truncate};

    /// `translate_a/single` 的响应形如：
    /// `[[["译文","原文",null,null,10],["第二段","..."]],null,"en",...]`
    ///
    /// 第一层 `[0]` 是分段数组，每段 `[0]` 是译文；拼起来才是完整译文。
    pub(crate) fn parse_response(body: &str) -> Result<String, String> {
        let value: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("响应不是合法 JSON：{e}"))?;
        let segments = value
            .get(0)
            .and_then(|first| first.as_array())
            .ok_or_else(|| "响应结构不符合预期".to_string())?;

        let mut out = String::new();
        for segment in segments {
            if let Some(piece) = segment.get(0).and_then(|piece| piece.as_str()) {
                out.push_str(piece);
            }
        }

        if out.trim().is_empty() {
            return Err("接口返回了空译文".to_string());
        }
        Ok(out.trim().to_string())
    }

    pub(crate) async fn translate(text: &str, target: Lang) -> Result<String, String> {
        let url = format!(
            "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={}&dt=t",
            target.google_code()
        );

        let client = super::http_client()?;
        // 用 GET 而不是 POST form：实测同一句话 GET 明显更快，且接口两种都接受。
        let response = client
            .get(&url)
            .query(&[("q", text)])
            .send()
            .await
            .map_err(|e| format!("请求翻译接口失败：{e}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("读取响应失败：{e}"))?;

        if !status.is_success() {
            return Err(format!("翻译接口返回 {status}：{}", truncate(&body, 120)));
        }
        parse_response(&body)
    }
}

// ---------------------------------------------------------------- OpenAI 兼容

pub(crate) mod openai {
    use serde_json::json;

    use super::{TranslateConfig, truncate};

    /// 从 chat/completions 的响应里取正文。抽成纯函数便于单测。
    pub(crate) fn parse_response(body: &str) -> Result<String, String> {
        let value: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("响应不是合法 JSON：{e}"))?;

        if let Some(message) = value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(|content| content.as_str())
        {
            let trimmed = message.trim();
            if trimmed.is_empty() {
                return Err("模型返回了空内容".to_string());
            }
            return Ok(trimmed.to_string());
        }

        // 有些兼容实现把错误放在 error.message
        if let Some(error) = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|message| message.as_str())
        {
            return Err(format!("模型返回错误：{error}"));
        }

        Err("响应里找不到 choices[0].message.content".to_string())
    }

    pub(super) async fn translate(text: &str, config: &TranslateConfig) -> Result<String, String> {
        if config.api_key.trim().is_empty() {
            return Err("OpenAI 兼容接口需要先在设置里填 API Key".to_string());
        }

        let base = config.base_url.trim_end_matches('/');
        let url = format!("{base}/chat/completions");
        let target = config.target;

        let body = json!({
            "model": config.model,
            "temperature": 0.2,
            "messages": [
                {
                    "role": "system",
                    "content": format!(
                        "你是翻译引擎。把用户给的文本翻译成{}。只输出译文本身，不要解释、不要加引号、不要保留原文。",
                        target.prompt_name()
                    ),
                },
                { "role": "user", "content": text }
            ]
        });

        let client = super::http_client()?;
        let response = client
            .post(&url)
            .bearer_auth(config.api_key.trim())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("请求模型接口失败：{e}"))?;

        let status = response.status();
        let raw = response
            .text()
            .await
            .map_err(|e| format!("读取响应失败：{e}"))?;

        if !status.is_success() {
            return Err(format!("模型接口返回 {status}：{}", truncate(&raw, 160)));
        }
        parse_response(&raw)
    }
}

// ---------------------------------------------------------------- 公共工具

fn truncate(text: &str, max: usize) -> String {
    let mut clipped: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        clipped.push('…');
    }
    clipped
}

/// 每次翻译建一个 client 太浪费；全局复用一个。
fn http_client() -> Result<&'static reqwest::Client, String> {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let client = reqwest::Client::builder()
        .user_agent("Lemma/0.1 (+https://github.com/veyastra2362/Lemma)")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))?;
    let _ = CLIENT.set(client);
    CLIENT
        .get()
        .ok_or_else(|| "HTTP 客户端初始化失败".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_words_go_to_the_dictionary() {
        assert!(is_single_word("threshold"));
        assert!(is_single_word("门槛"));
        assert!(is_single_word("don't"));
        assert!(is_single_word("e-mail"));
    }

    #[test]
    fn sentences_go_to_translation() {
        assert!(!is_single_word("hello world"));
        assert!(!is_single_word("当前进度：多服务商"));
        assert!(!is_single_word("The threshold value matters."));
        assert!(!is_single_word("第一句。"));
        // 超长单串也不再当词处理
        assert!(!is_single_word(&"a".repeat(41)));
    }

    #[test]
    fn empty_text_is_not_a_word() {
        assert!(!is_single_word(""));
        assert!(!is_single_word("   "));
    }

    /// 中文原文 + 译成中文只会原样返回，应自动反向成英文。
    #[test]
    fn chinese_source_flips_target_to_english() {
        let mut config = TranslateConfig {
            provider: ProviderKind::Google,
            target: Lang::Zh,
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
        };
        assert_eq!(effective_target("这是一段中文。", &config), Lang::En);
        // 英文原文不受影响
        assert_eq!(effective_target("English text.", &config), Lang::Zh);
        // 用户显式选英文时，中文原文也保持英文
        config.target = Lang::En;
        assert_eq!(effective_target("这是一段中文。", &config), Lang::En);
    }

    #[test]
    fn parses_google_response_segments() {
        let body = r#"[[["临界点","threshold",null,null,10]],null,"en",null,null,null,null,[]]"#;
        assert_eq!(google::parse_response(body).unwrap(), "临界点");
    }

    #[test]
    fn concatenates_google_segments() {
        let body = r#"[[["第一句。","One."],[" 第二句。"," Two."]],null,"en"]"#;
        assert_eq!(google::parse_response(body).unwrap(), "第一句。 第二句。");
    }

    #[test]
    fn rejects_bad_google_payloads() {
        assert!(google::parse_response("not json").is_err());
        assert!(google::parse_response(r#"{"a":1}"#).is_err());
        assert!(google::parse_response(r#"[[["",""]]]"#).is_err());
    }

    #[test]
    fn parses_openai_response() {
        let body = r#"{"choices":[{"message":{"content":"  译文  "}}]}"#;
        assert_eq!(openai::parse_response(body).unwrap(), "译文");
    }

    #[test]
    fn surfaces_openai_error_message() {
        let body = r#"{"error":{"message":"invalid api key"}}"#;
        let error = openai::parse_response(body).unwrap_err();
        assert!(error.contains("invalid api key"), "实际：{error}");
    }

    #[test]
    fn cache_returns_marked_hits() {
        let key = "test-key-hit";
        with_cache(|cache| {
            cache.put(
                key.to_string(),
                TranslateResult {
                    text: "缓存译文".to_string(),
                    provider: "google".to_string(),
                    elapsed_ms: 700.0,
                    cached: false,
                },
            );
        });

        let hit = with_cache(|cache| cache.get(key)).expect("应命中缓存");
        assert!(hit.cached);
        assert_eq!(hit.provider, "cache");
        assert_eq!(hit.elapsed_ms, 0.0);
        assert_eq!(hit.text, "缓存译文");
    }

    #[test]
    fn cache_evicts_beyond_capacity() {
        let capacity = CACHE_CAPACITY;
        for index in 0..(capacity + 10) {
            with_cache(|cache| {
                cache.put(
                    format!("evict-{index}"),
                    TranslateResult {
                        text: "x".to_string(),
                        provider: "google".to_string(),
                        elapsed_ms: 1.0,
                        cached: false,
                    },
                );
            });
        }
        with_cache(|cache| {
            assert!(cache.map.len() <= capacity);
            // 最早的键应该已经被淘汰
            assert!(cache.get("evict-0").is_none());
        });
    }

    #[test]
    fn cache_key_separates_targets() {
        assert_ne!(cache_key("hello", Lang::Zh), cache_key("hello", Lang::En));
    }

    #[test]
    fn prompt_names_are_human_readable() {
        assert_eq!(Lang::Zh.prompt_name(), "简体中文");
        assert_eq!(Lang::En.prompt_name(), "English");
    }
}
