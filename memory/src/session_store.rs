//! 单库会话存储（schema v11）：sessions、富 messages、FTS5 与迁移门控。

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
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

    fn migrate_to_v11(&self) -> Result<()> {
        let current = self.read_schema_version_or_zero()?;
        if current >= SCHEMA_VERSION {
            return Ok(());
        }
        // 空库 / 未建表：直接落到最新 schema。
        if current == 0 {
            self.conn.execute_batch(SCHEMA_V11_DDL)?;
            self.conn
                .execute("DELETE FROM schema_version", [])?;
            self.conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )?;
            return Ok(());
        }
        // 后续 task 再补逐步迁移；本期仅保证空库到 v11。
        Err(anyhow!(
            "session store schema version {current} requires stepwise migration (not implemented yet)"
        ))
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
