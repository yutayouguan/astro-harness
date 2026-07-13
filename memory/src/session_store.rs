//! 单库会话存储（schema v11）：sessions、富 messages、FTS5 与迁移门控。

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// 写入一条消息时的入参（富字段；JSON 列在落库前序列化）。
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

fn now_epoch_secs() -> Result<f64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_secs_f64())
}

fn json_to_db(value: &Option<Value>) -> Result<Option<String>> {
    match value {
        Some(v) => Ok(Some(serde_json::to_string(v)?)),
        None => Ok(None),
    }
}

fn json_from_db(raw: Option<String>) -> Result<Option<Value>> {
    match raw {
        Some(s) => Ok(Some(serde_json::from_str(&s)?)),
        None => Ok(None),
    }
}

/// 当前目标 schema 版本。
pub const SCHEMA_VERSION: i32 = 11;

/// 空库直接建到 v11 的完整 DDL（含 FTS inline 模式与触发器）。
const SCHEMA_V11_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS state_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    user_id TEXT,
    model TEXT,
    model_config TEXT,
    system_prompt TEXT,
    parent_session_id TEXT,
    started_at REAL NOT NULL,
    ended_at REAL,
    end_reason TEXT,
    message_count INTEGER DEFAULT 0,
    tool_call_count INTEGER DEFAULT 0,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cache_write_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    billing_provider TEXT,
    billing_base_url TEXT,
    billing_mode TEXT,
    estimated_cost_usd REAL,
    actual_cost_usd REAL,
    cost_status TEXT,
    cost_source TEXT,
    pricing_version TEXT,
    title TEXT,
    api_call_count INTEGER DEFAULT 0,
    FOREIGN KEY (parent_session_id) REFERENCES sessions(id)
);

CREATE INDEX IF NOT EXISTS idx_sessions_source ON sessions(source);
CREATE INDEX IF NOT EXISTS idx_sessions_parent ON sessions(parent_session_id);
CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_title_unique
    ON sessions(title) WHERE title IS NOT NULL;

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    role TEXT NOT NULL,
    content TEXT,
    tool_call_id TEXT,
    tool_calls TEXT,
    tool_name TEXT,
    timestamp REAL NOT NULL,
    token_count INTEGER,
    finish_reason TEXT,
    reasoning TEXT,
    reasoning_content TEXT,
    reasoning_details TEXT,
    codex_reasoning_items TEXT,
    codex_message_items TEXT
);

CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, timestamp);
"#;

const MESSAGES_FTS_V11_DDL: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    tool_name,
    tool_calls,
    tokenize = 'unicode61'
);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts_trigram USING fts5(
    content,
    tool_name,
    tool_calls,
    tokenize = 'trigram'
);

CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, content, tool_name, tool_calls)
        VALUES (new.id, new.content, new.tool_name, new.tool_calls);
    INSERT INTO messages_fts_trigram(rowid, content, tool_name, tool_calls)
        VALUES (new.id, new.content, new.tool_name, new.tool_calls);
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content, tool_name, tool_calls)
        VALUES ('delete', old.id, old.content, old.tool_name, old.tool_calls);
    INSERT INTO messages_fts_trigram(messages_fts_trigram, rowid, content, tool_name, tool_calls)
        VALUES ('delete', old.id, old.content, old.tool_name, old.tool_calls);
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_update AFTER UPDATE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content, tool_name, tool_calls)
        VALUES ('delete', old.id, old.content, old.tool_name, old.tool_calls);
    INSERT INTO messages_fts(rowid, content, tool_name, tool_calls)
        VALUES (new.id, new.content, new.tool_name, new.tool_calls);
    INSERT INTO messages_fts_trigram(messages_fts_trigram, rowid, content, tool_name, tool_calls)
        VALUES ('delete', old.id, old.content, old.tool_name, old.tool_calls);
    INSERT INTO messages_fts_trigram(rowid, content, tool_name, tool_calls)
        VALUES (new.id, new.content, new.tool_name, new.tool_calls);
END;
"#;

/// `messages` 表在 v11 需要补齐的列（声明式 ADD COLUMN）。
const MESSAGES_V11_COLUMNS: &[(&str, &str)] = &[
    ("tool_call_id", "TEXT"),
    ("tool_calls", "TEXT"),
    ("tool_name", "TEXT"),
    ("token_count", "INTEGER"),
    ("finish_reason", "TEXT"),
    ("reasoning", "TEXT"),
    ("reasoning_content", "TEXT"),
    ("reasoning_details", "TEXT"),
    ("codex_reasoning_items", "TEXT"),
    ("codex_message_items", "TEXT"),
];

/// 单库会话存储：元数据、富消息行与消息级 FTS。
pub struct SessionStore {
    conn: Connection,
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
        Ok(store)
    }

    /// 打开 `sessions_dir/state.db` 并迁移到 v11；若尚未导入，则从旁路 `sessions.db` 迁入会话元数据。
    pub fn open_with_legacy_migration(sessions_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(sessions_dir)
            .with_context(|| format!("create sessions dir {}", sessions_dir.display()))?;
        let store = Self::open(&sessions_dir.join("state.db"))?;
        store.import_legacy_sessions_db_once(sessions_dir)?;
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

    /// 按 id 读取会话元数据。
    pub fn get_session(&self, id: &str) -> Result<Option<StoredSession>> {
        self.conn
            .query_row(
                "SELECT id, source, title, started_at, ended_at, end_reason,
                        model, parent_session_id, message_count, tool_call_count
                 FROM sessions WHERE id = ?1",
                params![id],
                |row| {
                    Ok(StoredSession {
                        id: row.get(0)?,
                        source: row.get(1)?,
                        title: row.get(2)?,
                        started_at: row.get(3)?,
                        ended_at: row.get(4)?,
                        end_reason: row.get(5)?,
                        model: row.get(6)?,
                        parent_session_id: row.get(7)?,
                        message_count: row.get(8)?,
                        tool_call_count: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// 插入一条会话元数据（最小字段集）。
    pub fn create_session(
        &self,
        id: &str,
        source: &str,
        model: Option<&str>,
        user_id: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> Result<()> {
        let started_at = now_epoch_secs()?;
        self.conn.execute(
            "INSERT INTO sessions (id, source, model, user_id, parent_session_id, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, source, model, user_id, parent_session_id, started_at],
        )?;
        Ok(())
    }

    /// 追加一条富消息，并递增 `sessions.message_count`（`role=tool` 时同时 `tool_call_count++`）。
    pub fn append_message(&self, msg: NewMessage<'_>) -> Result<i64> {
        let timestamp = now_epoch_secs()?;
        let tool_calls = json_to_db(&msg.tool_calls)?;
        let reasoning_details = json_to_db(&msg.reasoning_details)?;
        let codex_reasoning_items = json_to_db(&msg.codex_reasoning_items)?;
        let codex_message_items = json_to_db(&msg.codex_message_items)?;

        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO messages (
                session_id, role, content, tool_call_id, tool_calls, tool_name,
                timestamp, token_count, finish_reason,
                reasoning, reasoning_content, reasoning_details,
                codex_reasoning_items, codex_message_items
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9,
                ?10, ?11, ?12,
                ?13, ?14
             )",
            params![
                msg.session_id,
                msg.role,
                msg.content,
                msg.tool_call_id,
                tool_calls,
                msg.tool_name,
                timestamp,
                msg.token_count,
                msg.finish_reason,
                msg.reasoning,
                msg.reasoning_content,
                reasoning_details,
                codex_reasoning_items,
                codex_message_items,
            ],
        )?;
        let id = tx.last_insert_rowid();

        if msg.role == "tool" {
            tx.execute(
                "UPDATE sessions
                 SET message_count = message_count + 1,
                     tool_call_count = tool_call_count + 1
                 WHERE id = ?1",
                params![msg.session_id],
            )?;
        } else {
            tx.execute(
                "UPDATE sessions SET message_count = message_count + 1 WHERE id = ?1",
                params![msg.session_id],
            )?;
        }

        tx.commit()?;
        Ok(id)
    }

    /// 按时间顺序读取会话内全部消息行。
    pub fn get_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason,
                    reasoning, reasoning_content, reasoning_details,
                    codex_reasoning_items, codex_message_items
             FROM messages
             WHERE session_id = ?1
             ORDER BY timestamp ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, f64>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<String>>(12)?,
                row.get::<_, Option<String>>(13)?,
                row.get::<_, Option<String>>(14)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (
                id,
                session_id,
                role,
                content,
                tool_call_id,
                tool_calls_raw,
                tool_name,
                timestamp,
                token_count,
                finish_reason,
                reasoning,
                reasoning_content,
                reasoning_details_raw,
                codex_reasoning_items_raw,
                codex_message_items_raw,
            ) = row?;
            out.push(StoredMessage {
                id,
                session_id,
                role,
                content,
                tool_call_id,
                tool_calls: json_from_db(tool_calls_raw)?,
                tool_name,
                timestamp,
                token_count,
                finish_reason,
                reasoning,
                reasoning_content,
                reasoning_details: json_from_db(reasoning_details_raw)?,
                codex_reasoning_items: json_from_db(codex_reasoning_items_raw)?,
                codex_message_items: json_from_db(codex_message_items_raw)?,
            });
        }
        Ok(out)
    }

    /// 重建 OpenAI conversation 形状（assistant 带 `tool_calls` / `reasoning*`）。
    pub fn get_messages_as_conversation(&self, session_id: &str) -> Result<Vec<Value>> {
        let messages = self.get_messages(session_id)?;
        let mut out = Vec::with_capacity(messages.len());
        for m in messages {
            let mut obj = serde_json::Map::new();
            obj.insert("role".into(), Value::String(m.role));
            match m.content {
                Some(c) => {
                    obj.insert("content".into(), Value::String(c));
                }
                None => {
                    obj.insert("content".into(), Value::Null);
                }
            }
            if let Some(tc) = m.tool_calls {
                obj.insert("tool_calls".into(), tc);
            }
            if let Some(id) = m.tool_call_id {
                obj.insert("tool_call_id".into(), Value::String(id));
            }
            if let Some(name) = m.tool_name {
                obj.insert("name".into(), Value::String(name));
            }
            if let Some(r) = m.reasoning {
                obj.insert("reasoning".into(), Value::String(r));
            }
            if let Some(r) = m.reasoning_content {
                obj.insert("reasoning_content".into(), Value::String(r));
            }
            if let Some(r) = m.reasoning_details {
                obj.insert("reasoning_details".into(), r);
            }
            if let Some(r) = m.codex_reasoning_items {
                obj.insert("codex_reasoning_items".into(), r);
            }
            if let Some(r) = m.codex_message_items {
                obj.insert("codex_message_items".into(), r);
            }
            out.push(Value::Object(obj));
        }
        Ok(out)
    }

    /// 跨会话消息 FTS：优先 `messages_fts`，再合并 `messages_fts_trigram`（CJK / 子串）。
    pub fn search_messages(
        &self,
        query: &str,
        source_filter: Option<&str>,
        role_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SearchHit>> {
        let q = query.trim();
        if q.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let fts_query = escape_fts5_query(q);
        let mut hits = Vec::new();
        let mut seen = HashSet::new();

        self.collect_fts_hits(
            "messages_fts",
            &fts_query,
            source_filter,
            role_filter,
            limit,
            &mut hits,
            &mut seen,
        )?;
        if (hits.len() as i64) < limit {
            self.collect_fts_hits(
                "messages_fts_trigram",
                &fts_query,
                source_filter,
                role_filter,
                limit,
                &mut hits,
                &mut seen,
            )?;
        }

        for hit in &mut hits {
            hit.context = self.neighbor_context(hit.session_id.as_str(), hit.id)?;
        }
        Ok(hits)
    }

    /// 按时间扫描并折叠为 UI 气泡：user / assistant（含 activities）；tool 不单独成泡。
    pub fn build_chat_history(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<ChatHistoryMessage>> {
        let messages = self.get_messages(session_id)?;
        let mut out: Vec<ChatHistoryMessage> = Vec::new();

        for m in messages {
            match m.role.as_str() {
                "user" => {
                    out.push(ChatHistoryMessage {
                        id: m.id.to_string(),
                        role: "user".into(),
                        content: m.content.unwrap_or_default(),
                        reasoning: None,
                        activities: Vec::new(),
                    });
                }
                "assistant" => {
                    let activities = activities_from_tool_calls(m.tool_calls.as_ref());
                    out.push(ChatHistoryMessage {
                        id: m.id.to_string(),
                        role: "assistant".into(),
                        content: m.content.unwrap_or_default(),
                        reasoning: m.reasoning.or(m.reasoning_content),
                        activities,
                    });
                }
                "tool" => {
                    let call_id = m.tool_call_id.as_deref();
                    let output = m.content.clone();
                    if let Some(assistant) = out.iter_mut().rev().find(|msg| msg.role == "assistant")
                    {
                        attach_tool_output(assistant, call_id, output, m.tool_name.as_deref());
                    }
                }
                _ => {}
            }
        }

        if out.len() > limit {
            let skip = out.len() - limit;
            out = out.into_iter().skip(skip).collect();
        }
        Ok(out)
    }

    fn collect_fts_hits(
        &self,
        fts_table: &str,
        fts_query: &str,
        source_filter: Option<&str>,
        role_filter: Option<&str>,
        limit: i64,
        hits: &mut Vec<SearchHit>,
        seen: &mut HashSet<i64>,
    ) -> Result<()> {
        let remaining = limit - hits.len() as i64;
        if remaining <= 0 {
            return Ok(());
        }

        // fts_table 仅内部常量 "messages_fts" | "messages_fts_trigram"。
        let sql = format!(
            "SELECT m.id, m.session_id, m.role,
                    COALESCE(snippet({fts}, 0, '', '', '…', 32), m.content, ''),
                    m.tool_name
             FROM {fts} AS f
             JOIN messages AS m ON m.id = f.rowid
             JOIN sessions AS s ON s.id = m.session_id
             WHERE {fts} MATCH ?1
               AND (?2 IS NULL OR s.source = ?2)
               AND (?3 IS NULL OR m.role = ?3)
             ORDER BY m.timestamp DESC, m.id DESC
             LIMIT ?4",
            fts = fts_table
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![fts_query, source_filter, role_filter, remaining],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )?;

        for row in rows {
            let (id, session_id, role, snippet, tool_name) = row?;
            if !seen.insert(id) {
                continue;
            }
            hits.push(SearchHit {
                id,
                session_id,
                role,
                snippet,
                context: String::new(),
                tool_name,
            });
            if hits.len() as i64 >= limit {
                break;
            }
        }
        Ok(())
    }

    fn neighbor_context(&self, session_id: &str, message_id: i64) -> Result<String> {
        let mut parts = Vec::new();
        let prev: Option<(String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT role, content FROM messages
                 WHERE session_id = ?1 AND id < ?2
                 ORDER BY id DESC LIMIT 1",
                params![session_id, message_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((role, content)) = prev {
            parts.push(format!(
                "[prev:{role}] {}",
                truncate_chars(content.as_deref().unwrap_or(""), 80)
            ));
        }
        let next: Option<(String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT role, content FROM messages
                 WHERE session_id = ?1 AND id > ?2
                 ORDER BY id ASC LIMIT 1",
                params![session_id, message_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((role, content)) = next {
            parts.push(format!(
                "[next:{role}] {}",
                truncate_chars(content.as_deref().unwrap_or(""), 80)
            ));
        }
        Ok(parts.join(" | "))
    }

    fn migrate_to_v11(&self) -> Result<()> {
        let current = self.read_schema_version_or_zero()?;
        if current >= SCHEMA_VERSION {
            return Ok(());
        }

        let has_messages = self.table_exists("messages")?;
        if current == 0 && !has_messages {
            // 空库：直接落到最新 schema。
            self.conn.execute_batch(SCHEMA_V11_DDL)?;
            self.conn.execute_batch(MESSAGES_FTS_V11_DDL)?;
            self.stamp_schema_version()?;
            return Ok(());
        }

        // 旧 MessageDb 风格（有 messages、无 schema_version）或中间版本：声明式补列 + FTS 重建。
        self.conn.execute_batch(SCHEMA_V11_DDL)?;
        self.ensure_messages_v11_columns()?;
        self.convert_legacy_message_timestamps()?;
        self.rebuild_messages_fts_v11()?;
        self.stamp_schema_version()?;
        Ok(())
    }

    fn stamp_schema_version(&self) -> Result<()> {
        self.conn.execute("DELETE FROM schema_version", [])?;
        self.conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![SCHEMA_VERSION],
        )?;
        Ok(())
    }

    fn table_exists(&self, name: &str) -> Result<bool> {
        let exists: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
            params![name],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    fn table_columns(&self, table: &str) -> Result<HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare(&format!("PRAGMA table_info({table})"))?;
        let cols = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<HashSet<_>, _>>()?;
        Ok(cols)
    }

    fn ensure_messages_v11_columns(&self) -> Result<()> {
        let existing = self.table_columns("messages")?;
        for (name, ty) in MESSAGES_V11_COLUMNS {
            if !existing.contains(*name) {
                self.conn
                    .execute(
                        &format!("ALTER TABLE messages ADD COLUMN {name} {ty}"),
                        [],
                    )
                    .with_context(|| format!("add messages.{name}"))?;
            }
        }
        Ok(())
    }

    /// 将旧 MessageDb 的 DATETIME 文本时间戳转为 Unix epoch REAL，供 `get_messages` 读取。
    fn convert_legacy_message_timestamps(&self) -> Result<()> {
        self.conn.execute_batch(
            "UPDATE messages
             SET timestamp = CAST(strftime('%s', timestamp) AS REAL)
             WHERE typeof(timestamp) = 'text';
             UPDATE messages
             SET timestamp = CAST(strftime('%s', 'now') AS REAL)
             WHERE timestamp IS NULL;",
        )?;
        Ok(())
    }

    fn rebuild_messages_fts_v11(&self) -> Result<()> {
        // 旧 MessageDb 触发器 / external-content FTS，以及任何半成品 v11 FTS。
        self.conn.execute_batch(
            "DROP TRIGGER IF EXISTS sync_messages_to_fts;
             DROP TRIGGER IF EXISTS sync_messages_fts_update;
             DROP TRIGGER IF EXISTS sync_messages_fts_delete;
             DROP TRIGGER IF EXISTS messages_fts_insert;
             DROP TRIGGER IF EXISTS messages_fts_delete;
             DROP TRIGGER IF EXISTS messages_fts_update;
             DROP TABLE IF EXISTS messages_fts;
             DROP TABLE IF EXISTS messages_fts_trigram;",
        )?;
        self.conn.execute_batch(MESSAGES_FTS_V11_DDL)?;
        self.conn.execute_batch(
            "INSERT INTO messages_fts(rowid, content, tool_name, tool_calls)
             SELECT id, content, tool_name, tool_calls FROM messages;
             INSERT INTO messages_fts_trigram(rowid, content, tool_name, tool_calls)
             SELECT id, content, tool_name, tool_calls FROM messages;",
        )?;
        Ok(())
    }

    /// 若 `state_meta.migrated_from_sessions_db` 未设，从旁路 `sessions.db` 导入会话行（幂等）。
    fn import_legacy_sessions_db_once(&self, sessions_dir: &Path) -> Result<()> {
        let already: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM state_meta WHERE key = 'migrated_from_sessions_db'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if already.as_deref() == Some("1") {
            return Ok(());
        }

        let legacy_path = sessions_dir.join("sessions.db");
        if legacy_path.is_file() {
            let legacy = Connection::open(&legacy_path)
                .with_context(|| format!("open legacy {}", legacy_path.display()))?;
            let has_sessions: bool = legacy.query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='sessions'",
                [],
                |row| row.get(0),
            )?;
            if has_sessions {
                let mut stmt = legacy.prepare(
                    "SELECT session_id, summary,
                            CAST(strftime('%s', created_at) AS REAL) AS started_at
                     FROM sessions",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<f64>>(2)?,
                    ))
                })?;
                for row in rows {
                    let (session_id, summary, started_at) = row?;
                    let title = truncate_chars(&summary, 80);
                    let started = started_at.unwrap_or_else(|| {
                        now_epoch_secs().unwrap_or(0.0)
                    });
                    self.conn.execute(
                        "INSERT INTO sessions (id, source, title, started_at)
                         VALUES (?1, 'tauri', ?2, ?3)
                         ON CONFLICT(id) DO NOTHING",
                        params![session_id, title, started],
                    )?;
                }
            }
        }

        self.conn.execute(
            "INSERT INTO state_meta (key, value) VALUES ('migrated_from_sessions_db', '1')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )?;
        Ok(())
    }

    fn read_schema_version_or_zero(&self) -> Result<i32> {
        let exists: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_version'",
            [],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(0);
        }
        let version: Option<i32> = self
            .conn
            .query_row(
                "SELECT version FROM schema_version LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(version.unwrap_or(0))
    }
}

/// 将用户查询包成 FTS5 短语（双引号转义），避免运算符注入。
fn escape_fts5_query(query: &str) -> String {
    let escaped = query.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}…")
}

fn activities_from_tool_calls(tool_calls: Option<&Value>) -> Vec<ChatActivityStored> {
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

fn attach_tool_output(
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
