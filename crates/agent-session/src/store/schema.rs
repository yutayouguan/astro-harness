//! SessionStore 当前 schema 与初始化。

use agent_db::sqlx;
use anyhow::{Context, Result};

use super::SessionStore;

pub const SCHEMA_VERSION: i32 = 22;

const SCHEMA_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    icon TEXT
);

CREATE TABLE IF NOT EXISTS project_roots (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    PRIMARY KEY (project_id, path)
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
    archived_at REAL,
    pinned_at REAL,
    api_call_count INTEGER DEFAULT 0,
    project_root TEXT,
    project_id TEXT REFERENCES projects(id),
    branch_kind TEXT,
    branch_parent_message_id INTEGER,
    branch_parent_turn_index INTEGER,
    branch_inherited_turn_count INTEGER,
    branch_created_at REAL,
    FOREIGN KEY (parent_session_id) REFERENCES sessions(id)
);

CREATE INDEX IF NOT EXISTS idx_sessions_source ON sessions(source);
CREATE INDEX IF NOT EXISTS idx_sessions_parent ON sessions(parent_session_id);
CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_title_unique
    ON sessions(title) WHERE title IS NOT NULL;

CREATE TABLE IF NOT EXISTS response_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    item_json TEXT NOT NULL,
    role TEXT,
    search_text TEXT NOT NULL DEFAULT '',
    tool_name TEXT,
    timestamp REAL NOT NULL,
    token_count INTEGER,
    finish_reason TEXT
);

CREATE INDEX IF NOT EXISTS idx_response_items_session
    ON response_items(session_id, timestamp, id);
"#;

const RESPONSE_ITEMS_FTS_DDL: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS response_items_fts USING fts5(
    search_text,
    tool_name,
    item_json,
    tokenize = 'unicode61'
);

CREATE VIRTUAL TABLE IF NOT EXISTS response_items_fts_trigram USING fts5(
    search_text,
    tool_name,
    item_json,
    tokenize = 'trigram'
);

CREATE TRIGGER IF NOT EXISTS response_items_fts_insert AFTER INSERT ON response_items BEGIN
    INSERT INTO response_items_fts(rowid, search_text, tool_name, item_json)
        VALUES (new.id, new.search_text, new.tool_name, new.item_json);
    INSERT INTO response_items_fts_trigram(rowid, search_text, tool_name, item_json)
        VALUES (new.id, new.search_text, new.tool_name, new.item_json);
END;

-- SQLite 3.43+ 上 contentful FTS5 的 INSERT … VALUES('delete', …) 会报 SQL logic error；
-- 改用普通 DELETE（与 direct DELETE FROM fts 行为一致）。
CREATE TRIGGER IF NOT EXISTS response_items_fts_delete AFTER DELETE ON response_items BEGIN
    DELETE FROM response_items_fts WHERE rowid = old.id;
    DELETE FROM response_items_fts_trigram WHERE rowid = old.id;
END;

CREATE TRIGGER IF NOT EXISTS response_items_fts_update AFTER UPDATE ON response_items BEGIN
    DELETE FROM response_items_fts WHERE rowid = old.id;
    INSERT INTO response_items_fts(rowid, search_text, tool_name, item_json)
        VALUES (new.id, new.search_text, new.tool_name, new.item_json);
    DELETE FROM response_items_fts_trigram WHERE rowid = old.id;
    INSERT INTO response_items_fts_trigram(rowid, search_text, tool_name, item_json)
        VALUES (new.id, new.search_text, new.tool_name, new.item_json);
END;
"#;

impl SessionStore {
    /// 初始化当前 schema。已有数据库必须精确匹配当前版本。
    pub(crate) async fn initialize_schema(&self) -> Result<()> {
        let current = self.read_schema_version_or_zero().await?;
        if current == SCHEMA_VERSION {
            return self.validate_current_schema().await;
        }
        if current != 0 || self.has_user_tables().await? {
            self.rebuild_schema().await?;
            return self.validate_current_schema().await;
        }

        let mut tx = self.pool.begin().await?;
        sqlx::raw_sql(SCHEMA_DDL).execute(&mut *tx).await?;
        sqlx::raw_sql(RESPONSE_ITEMS_FTS_DDL)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM schema_version")
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO schema_version (version) VALUES (?1)")
            .bind(SCHEMA_VERSION)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.validate_current_schema().await
    }

    async fn has_user_tables(&self) -> Result<bool> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    async fn validate_current_schema(&self) -> Result<()> {
        sqlx::query(
            "SELECT id, project_id, branch_kind, branch_parent_message_id,
                    branch_parent_turn_index, branch_inherited_turn_count, branch_created_at
             FROM sessions LIMIT 0",
        )
        .execute(&self.pool)
        .await
        .context("session database schema marker is current but sessions table is incomplete")?;
        sqlx::query(
            "SELECT id, item_json, role, search_text, tool_name
             FROM response_items LIMIT 0",
        )
        .execute(&self.pool)
        .await
        .context("session database schema marker is current but response_items table is incomplete")?;
        sqlx::query("SELECT rowid FROM response_items_fts LIMIT 0")
            .execute(&self.pool)
            .await
            .context("session database schema marker is current but FTS tables are incomplete")?;
        sqlx::query("SELECT rowid FROM response_items_fts_trigram LIMIT 0")
            .execute(&self.pool)
            .await
            .context("session database schema marker is current but FTS tables are incomplete")?;
        Ok(())
    }

    async fn rebuild_schema(&self) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("PRAGMA foreign_keys=OFF")
            .execute(&mut *tx)
            .await?;
        sqlx::raw_sql(
            "DROP TRIGGER IF EXISTS messages_fts_insert;
             DROP TRIGGER IF EXISTS messages_fts_delete;
             DROP TRIGGER IF EXISTS messages_fts_update;
             DROP TRIGGER IF EXISTS response_items_fts_insert;
             DROP TRIGGER IF EXISTS response_items_fts_delete;
             DROP TRIGGER IF EXISTS response_items_fts_update;
             DROP TABLE IF EXISTS messages_fts;
             DROP TABLE IF EXISTS messages_fts_trigram;
             DROP TABLE IF EXISTS response_items_fts;
             DROP TABLE IF EXISTS response_items_fts_trigram;
             DROP TABLE IF EXISTS messages;
             DROP TABLE IF EXISTS response_items;
             DROP TABLE IF EXISTS project_roots;
             DROP TABLE IF EXISTS sessions;
             DROP TABLE IF EXISTS projects;
             DROP TABLE IF EXISTS schema_version;",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(SCHEMA_DDL).execute(&mut *tx).await?;
        sqlx::raw_sql(RESPONSE_ITEMS_FTS_DDL)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO schema_version (version) VALUES (?1)")
            .bind(SCHEMA_VERSION)
            .execute(&mut *tx)
            .await?;
        sqlx::query("PRAGMA foreign_keys=ON")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn read_schema_version_or_zero(&self) -> Result<i32> {
        let (exists,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_version'",
        )
        .fetch_one(&self.pool)
        .await?;
        if exists == 0 {
            return Ok(0);
        }
        let version: Option<(i32,)> = sqlx::query_as("SELECT version FROM schema_version LIMIT 1")
            .fetch_optional(&self.pool)
            .await?;
        Ok(version.map(|(v,)| v).unwrap_or(0))
    }
}
