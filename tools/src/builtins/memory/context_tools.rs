//! Agentic 上下文工具：`search_context` / `pin_context`。
//!
//! - `search_context`：按需检索 session FTS、MEMORY/USER、Knowledge，避免每轮塞满 Dynamic
//! - `pin_context`：将会话内固定片段写入 `{workspace}/pinned-context.json`，供后续 system prompt 注入

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use common::truncate_chars;

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

const PINNED_FILE: &str = "pinned-context.json";
const MAX_PIN_CHARS: usize = 8 * 1024;
const MAX_PINS: usize = 20;

/// `search_context` 检索范围。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchScope {
    /// 会话历史 FTS。
    Session,
    /// MEMORY.md + USER.md 子串。
    Memory,
    /// Knowledge Content DB。
    Knowledge,
    /// 以上全部（默认）。
    All,
}

impl Default for SearchScope {
    fn default() -> Self {
        Self::All
    }
}

/// `search_context` 参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SearchContextArgs {
    /// 检索关键词。
    pub query: String,
    /// 范围，默认 `all`。
    #[serde(default)]
    pub scope: SearchScope,
    /// 每源返回上限，默认 5，最大 10。
    #[serde(default)]
    pub limit: Option<u32>,
}

/// `pin_context` 动作。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PinAction {
    /// 追加一条固定片段。
    Pin,
    /// 列出当前固定片段。
    List,
    /// 按 id 移除。
    Unpin,
    /// 清空全部。
    Clear,
}

/// `pin_context` 参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PinContextArgs {
    pub action: PinAction,
    /// `pin` 时的正文。
    #[serde(default)]
    pub content: Option<String>,
    /// `unpin` 时的片段 id。
    #[serde(default)]
    pub id: Option<String>,
    /// 可选短标题（写入列表展示）。
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PinnedEntry {
    id: String,
    title: String,
    content: String,
    created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PinnedStore {
    entries: Vec<PinnedEntry>,
}

/// 向注册表登记 agentic 上下文工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "search_context".to_string(),
        toolset: "search_context".to_string(),
        description: "On-demand context search across session history (FTS), MEMORY/USER, and knowledge DB. Prefer this over stuffing every recall into the system prompt. scope=session|memory|knowledge|all."
            .to_string(),
        schema: schema_for_args::<SearchContextArgs>(),
        check_fn: None,
        icon: "search",
        ..ToolEntry::lifecycle_defaults()
    });
    registry.register(ToolEntry {
        name: "pin_context".to_string(),
        toolset: "pin_context".to_string(),
        description: "Pin/unpin session-scoped context snippets that stay in the system prompt until cleared. action=pin|list|unpin|clear. Use after search_context to keep key facts loaded."
            .to_string(),
        schema: schema_for_args::<PinContextArgs>(),
        check_fn: None,
        icon: "pin",
        ..ToolEntry::lifecycle_defaults().exclusive()
    });
}

/// 分发 `search_context` / `pin_context`。
pub fn dispatch(ctx: &ToolContext<'_>, name: &str, args: &serde_json::Value) -> anyhow::Result<String> {
    match name {
        "search_context" => dispatch_search(ctx, args),
        "pin_context" => dispatch_pin(ctx, args),
        other => anyhow::bail!("未知上下文工具: {other}"),
    }
}

fn dispatch_search(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: SearchContextArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("search_context 参数无效: {e}"))?;
    let query = parsed.query.trim();
    if query.is_empty() {
        anyhow::bail!("search_context 需要非空 query");
    }
    let limit = parsed.limit.unwrap_or(5).clamp(1, 10) as usize;
    let mut sections = Vec::new();

    let want_session = matches!(parsed.scope, SearchScope::Session | SearchScope::All);
    let want_memory = matches!(parsed.scope, SearchScope::Memory | SearchScope::All);
    let want_knowledge = matches!(parsed.scope, SearchScope::Knowledge | SearchScope::All);

    if want_session {
        let hits = ctx
            .sessions
            .search_messages(query, None, None, limit as i64)
            .unwrap_or_default();
        if hits.is_empty() {
            sections.push("## session\n（无匹配）".to_string());
        } else {
            let body = hits
                .iter()
                .take(limit)
                .map(|h| {
                    let preview = truncate_chars(&h.snippet, 400);
                    format!(
                        "- [{}] session={} id={}\n  {}",
                        h.role, h.session_id, h.id, preview
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("## session\n{body}"));
        }
    }

    if want_memory {
        let mem_hits = search_entries(ctx.memory.memory.live_entries(), query, limit);
        let user_hits = search_entries(ctx.memory.user.live_entries(), query, limit);
        let mut lines = Vec::new();
        for e in &mem_hits {
            lines.push(format!("- [MEMORY] {}", truncate_chars(e, 400)));
        }
        for e in &user_hits {
            lines.push(format!("- [USER] {}", truncate_chars(e, 400)));
        }
        if lines.is_empty() {
            sections.push("## memory\n（无匹配）".to_string());
        } else {
            sections.push(format!("## memory\n{}", lines.join("\n")));
        }
    }

    if want_knowledge {
        match artifacts::KnowledgeDb::open_default() {
            Ok(db) => {
                let rows = db.search(query, limit).unwrap_or_default();
                if rows.is_empty() {
                    sections.push("## knowledge\n（无匹配）".to_string());
                } else {
                    let body = rows
                        .iter()
                        .map(|r| format!("- [{}] {} ({})", r.status, r.title, r.path))
                        .collect::<Vec<_>>()
                        .join("\n");
                    sections.push(format!("## knowledge\n{body}"));
                }
            }
            Err(_) => sections.push("## knowledge\n（库不可用）".to_string()),
        }
    }

    Ok(common::truncate_tool_result(
        &format!("# search_context: {query}\n\n{}", sections.join("\n\n")),
        common::MAX_TOOL_RESULT_BYTES,
    ))
}

fn dispatch_pin(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: PinContextArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("pin_context 参数无效: {e}"))?;
    let path = pinned_path(&ctx.workspace_dir);
    let mut store = load_pinned(&path)?;

    match parsed.action {
        PinAction::List => Ok(format_pinned_list(&store)),
        PinAction::Clear => {
            store.entries.clear();
            save_pinned(&path, &store)?;
            Ok("已清空全部固定上下文".into())
        }
        PinAction::Unpin => {
            let id = parsed
                .id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("unpin 需要 id"))?;
            let before = store.entries.len();
            store.entries.retain(|e| e.id != id);
            if store.entries.len() == before {
                anyhow::bail!("未找到固定片段: {id}");
            }
            save_pinned(&path, &store)?;
            Ok(format!("已取消固定: {id}"))
        }
        PinAction::Pin => {
            let content = parsed
                .content
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("pin 需要 content"))?;
            if content.chars().count() > MAX_PIN_CHARS {
                anyhow::bail!("content 超过 {MAX_PIN_CHARS} 字符上限");
            }
            if store.entries.len() >= MAX_PINS {
                anyhow::bail!("固定片段已达上限 {MAX_PINS}，请先 unpin 或 clear");
            }
            let title = parsed
                .title
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("pinned")
                .to_string();
            let id = uuid::Uuid::new_v4().to_string();
            let short_id: String = id.chars().take(8).collect();
            store.entries.push(PinnedEntry {
                id: short_id.clone(),
                title,
                content: content.to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
            });
            save_pinned(&path, &store)?;
            Ok(format!(
                "已固定上下文 id={short_id}（共 {} 条）；将在后续 system prompt 中注入",
                store.entries.len()
            ))
        }
    }
}

/// 供 AgentLoop 注入 system prompt 的固定上下文渲染（无条目返回空串）。
pub fn render_pinned_for_prompt(workspace_dir: &Path) -> String {
    let store = load_pinned(&pinned_path(workspace_dir)).unwrap_or_default();
    if store.entries.is_empty() {
        return String::new();
    }
    let body = store
        .entries
        .iter()
        .map(|e| format!("### {} ({})\n{}", e.title, e.id, e.content.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("# 固定上下文（pin_context）\n{body}")
}

fn pinned_path(workspace: &Path) -> PathBuf {
    workspace.join(PINNED_FILE)
}

fn load_pinned(path: &Path) -> anyhow::Result<PinnedStore> {
    if !path.exists() {
        return Ok(PinnedStore::default());
    }
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn save_pinned(path: &Path, store: &PinnedStore) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(store)?;
    std::fs::write(path, raw)?;
    Ok(())
}

fn format_pinned_list(store: &PinnedStore) -> String {
    if store.entries.is_empty() {
        return "当前无固定上下文".into();
    }
    let lines: Vec<String> = store
        .entries
        .iter()
        .map(|e| {
            format!(
                "- id={} title={} chars={} at={}",
                e.id,
                e.title,
                e.content.chars().count(),
                e.created_at
            )
        })
        .collect();
    format!("固定上下文 {} 条:\n{}", store.entries.len(), lines.join("\n"))
}

fn search_entries(entries: &[String], query: &str, limit: usize) -> Vec<String> {
    let q = query.to_lowercase();
    entries
        .iter()
        .filter(|e| e.to_lowercase().contains(&q))
        .take(limit)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pin_list_unpin_roundtrip() {
        let dir = tempdir().unwrap();
        let path = pinned_path(dir.path());
        let mut store = PinnedStore::default();
        store.entries.push(PinnedEntry {
            id: "abc12345".into(),
            title: "fact".into(),
            content: "important fact".into(),
            created_at: "t".into(),
        });
        save_pinned(&path, &store).unwrap();
        let loaded = load_pinned(&path).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        let rendered = render_pinned_for_prompt(dir.path());
        assert!(rendered.contains("important fact"));
        assert!(rendered.contains("abc12345"));
    }

    #[test]
    fn search_entries_case_insensitive() {
        let entries = vec!["Hello World".into(), "other".into()];
        let hits = search_entries(&entries, "hello", 5);
        assert_eq!(hits.len(), 1);
    }
}
