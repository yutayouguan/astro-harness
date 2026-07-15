//! Schema 版本、DDL 与 FTS 自愈（不做旧数据迁移）。

use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};

use super::SessionStore;

pub const SCHEMA_VERSION: i32 = 13;

const SCHEMA_V11_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
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

-- SQLite 3.43+ 上 contentful FTS5 的 INSERT … VALUES('delete', …) 会报 SQL logic error；
-- 改用普通 DELETE（与 direct DELETE FROM fts 行为一致）。
CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE rowid = old.id;
    DELETE FROM messages_fts_trigram WHERE rowid = old.id;
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_update AFTER UPDATE ON messages BEGIN
    DELETE FROM messages_fts WHERE rowid = old.id;
    INSERT INTO messages_fts(rowid, content, tool_name, tool_calls)
        VALUES (new.id, new.content, new.tool_name, new.tool_calls);
    DELETE FROM messages_fts_trigram WHERE rowid = old.id;
    INSERT INTO messages_fts_trigram(rowid, content, tool_name, tool_calls)
        VALUES (new.id, new.content, new.tool_name, new.tool_calls);
END;
"#;

impl SessionStore {
    /// 空库建表并 stamp；旧库由 [`SessionStore::open`] 删文件重建，此处不做数据迁移。
    pub(crate) fn migrate_schema(&self) -> Result<()> {
        let current = self.read_schema_version_or_zero()?;
        if current >= SCHEMA_VERSION {
            return Ok(());
        }
        if !self.table_exists("messages")? {
            self.conn.execute_batch(SCHEMA_V11_DDL)?;
            self.conn.execute_batch(MESSAGES_FTS_V11_DDL)?;
        }
        self.stamp_schema_version()?;
        Ok(())
    }

    pub(crate) fn stamp_schema_version(&self) -> Result<()> {
        self.conn.execute("DELETE FROM schema_version", [])?;
        self.conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![SCHEMA_VERSION],
        )?;
        Ok(())
    }

    pub(crate) fn table_exists(&self, name: &str) -> Result<bool> {
        let exists: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
            params![name],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    pub(crate) fn drop_messages_fts_objects(&self) -> Result<()> {
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
        Ok(())
    }

    pub(crate) fn rebuild_messages_fts_v11(&self) -> Result<()> {
        self.drop_messages_fts_objects()?;
        self.conn.execute_batch(MESSAGES_FTS_V11_DDL)?;
        self.conn.execute_batch(
            "INSERT INTO messages_fts(rowid, content, tool_name, tool_calls)
             SELECT id, content, tool_name, tool_calls FROM messages;
             INSERT INTO messages_fts_trigram(rowid, content, tool_name, tool_calls)
             SELECT id, content, tool_name, tool_calls FROM messages;",
        )?;
        Ok(())
    }

    /// 为仅存在于 `messages` 的 `session_id` 补齐 `sessions` 行（幂等）。
    pub(crate) fn backfill_sessions_from_messages(&self) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, source, started_at, message_count, tool_call_count)
             SELECT
                 m.session_id,
                 'tauri',
                 MIN(m.timestamp),
                 COUNT(*),
                 COALESCE(SUM(CASE WHEN m.role = 'tool' THEN 1 ELSE 0 END), 0)
             FROM messages m
             WHERE NOT EXISTS (SELECT 1 FROM sessions s WHERE s.id = m.session_id)
             GROUP BY m.session_id",
            [],
        )?;
        Ok(())
    }

    /// 检测并重建失效的 `messages_fts` 触发器（坏触发器 / 错误 delete 语法）。
    pub(crate) fn repair_messages_fts_if_needed(&self) -> Result<()> {
        if !self.needs_messages_fts_repair()? {
            return Ok(());
        }
        self.rebuild_messages_fts_v11()
            .context("repair messages_fts triggers")?;
        Ok(())
    }

    pub(crate) fn needs_messages_fts_repair(&self) -> Result<bool> {
        let broken: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'trigger'
               AND name IN (
                 'sync_messages_to_fts',
                 'sync_messages_fts_update',
                 'sync_messages_fts_delete'
               )",
            [],
            |row| row.get(0),
        )?;
        if broken > 0 {
            return Ok(true);
        }

        let delete_sql: Option<String> = self
            .conn
            .query_row(
                "SELECT sql FROM sqlite_master
                 WHERE type = 'trigger' AND name = 'messages_fts_delete'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(delete_sql
            .as_deref()
            .is_some_and(|sql| sql.contains("'delete'")))
    }

    pub(crate) fn read_schema_version_or_zero(&self) -> Result<i32> {
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
