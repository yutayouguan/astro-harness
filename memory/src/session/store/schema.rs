//! Schema 版本、DDL 与迁移/自愈。

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::Path;

use super::{insert_legacy_session, now_epoch_secs, truncate_chars, SessionStore};

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

impl SessionStore {
    pub(crate) fn migrate_to_v11(&self) -> Result<()> {
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
        self.ensure_messages_content_nullable()?;
        self.rebuild_messages_fts_v11()?;
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

    pub(crate) fn table_columns(&self, table: &str) -> Result<HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare(&format!("PRAGMA table_info({table})"))?;
        let cols = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<HashSet<_>, _>>()?;
        Ok(cols)
    }

    pub(crate) fn ensure_messages_v11_columns(&self) -> Result<()> {
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

    /// SQLite 无法 `ALTER` 去掉 NOT NULL；旧 MessageDb 的 `content TEXT NOT NULL` 需整表重建。
    pub(crate) fn ensure_messages_content_nullable(&self) -> Result<()> {
        if !self.table_exists("messages")? {
            return Ok(());
        }
        if !self.column_is_not_null("messages", "content")? {
            return Ok(());
        }

        // 先拆掉 FTS / 触发器，避免 DROP TABLE messages 被依赖挡住。
        self.drop_messages_fts_objects()?;

        // 复制期关闭 FK：遗留 messages 可能尚未有对应 sessions 行。
        self.conn.execute_batch("PRAGMA foreign_keys=OFF;")?;
        self.conn.execute_batch(
            r#"
            CREATE TABLE messages_v11_rebuild (
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

            INSERT INTO messages_v11_rebuild (
                id, session_id, role, content, tool_call_id, tool_calls, tool_name,
                timestamp, token_count, finish_reason,
                reasoning, reasoning_content, reasoning_details,
                codex_reasoning_items, codex_message_items
            )
            SELECT
                id, session_id, role, content, tool_call_id, tool_calls, tool_name,
                timestamp, token_count, finish_reason,
                reasoning, reasoning_content, reasoning_details,
                codex_reasoning_items, codex_message_items
            FROM messages;

            DROP TABLE messages;
            ALTER TABLE messages_v11_rebuild RENAME TO messages;
            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, timestamp);
            "#,
        )?;
        // 恢复 AUTOINCREMENT 序列，避免后续 id 冲突。
        self.conn.execute_batch(
            "DELETE FROM sqlite_sequence WHERE name IN ('messages', 'messages_v11_rebuild');
             INSERT INTO sqlite_sequence(name, seq)
             SELECT 'messages', IFNULL(MAX(id), 0) FROM messages;",
        )?;
        self.conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        // 上面 drop 过 FTS；此处重建，避免半成品库无索引。
        self.rebuild_messages_fts_v11()?;
        Ok(())
    }

    pub(crate) fn column_is_not_null(&self, table: &str, column: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(3)?))
        })?;
        for row in rows {
            let (name, notnull) = row?;
            if name == column {
                return Ok(notnull != 0);
            }
        }
        Ok(false)
    }

    /// 将旧 MessageDb 的 DATETIME 文本时间戳转为 Unix epoch REAL，供 `get_messages` 读取。
    pub(crate) fn convert_legacy_message_timestamps(&self) -> Result<()> {
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
        // 旧 MessageDb 触发器 / external-content FTS，以及任何半成品 v11 FTS。
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
                 'legacy',
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

    /// 检测并重建失效的 `messages_fts` 触发器：
    /// - 旧 MessageDb 的 `sync_messages_*`（引用已删除的 `message_id`）
    /// - 使用 FTS5 `VALUES('delete', …)` 的 contentful 触发器（SQLite 3.43+ 会 SQL logic error）
    pub(crate) fn repair_messages_fts_if_needed(&self) -> Result<()> {
        if !self.needs_messages_fts_repair()? {
            return Ok(());
        }
        self.rebuild_messages_fts_v11()
            .context("repair messages_fts triggers")?;
        Ok(())
    }

    pub(crate) fn needs_messages_fts_repair(&self) -> Result<bool> {
        let legacy: i64 = self.conn.query_row(
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
        if legacy > 0 {
            return Ok(true);
        }

        // 旧 v11 DDL 用 INSERT … ('delete', …)，在较新 SQLite 上无法 DELETE/UPDATE 消息。
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

    /// 若 `state_meta.migrated_from_sessions_db` 未设，从旁路 `sessions.db` 导入会话行（幂等）。
    pub(crate) fn import_legacy_sessions_db_once(&self, sessions_dir: &Path) -> Result<()> {
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

        let tx = self.conn.unchecked_transaction()?;

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
                    let started = started_at.unwrap_or_else(|| now_epoch_secs().unwrap_or(0.0));
                    insert_legacy_session(&tx, &session_id, &title, started)?;
                }
            }
        }

        tx.execute(
            "INSERT INTO state_meta (key, value) VALUES ('migrated_from_sessions_db', '1')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )?;
        tx.commit()?;
        Ok(())
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
