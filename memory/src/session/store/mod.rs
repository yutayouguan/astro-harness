//! 单库会话存储（schema v11）：sessions、富 messages、FTS5 与迁移门控。

mod schema;
mod sessions;
mod messages;
mod search;

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub use schema::SCHEMA_VERSION;

#[derive(Debug, Clone)]
pub struct NewMessage<'a> {
    pub session_id: &'a str,
    pub role: &'a str,
    pub content: Option<&'a str>,
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
}

impl<'a> NewMessage<'a> {
    /// 仅填 `session_id` / `role`，其余 Option 字段为 `None`（便于 struct update）。
    pub fn empty(session_id: &'a str, role: &'a str) -> Self {
        Self {
            session_id,
            role,
            content: None,
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
}

/// 侧栏「近期会话」列表项：`title` 优先，否则用首条 user `content` 截断作 preview。
#[derive(Debug, Clone)]
pub struct RecentSession {
    pub id: String,
    pub title: Option<String>,
    pub started_at: f64,
    /// 首条 user 消息正文截断；无则 `None`。
    pub preview: Option<String>,
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

/// 当前目标 schema 版本。

/// 单库会话存储：元数据、富消息行与消息级 FTS。
pub struct SessionStore {
    pub(crate) conn: Connection,
}

impl SessionStore {
    /// 打开或创建 `state.db`，启用 WAL，并将 schema 迁移到 v11。
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create parent dir for {}", path.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("open session store at {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let store = Self { conn };
        store.migrate_to_v11()?;
        // 已 stamp v11 的库也可能残留旧触发器 / 失效的 FTS delete 语法，每次打开自愈。
        store.repair_messages_fts_if_needed()?;
        // 旧 MessageDb 可能留下「有 messages、无 sessions 行」的孤儿会话。
        store.backfill_sessions_from_messages()?;
        Ok(store)
    }

    /// 打开 `sessions_dir/state.db` 并迁移到 v11；若尚未导入，则从旁路 `sessions.db` 迁入会话元数据。
    pub fn open_with_legacy_migration(sessions_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(sessions_dir)
            .with_context(|| format!("create sessions dir {}", sessions_dir.display()))?;
        // 注意：`open` 可能已对孤儿 messages 做过 backfill；导入会补齐 title。
        let store = Self::open(&sessions_dir.join("state.db"))?;
        store.import_legacy_sessions_db_once(sessions_dir)?;
        // 导入后再扫一遍，覆盖「仅有 messages、无 sessions.db 行」的会话。
        store.backfill_sessions_from_messages()?;
        Ok(store)
    }

    /// 读取当前 `schema_version` 表中的版本号。
    pub fn schema_version(&self) -> Result<i32> {
        let version: Option<i32> = self
            .conn
            .query_row(
                "SELECT version FROM schema_version LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        version.ok_or_else(|| anyhow!("schema_version table is empty"))
    }

}

/// 导入遗留会话：先带 title；若撞上 `idx_sessions_title_unique` 则降级为 title=NULL。
///
/// 若该 id 已由 messages 回填（无 title），则用导入的 title 补齐。
pub(crate) fn insert_legacy_session(
    conn: &Connection,
    session_id: &str,
    title: &str,
    started_at: f64,
) -> Result<()> {
    let with_title = conn.execute(
        "INSERT INTO sessions (id, source, title, started_at)
         VALUES (?1, 'tauri', ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET
             title = COALESCE(sessions.title, excluded.title),
             source = CASE WHEN sessions.source = 'legacy' THEN excluded.source ELSE sessions.source END",
        params![session_id, title, started_at],
    );
    match with_title {
        Ok(_) => Ok(()),
        Err(err) if is_unique_constraint(&err) => {
            conn.execute(
                "INSERT INTO sessions (id, source, title, started_at)
                 VALUES (?1, 'tauri', NULL, ?2)
                 ON CONFLICT(id) DO UPDATE SET
                     source = CASE WHEN sessions.source = 'legacy' THEN 'tauri' ELSE sessions.source END",
                params![session_id, started_at],
            )?;
            Ok(())
        }
        Err(err) => Err(err.into()),
    }
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

pub(crate) fn truncate_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}…")
}

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
                .or_else(|| {
                    tc.get("function")
                        .and_then(|f| f.get("arguments"))
                        .cloned()
                })
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
            })
        })
        .collect()
}

pub(crate) fn attach_tool_output(
    assistant: &mut ChatHistoryMessage,
    call_id: Option<&str>,
    output: Option<String>,
    tool_name: Option<&str>,
) {
    if let Some(cid) = call_id {
        if let Some(act) = assistant.activities.iter_mut().find(|a| a.id == cid) {
            act.output = output;
            act.status = Some("done".into());
            if act.title == "tool" {
                if let Some(name) = tool_name {
                    act.title = name.to_string();
                }
            }
            return;
        }
    }
    // 无匹配 skeleton：按顺序挂到第一个尚无 output 的 activity，或追加。
    if let Some(act) = assistant
        .activities
        .iter_mut()
        .find(|a| a.output.is_none())
    {
        if let Some(cid) = call_id {
            act.id = cid.to_string();
        }
        if let Some(name) = tool_name {
            act.title = name.to_string();
        }
        act.output = output;
        act.status = Some("done".into());
        return;
    }
    assistant.activities.push(ChatActivityStored {
        id: call_id.unwrap_or("unknown").to_string(),
        kind: "tool".into(),
        title: tool_name.unwrap_or("tool").to_string(),
        input: None,
        output,
        status: Some("done".into()),
    });
}

