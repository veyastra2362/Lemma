//! 离线词库：redb 存储 + 归一化检索。
//!
//! 设计要点：
//! - **键是归一化后的词形**（小写 + NFKC），这样 `Threshold` / `threshold` 命中同一条；
//! - 值是 `postcard` 序列化的 [`Entry`]，比 JSON 小一半以上；
//! - 检索走三级回退：精确 → 去所有格/复数 → 词形变化（`exchange` 反查）；
//! - 前缀查询用于候选提示。
//!
//! 这一版**不把词库全量读进内存**：redb 是 mmap + B-tree，热页自动留在页缓存里，
//! 冷门词按需换入。这直接决定了常驻内存能不能压住。

use std::path::{Path, PathBuf};

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// 词条。字段对齐 ECDICT 的列。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Entry {
    pub word: String,
    pub phonetic: String,
    /// 中文释义，多义项用 `\n` 分隔。
    pub translation: String,
    /// 英文释义。
    pub definition: String,
    /// 词性占比，形如 `n:46/v:54`。
    pub pos: String,
    /// 考试标签，形如 `zk gk cet4`。
    pub tag: String,
    /// 词形变化，形如 `p:did/d:done/i:doing/3:does`。
    pub exchange: String,
    pub bnc: u32,
    pub frq: u32,
}

const TERMS: TableDefinition<&str, &[u8]> = TableDefinition::new("terms");
const META: TableDefinition<&str, &str> = TableDefinition::new("meta");

/// 归一化：小写 + NFKC（全角转半角等）+ 去首尾空白。
pub fn normalize(word: &str) -> String {
    word.trim().nfkc().collect::<String>().to_lowercase()
}

/// 词形回退候选：所有格、复数、过去式之类的廉价规则。
///
/// 真正的词形还原要靠 ECDICT 的 `exchange` 字段反查，这里只是先手便宜的一层。
fn fallback_keys(word: &str) -> Vec<String> {
    let mut keys = Vec::new();

    if let Some(stripped) = word.strip_suffix("'s").or_else(|| word.strip_suffix("’s")) {
        keys.push(stripped.to_string());
    }
    if let Some(stripped) = word.strip_suffix("s") {
        keys.push(stripped.to_string());
    }
    if let Some(stripped) = word.strip_suffix("es") {
        keys.push(stripped.to_string());
    }
    if let Some(stripped) = word.strip_suffix("ed") {
        keys.push(stripped.to_string());
    }
    if let Some(stripped) = word.strip_suffix("ing") {
        keys.push(stripped.to_string());
    }
    keys.retain(|key| !key.is_empty());
    keys
}

pub struct DictStore {
    db: Database,
    path: PathBuf,
}

impl DictStore {
    /// 打开（不存在则创建）词库。
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("建目录失败：{e}"))?;
        }
        let db = Database::create(path).map_err(|e| format!("打开词库失败：{e}"))?;
        Ok(Self {
            db,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 词条总数。
    pub fn len(&self) -> u64 {
        let Ok(txn) = self.db.begin_read() else {
            return 0;
        };
        let Ok(table) = txn.open_table(META) else {
            return 0;
        };
        table
            .get("count")
            .ok()
            .flatten()
            .and_then(|value| value.value().parse().ok())
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 批量写入。`entries` 是 `(归一化键, 词条)`。
    ///
    /// 分块提交：一次写 77 万条会让事务日志非常大，内存和崩溃恢复都难受。
    pub fn insert_batch(
        &self,
        entries: impl Iterator<Item = (String, Entry)>,
        chunk: usize,
    ) -> Result<u64, String> {
        let mut written = 0u64;
        let mut buffer: Vec<(String, Entry)> = Vec::with_capacity(chunk);

        for (key, entry) in entries {
            buffer.push((key, entry));
            if buffer.len() >= chunk {
                written += self.flush(&buffer)?;
                buffer.clear();
            }
        }
        if !buffer.is_empty() {
            written += self.flush(&buffer)?;
        }

        let txn = self.db.begin_write().map_err(|e| e.to_string())?;
        {
            let mut meta = txn.open_table(META).map_err(|e| e.to_string())?;
            meta.insert("count", written.to_string().as_str())
                .map_err(|e| e.to_string())?;
        }
        txn.commit().map_err(|e| e.to_string())?;

        Ok(written)
    }

    fn flush(&self, buffer: &[(String, Entry)]) -> Result<u64, String> {
        let txn = self.db.begin_write().map_err(|e| e.to_string())?;
        {
            let mut table = txn.open_table(TERMS).map_err(|e| e.to_string())?;
            for (key, entry) in buffer {
                let bytes = postcard::to_allocvec(entry).map_err(|e| e.to_string())?;
                table
                    .insert(key.as_str(), bytes.as_slice())
                    .map_err(|e| e.to_string())?;
            }
        }
        txn.commit().map_err(|e| e.to_string())?;
        Ok(buffer.len() as u64)
    }

    /// 精确（含归一化）查询。
    pub fn get(&self, word: &str) -> Result<Option<Entry>, String> {
        let key = normalize(word);
        if key.is_empty() {
            return Ok(None);
        }

        let txn = self.db.begin_read().map_err(|e| e.to_string())?;
        let Ok(table) = txn.open_table(TERMS) else {
            return Ok(None);
        };

        if let Some(entry) = read_entry(&table, &key)? {
            return Ok(Some(entry));
        }

        // 回退：所有格 / 复数 / 时态后缀。
        //
        // TODO(M2.1)：再加一层「拿 exchange 反查原形」（`went` → `go`）。
        // 那需要为 exchange 建倒排表，等 Deck 下探做完再补，避免现在把导入耗时翻倍。
        for candidate in fallback_keys(&key) {
            if let Some(entry) = read_entry(&table, &candidate)? {
                return Ok(Some(entry));
            }
        }

        Ok(None)
    }

    /// 前缀查询，用于候选提示。最多返回 `limit` 条。
    pub fn prefix(&self, prefix: &str, limit: usize) -> Result<Vec<String>, String> {
        let key = normalize(prefix);
        if key.is_empty() {
            return Ok(Vec::new());
        }

        let txn = self.db.begin_read().map_err(|e| e.to_string())?;
        let Ok(table) = txn.open_table(TERMS) else {
            return Ok(Vec::new());
        };

        let mut words = Vec::new();
        let range = table.range(key.as_str()..).map_err(|e| e.to_string())?;
        for item in range {
            let (k, _) = item.map_err(|e| e.to_string())?;
            let word = k.value().to_string();
            if !word.starts_with(&key) {
                break;
            }
            words.push(word);
            if words.len() >= limit {
                break;
            }
        }
        Ok(words)
    }
}

fn read_entry(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    key: &str,
) -> Result<Option<Entry>, String> {
    let Some(value) = table.get(key).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let entry: Entry = postcard::from_bytes(value.value()).map_err(|e| e.to_string())?;
    Ok(Some(entry))
}

/// 进程级词库实例。
///
/// GUI 启动时打开一次，此后取词链路直接复用——每开一次 redb 都要重新 mmap，
/// 放在取词热路径上会明显拖慢。
static GLOBAL: std::sync::OnceLock<std::sync::Arc<DictStore>> = std::sync::OnceLock::new();

pub fn install(store: std::sync::Arc<DictStore>) {
    let _ = GLOBAL.set(store);
}

pub fn global() -> Option<&'static std::sync::Arc<DictStore>> {
    GLOBAL.get()
}

/// 用全局词库查词；词库没装就返回 None。
pub fn lookup_global(word: &str) -> Option<Entry> {
    global().and_then(|store| store.get(word).ok().flatten())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str) -> DictStore {
        let dir = std::env::temp_dir().join(format!("trans-dict-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        DictStore::open(&dir.join("dict.redb")).expect("建库失败")
    }

    fn entry(word: &str, translation: &str) -> Entry {
        Entry {
            word: word.to_string(),
            translation: translation.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn normalizes_case_and_width() {
        assert_eq!(normalize("  Threshold "), "threshold");
        // 全角字母 → 半角
        assert_eq!(normalize("ＴＨＲＥＳＨＯＬＤ"), "threshold");
    }

    #[test]
    fn exact_lookup_is_case_insensitive() {
        let store = temp_db("exact");
        store
            .insert_batch(
                std::iter::once(("threshold".to_string(), entry("threshold", "n. 门槛"))),
                10,
            )
            .unwrap();
        assert_eq!(store.len(), 1);

        let hit = store.get("THRESHOLD").unwrap().expect("应该命中");
        assert_eq!(hit.translation, "n. 门槛");
        assert!(store.get("mis sing").unwrap().is_none());
    }

    #[test]
    fn falls_back_to_plural_and_possessive() {
        let store = temp_db("plural");
        store
            .insert_batch(
                std::iter::once(("dog".to_string(), entry("dog", "n. 狗"))),
                10,
            )
            .unwrap();

        assert!(store.get("dogs").unwrap().is_some());
        assert!(store.get("dog's").unwrap().is_some());
    }

    #[test]
    fn global_lookup_sees_the_installed_store() {
        let store = temp_db("global");
        store
            .insert_batch(
                std::iter::once(("threshold".to_string(), entry("threshold", "n. 门槛"))),
                10,
            )
            .unwrap();
        super::install(std::sync::Arc::new(store));

        let hit = super::lookup_global("Threshold").expect("全局词库应命中");
        assert_eq!(hit.translation, "n. 门槛");
        assert!(super::lookup_global("nothing-here").is_none());
    }

    #[test]
    fn prefix_suggestions_are_ordered_and_bounded() {
        let store = temp_db("prefix");
        let entries: Vec<(String, Entry)> = [
            ("dictate", entry("dictate", "v. 口述")),
            ("dictionary", entry("dictionary", "n. 词典")),
            ("diction", entry("diction", "n. 措辞")),
        ]
        .into_iter()
        .map(|(key, entry)| (key.to_string(), entry))
        .collect();
        store.insert_batch(entries.into_iter(), 2).unwrap();

        let words = store.prefix("dict", 10).unwrap();
        assert_eq!(words, vec!["dictate", "diction", "dictionary"]);
        assert_eq!(store.prefix("dict", 2).unwrap().len(), 2);
        assert!(store.prefix("", 5).unwrap().is_empty());
    }
}
