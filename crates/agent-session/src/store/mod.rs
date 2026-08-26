//! 单库会话存储（schema v17）：sessions、富 messages、FTS5；旧库走增量迁移不丢数据。

mod branches;
mod messages;
pub mod projects;
mod rollout_projection;
mod schema;
mod search;
mod sessions;

use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use types::SqliteStore;

pub use rollout_projection::rebuild_messages_from_rollout;
pub use schema::SCHEMA_VERSION;

/// 单次 LLM 调用的账单增量（累加到 sessions 行）。
#[derive(Debug, Clone, Default)]
pub struct BillingDelta {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub estimated_cost_usd: f64,
    pub api_call_count: i64,
    pub billing_provider: Option<String>,
    pub billing_base_url: Option<String>,
    pub billing_mode: Option<String>,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
    pub pricing_version: Option<String>,
    pub model: Option<String>,
}

/// 从 sessions 读出的账单列快照。
#[derive(Debug, Clone)]
pub struct SessionBillingRow {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub api_call_count: i64,
    pub estimated_cost_usd: f64,
    pub actual_cost_usd: Option<f64>,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
    pub pricing_version: Option<String>,
    pub billing_provider: Option<String>,
    pub billing_base_url: Option<String>,
    pub billing_mode: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewMessage<'a> {
    pub session_id: &'a str,
    pub role: &'a str,
    pub content: Option<&'a str>,
    /// Provider-facing/internal delivery view persisted by the initial INSERT.
    pub compressed_content: Option<&'a str>,
    pub tool_calls: Option<Value>,
    pub tool_call_id: Option<&'a str>,
    pub tool_name: Option<&'a str>,
    pub token_count: Option<i64>,
    pub finish_reason: Option<&'a str>,
    pub reasoning: Option<&'a str>,
    pub reasoning_content: Option<&'a str>,
    pub reasoning_details: Option<Value>,
    pub codex_reasoning_items: Option<Value>,
    pub codex_message_items: Option<Value>,
    /// 结构化媒体 JSON 数组（`MediaAsset[]`）；空则不写列。
    pub media_json: Option<&'a str>,
}

impl<'a> NewMessage<'a> {
    /// 仅填 `session_id` / `role`，其余 Option 字段为 `None`（便于 struct update）。
    pub fn empty(session_id: &'a str, role: &'a str) -> Self {
        Self {
            session_id,
            role,
            content: None,
            compressed_content: None,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            token_count: None,
            finish_reason: None,
            reasoning: None,
            reasoning_content: None,
            reasoning_details: None,
            codex_reasoning_items: None,
            codex_message_items: None,
            media_json: None,
        }
    }
}

/// 从库中读出的富消息行。
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: Option<String>,
    pub compressed_content: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Option<Value>,
    pub tool_name: Option<String>,
    pub timestamp: f64,
    pub token_count: Option<i64>,
    pub finish_reason: Option<String>,
    pub reasoning: Option<String>,
    pub reasoning_content: Option<String>,
    pub reasoning_details: Option<Value>,
    pub codex_reasoning_items: Option<Value>,
    pub codex_message_items: Option<Value>,
    /// 结构化媒体 JSON 数组字符串。
    pub media_json: Option<String>,
}

/// 从库中读出的会话元数据行。
#[derive(Debug, Clone)]
pub struct StoredSession {
    pub id: String,
    pub source: String,
    pub title: Option<String>,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub end_reason: Option<String>,
    pub model: Option<String>,
    pub parent_session_id: Option<String>,
    pub message_count: i64,
    pub tool_call_count: i64,
    pub archived_at: Option<f64>,
    pub pinned_at: Option<f64>,
    /// 源会话中作为分叉点的 user 消息行 id；旧分支可为空。
    pub branch_parent_message_id: Option<i64>,
    /// 分叉点在父会话中的 user turn 序号（从 1 开始）。
    pub branch_parent_turn_index: Option<i64>,
    /// 子会话创建时继承的完整 user turn 数。
    pub branch_inherited_turn_count: Option<i64>,
    /// 分支创建时间；普通会话与旧分支可为空。
    pub branch_created_at: Option<f64>,
}

/// 从父会话的一个已完成 user turn 创建分支后的结果。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ForkedSession {
    pub session_id: String,
    pub parent_session_id: String,
    pub parent_message_id: i64,
    pub parent_turn_index: i64,
    pub inherited_turn_count: i64,
    pub copied_message_count: i64,
    pub created_at: f64,
}

/// 会话图中的一个 user turn。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionTurnNode {
    pub turn_index: i64,
    pub user_message_id: i64,
    pub content: Option<String>,
    pub completed: bool,
    /// 在此 turn 分叉出的直接子会话。
    pub child_session_ids: Vec<String>,
}

/// 会话谱系图中的会话节点。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionLineageNode {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub parent_message_id: Option<i64>,
    pub parent_turn_index: Option<i64>,
    pub inherited_turn_count: i64,
    pub branch_created_at: Option<f64>,
    pub legacy_metadata: bool,
    pub orphaned: bool,
    pub turns: Vec<SessionTurnNode>,
}

/// 以可达根会话为起点的递归谱系图，适合直接序列化给 Tauri。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionLineageGraph {
    pub requested_session_id: String,
    pub root_session_id: String,
    pub nodes: Vec<SessionLineageNode>,
    pub cycle_detected: bool,
    pub orphaned_parent_ids: Vec<String>,
}

/// FTS 搜索命中。
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub snippet: String,
    /// 邻接上下文（同会话前后消息摘要）；无则空串。
    pub context: String,
    pub tool_name: Option<String>,
}

/// UI 恢复用的折叠后聊天气泡。
#[derive(Debug, Clone)]
pub struct ChatHistoryMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub reasoning: Option<String>,
    pub activities: Vec<ChatActivityStored>,
    /// `reasoning_details.astro_timeline_v1`（JSON array）
    pub segments: Option<Value>,
    /// `reasoning_details.astro_surfaces_v1`（JSON array）
    pub ui_surfaces: Option<Value>,
}

/// 侧栏「近期会话」列表项：`title` 优先，否则用首条 user `content` 截断作 preview。
#[derive(Debug, Clone)]
pub struct RecentSession {
    pub id: String,
    pub title: Option<String>,
    pub started_at: f64,
    /// 首条 user 消息正文截断；无则 `None`。
    pub preview: Option<String>,
    pub ended_at: Option<f64>,
    pub end_reason: Option<String>,
    pub archived_at: Option<f64>,
    pub pinned_at: Option<f64>,
}

/// 会话列表筛选条件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionListFilter {
    Active,
    Archived,
}

/// 助手气泡上的工具/活动条（由 `tool_calls` + 后续 `tool` 行折叠）。
#[derive(Debug, Clone)]
pub struct ChatActivityStored {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub input: Option<String>,
    pub output: Option<String>,
    pub status: Option<String>,
    /// 工具结果结构化媒体（`messages.media_json` 解析后的 JSON 数组）。
    pub media: Option<Value>,
}

pub(crate) fn now_epoch_secs() -> Result<f64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_secs_f64())
}

pub(crate) fn json_to_db(value: &Option<Value>) -> Result<Option<String>> {
    match value {
        Some(v) => Ok(Some(serde_json::to_string(v)?)),
        None => Ok(None),
    }
}

pub(crate) fn json_from_db(raw: Option<String>) -> Result<Option<Value>> {
    match raw {
        Some(s) => Ok(Some(serde_json::from_str(&s)?)),
        None => Ok(None),
    }
}

/// 单库会话存储：元数据、富消息行与消息级 FTS。
pub struct SessionStore {
    pub(crate) conn: Connection,
    path: PathBuf,
}

impl SessionStore {
    /// 打开或创建 `state.db`。schema 低于 [`SCHEMA_VERSION`] 时进行增量迁移。
    pub fn open(path: &Path) -> Result<Self> {
        if path.exists() {
            let version = peek_schema_version(path).unwrap_or(0);
            // v13→v17 为 additive（ALTER / 数据清洗），可就地升级，不必丢历史。
            let additive_only = (13..SCHEMA_VERSION).contains(&version);
            if version < SCHEMA_VERSION && !additive_only {
                tracing::warn!(
                    version,
                    target = SCHEMA_VERSION,
                    "session state.db outdated; discarding prior chat history"
                );
                types::delete_sqlite_files(path);
            } else {
                tracing::debug!(version, target = SCHEMA_VERSION, "session state.db opened");
            }
        }
        let conn = types::open_wal(path)
            .with_context(|| format!("open session store at {}", path.display()))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let store = Self {
            conn,
            path: path.to_path_buf(),
        };
        store.migrate_schema()?;
        // FTS 触发器自愈。
        store.repair_messages_fts_if_needed()?;
        // 补齐「有 messages、无 sessions 行」的孤儿会话。
        store.backfill_sessions_from_messages()?;
        Ok(store)
    }

    /// 打开 `sessions_dir/state.db`；若存在旁路旧 `sessions.db` 则删除（不导入）。
    pub fn open_sessions_dir(sessions_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(sessions_dir)
            .with_context(|| format!("create sessions dir {}", sessions_dir.display()))?;
        discard_sidecar_sessions_db(sessions_dir);
        let store = Self::open(&sessions_dir.join("state.db"))?;
        store.backfill_sessions_from_messages()?;
        Ok(store)
    }

    /// 数据库文件路径
    pub fn db_path(&self) -> &Path {
        &self.path
    }

    /// 读取当前 `schema_version` 表中的版本号。
    pub fn schema_version(&self) -> Result<i32> {
        let version: Option<i32> = self
            .conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        version.ok_or_else(|| anyhow!("schema_version table is empty"))
    }
}

impl crate::ConversationStore for SessionStore {
    fn append_message(&self, msg: NewMessage<'_>) -> Result<i64> {
        SessionStore::append_message(self, msg)
    }

    fn get_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        SessionStore::get_messages(self, session_id)
    }

    fn update_message_compressed_content(
        &self,
        message_id: i64,
        compressed: Option<&str>,
    ) -> Result<()> {
        SessionStore::update_message_compressed_content(self, message_id, compressed)
    }

    fn patch_last_assistant_reasoning_details(
        &self,
        session_id: &str,
        details: &Value,
    ) -> Result<()> {
        SessionStore::patch_last_assistant_reasoning_details(self, session_id, details)
    }

    fn ensure_session(&self, id: &str, source: &str) -> Result<()> {
        SessionStore::ensure_session(self, id, source)
    }

    fn update_session_billing(&self, id: &str, d: BillingDelta) -> Result<()> {
        SessionStore::update_session_billing(self, id, d)
    }

    fn recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<crate::ScrolledMessage>> {
        SessionStore::recent_messages(self, session_id, limit)
    }

    fn recall_message_ids(&self, session_id: &str, query: &str, limit: usize) -> Result<Vec<i64>> {
        SessionStore::recall_message_ids(self, session_id, query, limit)
    }

    fn scroll_context_window(
        &self,
        session_id: &str,
        around_message_id: i64,
        window_size: i64,
    ) -> Result<Vec<crate::ScrolledMessage>> {
        SessionStore::scroll_context_window(self, session_id, around_message_id, window_size)
    }

    fn search_messages(
        &self,
        query: &str,
        source_filter: Option<&str>,
        role_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SearchHit>> {
        SessionStore::search_messages(self, query, source_filter, role_filter, limit)
    }
}

impl SqliteStore for SessionStore {
    fn path(&self) -> &Path {
        &self.path
    }

    fn migrate(&self) -> anyhow::Result<()> {
        self.migrate_schema()
    }
}

/// 读取已有库的 schema 版本；无法读取时视为 0。
fn peek_schema_version(path: &Path) -> Result<i32> {
    let conn = Connection::open(path)?;
    let has: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if !has {
        return Ok(0);
    }
    let version: Option<i32> = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(version.unwrap_or(0))
}

/// 删除旁路旧 `sessions.db`（不再导入）。
fn discard_sidecar_sessions_db(sessions_dir: &Path) {
    let base = sessions_dir.join("sessions.db");
    types::delete_sqlite_files(&base);
}

pub(crate) fn is_unique_constraint(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(e, _) => {
            e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
        }
        _ => false,
    }
}

/// 将用户查询包成 FTS5 短语（双引号转义），避免运算符注入。
pub(crate) fn escape_fts5_query(query: &str) -> String {
    let escaped = query.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

pub(crate) use types::truncate_chars;

pub(crate) fn activities_from_tool_calls(tool_calls: Option<&Value>) -> Vec<ChatActivityStored> {
    let Some(Value::Array(arr)) = tool_calls else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|tc| {
            let id = tc
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                return None;
            }
            // OpenAI 形状可能是 name 在顶层，或 function.name
            let title = tc
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    tc.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("tool")
                .to_string();
            let input = tc
                .get("arguments")
                .cloned()
                .or_else(|| tc.get("function").and_then(|f| f.get("arguments")).cloned())
                .map(|args| match args {
                    Value::String(s) => s,
                    other => other.to_string(),
                });
            Some(ChatActivityStored {
                id,
                kind: "tool".into(),
                title,
                input,
                output: None,
                status: Some("running".into()),
                media: None,
            })
        })
        .collect()
}

/// 将同轮连续 assistant 气泡合并为一条（工具循环落盘会产生多条）。
pub(crate) fn coalesce_consecutive_assistants(
    messages: Vec<ChatHistoryMessage>,
) -> Vec<ChatHistoryMessage> {
    let mut out: Vec<ChatHistoryMessage> = Vec::with_capacity(messages.len());
    for m in messages {
        if m.role != "assistant" {
            out.push(m);
            continue;
        }
        let Some(prev) = out.last_mut().filter(|p| p.role == "assistant") else {
            out.push(m);
            continue;
        };
        let contents = [prev.content.as_str(), m.content.as_str()]
            .into_iter()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        prev.content = contents.join("\n\n");
        merge_history_activities(&mut prev.activities, m.activities);
        let prev_seg_len = prev
            .segments
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let next_seg_len = m
            .segments
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if next_seg_len >= prev_seg_len && next_seg_len > 0 {
            prev.segments = m.segments;
        }
        let prev_surf_len = prev
            .ui_surfaces
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let next_surf_len = m
            .ui_surfaces
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if next_surf_len >= prev_surf_len && next_surf_len > 0 {
            prev.ui_surfaces = m.ui_surfaces;
        }
        if let Some(r) = m.reasoning.filter(|s| !s.trim().is_empty()) {
            prev.reasoning = Some(r);
        }
    }
    out
}

fn merge_history_activities(dest: &mut Vec<ChatActivityStored>, incoming: Vec<ChatActivityStored>) {
    for act in incoming {
        if let Some(prev) = dest.iter_mut().find(|a| a.id == act.id) {
            if act.output.is_some() {
                prev.output = act.output;
            }
            if act.input.is_some() {
                prev.input = act.input;
            }
            if act.media.is_some() {
                prev.media = act.media;
            }
            if act.status.is_some() {
                prev.status = act.status;
            }
            if prev.title == "tool" && act.title != "tool" {
                prev.title = act.title;
            }
        } else {
            dest.push(act);
        }
    }
}

pub(crate) fn attach_tool_output(
    assistant: &mut ChatHistoryMessage,
    call_id: Option<&str>,
    output: Option<String>,
    tool_name: Option<&str>,
    media: Option<Value>,
) {
    if let Some(cid) = call_id {
        if let Some(act) = assistant.activities.iter_mut().find(|a| a.id == cid) {
            act.output = output;
            act.status = Some("done".into());
            if media.is_some() {
                act.media = media;
            }
            if act.title == "tool" {
                if let Some(name) = tool_name {
                    act.title = name.to_string();
                }
            }
            return;
        }
    }
    // 无匹配 skeleton：按顺序挂到第一个尚无 output 的 activity，或追加。
    if let Some(act) = assistant.activities.iter_mut().find(|a| a.output.is_none()) {
        if let Some(cid) = call_id {
            act.id = cid.to_string();
        }
        if let Some(name) = tool_name {
            act.title = name.to_string();
        }
        act.output = output;
        act.status = Some("done".into());
        if media.is_some() {
            act.media = media;
        }
        return;
    }
    assistant.activities.push(ChatActivityStored {
        id: call_id.unwrap_or("unknown").to_string(),
        kind: "tool".into(),
        title: tool_name.unwrap_or("tool").to_string(),
        input: None,
        output,
        status: Some("done".into()),
        media,
    });
}
