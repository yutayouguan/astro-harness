//! 记忆引用追踪：追踪每条记忆的使用频率，为 Dreaming 排序提供量化信号。
//!
//! 不修改 MEMORY.md 的存储格式，使用 companion 文件 `memory/usage.json` 存储引用计数。
//! 条目以内容前 64 字符的 SHA256 前 8 位作为稳定 ID。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const USAGE_FILE: &str = "memory/usage.json";
const ID_PREFIX_CHARS: usize = 64;
const ID_HASH_LEN: usize = 8;

/// 单条记忆的引用统计。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryCitation {
    pub usage_count: u32,
    pub last_used: Option<String>,
    pub created_at: Option<String>,
}

/// 引用追踪存储（companion 文件）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CitationStore {
    pub entries: HashMap<String, MemoryCitation>,
}

pub fn entry_id(content: &str) -> String {
    let prefix: String = content.trim().chars().take(ID_PREFIX_CHARS).collect();
    let digest = Sha256::digest(prefix.as_bytes());
    format!("{:x}", digest).chars().take(ID_HASH_LEN).collect()
}

fn usage_path(workspace: &Path) -> PathBuf {
    workspace.join(USAGE_FILE)
}

pub fn load_citations(workspace: &Path) -> CitationStore {
    let path = usage_path(workspace);
    let Ok(raw) = fs::read_to_string(&path) else {
        return CitationStore::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_citations(workspace: &Path, store: &CitationStore) -> anyhow::Result<()> {
    let path = usage_path(workspace);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(store)?;
    fs::write(&tmp, raw)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// 记录一批引用（assistant 消息中出现的 `[mem:xxx]` 标签）。
pub fn record_citations(workspace: &Path, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let mut store = load_citations(workspace);
    let now = chrono::Utc::now().to_rfc3339();
    for id in ids {
        let entry = store.entries.entry(id.clone()).or_default();
        entry.usage_count += 1;
        entry.last_used = Some(now.clone());
    }
    let _ = save_citations(workspace, &store);
}

/// 为一组记忆条目生成带引用标签的 prompt 片段。
pub fn annotate_entries(entries: &[String]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|e| {
            let id = entry_id(e);
            (id, e.clone())
        })
        .collect()
}

/// 渲染带 `[mem:xxx]` 标签的记忆内容，用于 system prompt 注入。
pub fn render_with_citations(entries: &[String]) -> String {
    entries
        .iter()
        .map(|e| {
            let id = entry_id(e);
            format!("[mem:{id}] {e}")
        })
        .collect::<Vec<_>>()
        .join("\n§\n")
}

/// 从 assistant 消息中提取所有 `[mem:xxx]` 引用。
pub fn extract_citations(text: &str) -> Vec<String> {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\[mem:([a-f0-9]{6,16})\]").unwrap());
    RE.captures_iter(text)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// 按引用频率排序条目（高频在前），用于 Dreaming 输入。
pub fn sort_by_usage(entries: &[String], workspace: &Path) -> Vec<String> {
    let store = load_citations(workspace);
    let mut with_score: Vec<(u32, &String)> = entries
        .iter()
        .map(|e| {
            let id = entry_id(e);
            let count = store.entries.get(&id).map(|c| c.usage_count).unwrap_or(0);
            (count, e)
        })
        .collect();
    with_score.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    with_score.into_iter().map(|(_, e)| e.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_id_is_stable() {
        let id1 = entry_id("用户喜欢用 Rust 编程");
        let id2 = entry_id("用户喜欢用 Rust 编程");
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), ID_HASH_LEN);
    }

    #[test]
    fn entry_id_differs_for_different_content() {
        let id1 = entry_id("用户喜欢 Rust");
        let id2 = entry_id("用户喜欢 Python");
        assert_ne!(id1, id2);
    }

    #[test]
    fn extract_citations_finds_all() {
        let text = "根据 [mem:abc12345] 和 [mem:def67890] 的记忆...";
        let ids = extract_citations(text);
        assert_eq!(ids, vec!["abc12345", "def67890"]);
    }

    #[test]
    fn extract_citations_empty_on_no_match() {
        assert!(extract_citations("no citations here").is_empty());
    }

    #[test]
    fn render_with_citations_format() {
        let entries = vec!["用户是 Rust 开发者".to_string(), "项目用 Tauri".to_string()];
        let rendered = render_with_citations(&entries);
        assert!(rendered.contains("[mem:"));
        assert!(rendered.contains("§"));
    }

    #[test]
    fn sort_by_usage_orders_by_count() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        let mut store = CitationStore::default();
        let e1 = "rarely used";
        let e2 = "frequently used";
        store.entries.insert(
            entry_id(e1),
            MemoryCitation {
                usage_count: 1,
                ..Default::default()
            },
        );
        store.entries.insert(
            entry_id(e2),
            MemoryCitation {
                usage_count: 10,
                ..Default::default()
            },
        );
        save_citations(ws, &store).unwrap();

        let entries = vec![e1.to_string(), e2.to_string()];
        let sorted = sort_by_usage(&entries, ws);
        assert_eq!(sorted[0], "frequently used");
        assert_eq!(sorted[1], "rarely used");
    }
}
