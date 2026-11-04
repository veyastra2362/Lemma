//! 翻译缓存的落盘。
//!
//! 为什么要有：译文缓存原来只在内存里（LRU 256），**重启就没了**——
//! 而实际阅读场景里，同一段话反复看（返回上一段、重开同一篇文章）非常常见。
//!
//! 为什么用 JSON Lines 而不是 redb：
//! - redb 是**独占锁**：词库已经因此让 CLI 与 GUI 打过架，再加一个锁文件只会更糟；
//! - 追加写一行就是一次 `write`，不需要事务、不需要 schema 迁移；
//! - 出问题能直接 `head` 看内容，坏了删掉即等于清缓存。
//!
//! 代价是要自己做容量控制：文件超过上限时按"保留最近的"重写一遍。

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// 内存里最多保留多少条（与旧的 LRU 容量一致）。
pub const MAX_ENTRIES: usize = 256;
/// 文件超过这个大小时重写（保留最近的 `MAX_ENTRIES` 条）。
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// 一行缓存。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    /// `目标语言:原文`
    key: String,
    text: String,
    provider: String,
}

static STORE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// 绑定缓存文件位置（每个进程一次）。
pub fn attach(path: PathBuf) {
    if let Ok(mut slot) = STORE.lock() {
        *slot = Some(path);
    }
}

fn path() -> Option<PathBuf> {
    STORE.lock().ok().and_then(|slot| slot.clone())
}

/// 读出最近的若干条，按"最旧 → 最新"排列。
///
/// 容忍坏行：解析失败就跳过（文件是追加写的，进程被杀时最后一行可能只写了一半）。
pub fn load(path: &Path) -> Vec<(String, String, String)> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut records: VecDeque<(String, String, String)> = VecDeque::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Record>(line) else {
            continue;
        };
        records.push_back((record.key, record.text, record.provider));
        if records.len() > MAX_ENTRIES {
            records.pop_front();
        }
    }
    records.into_iter().collect()
}

/// 追加一条（用全局路径）。失败只记日志——缓存写不进去不该影响翻译本身。
pub fn append(key: &str, text: &str, provider: &str) {
    if let Some(path) = path() {
        append_to(&path, key, text, provider);
    }
}

/// 追加一条到指定文件。测试直接用它，避免多个测试抢同一个全局路径。
pub fn append_to(path: &Path, key: &str, text: &str, provider: &str) {
    let record = Record {
        key: key.to_string(),
        text: text.to_string(),
        provider: provider.to_string(),
    };
    let Ok(line) = serde_json::to_string(&record) else {
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| writeln!(file, "{line}"));
    if let Err(error) = written {
        crate::core::logs::line(format!("translate cache write failed: {error}"));
        return;
    }

    // 文件太大就重写一遍，只留最近的 MAX_ENTRIES 条
    if std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) > MAX_FILE_BYTES {
        rewrite(path);
    }
}

fn rewrite(path: &Path) {
    let kept = load(path);
    let mut content = String::new();
    for (key, text, provider) in kept {
        if let Ok(line) = serde_json::to_string(&Record {
            key,
            text,
            provider,
        }) {
            content.push_str(&line);
            content.push('\n');
        }
    }
    if std::fs::write(path, content).is_ok() {
        crate::core::logs::line("translate cache compacted");
    }
}

/// 删掉缓存文件（设置里换 provider / 目标语言时用）。
pub fn clear() {
    if let Some(path) = path() {
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("lemma-cache-test");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(name)
    }

    #[test]
    fn append_then_load_round_trips() {
        let path = temp_path("round-trip.jsonl");
        let _ = std::fs::remove_file(&path);

        append_to(&path, "Zh:hello", "你好", "google");
        append_to(&path, "En:hello", "hello", "google");

        let loaded = load(&path);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].0, "Zh:hello");
        assert_eq!(loaded[0].1, "你好");
        assert_eq!(loaded[1].0, "En:hello");
    }

    #[test]
    fn broken_lines_are_skipped() {
        let path = temp_path("broken.jsonl");
        let _ = std::fs::write(
            &path,
            "{\"key\":\"A:ok\",\"text\":\"好\",\"provider\":\"google\"}\n{\"key\":\"B:trunc\n",
        );
        let loaded = load(&path);
        assert_eq!(loaded.len(), 1, "坏行应被跳过而不是报错");
        assert_eq!(loaded[0].0, "A:ok");
    }

    #[test]
    fn missing_file_is_empty_not_error() {
        let loaded = load(&temp_path("does-not-exist.jsonl"));
        assert!(loaded.is_empty());
    }

    #[test]
    fn only_recent_entries_are_kept() {
        let path = temp_path("many.jsonl");
        let _ = std::fs::remove_file(&path);
        for index in 0..(MAX_ENTRIES + 40) {
            append_to(
                &path,
                &format!("Zh:{index}"),
                &format!("译文{index}"),
                "google",
            );
        }
        let loaded = load(&path);
        assert_eq!(loaded.len(), MAX_ENTRIES);
        // 保留的应该是最新的那批
        assert_eq!(loaded.last().map(|item| item.0.as_str()), Some("Zh:295"));
    }
}
