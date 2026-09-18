//! SessionStore 当前 schema 与初始化。

use agent_db::sqlx;
use anyhow::{Context, Result};

use super::SessionStore;

pub const SCHEMA_VERSION: i32 = 25;

const THREAD_CONTEXT_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS thread_context (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    notes TEXT NOT NULL DEFAULT '',
    revision INTEGER NOT NULL DEFAULT 0,
    notes_stale INTEGER NOT NULL DEFAULT 0,
    compaction_turn TEXT,
    compaction_reason TEXT,
    compaction_status TEXT NOT NULL DEFAULT 'idle',
    memory_polluted INTEGER NOT NULL DEFAULT 0
);
"#;

const THREAD_ATTACHMENTS_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS thread_attachments (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    attachment_type TEXT NOT NULL,
    identity_key TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at REAL NOT NULL,
    UNIQUE(session_id, attachment_type, identity_key)
);

CREATE INDEX IF NOT EXISTS idx_thread_attachments_session
    ON thread_attachments(session_id, created_at DESC, id DESC);
"#;

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
    search_text_seg TEXT NOT NULL DEFAULT '',
    tool_name TEXT,
    timestamp REAL NOT NULL,
    token_count INTEGER,
    finish_reason TEXT
);

CREATE INDEX IF NOT EXISTS idx_response_items_session
    ON response_items(session_id, timestamp, id);
"#;

// 只索引 `search_text_seg`（按字切分的检索文本）与工具名。
//
// - 不再索引 `item_json`：它是整条消息的 JSON，正文已被 search_text 覆盖，查询侧也从不读取。
// - `content=''`（contentless）不再保留正文副本，`contentless_delete=1` 支持按 rowid 删除；
//   展示用摘要改由基表的原文生成，因此不再依赖 `snippet()`。
// - trigram 表已移除：中文子串由按字切分承担，英文子串由查询侧的前缀匹配承担。
const RESPONSE_ITEMS_FTS_DDL: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS response_items_fts USING fts5(
    search_text_seg,
    tool_name,
    content = '',
    contentless_delete = 1,
    tokenize = 'unicode61'
);

CREATE TRIGGER IF NOT EXISTS response_items_fts_insert AFTER INSERT ON response_items BEGIN
    INSERT INTO response_items_fts(rowid, search_text_seg, tool_name)
        VALUES (new.id, new.search_text_seg, new.tool_name);
END;

CREATE TRIGGER IF NOT EXISTS response_items_fts_delete AFTER DELETE ON response_items BEGIN
    DELETE FROM response_items_fts WHERE rowid = old.id;
END;

-- contentless 表不支持原地更新，统一走删除后重建。
CREATE TRIGGER IF NOT EXISTS response_items_fts_update AFTER UPDATE ON response_items BEGIN
    DELETE FROM response_items_fts WHERE rowid = old.id;
    INSERT INTO response_items_fts(rowid, search_text_seg, tool_name)
        VALUES (new.id, new.search_text_seg, new.tool_name);
END;
"#;

impl SessionStore {
    /// 初始化当前 schema。已有数据库必须精确匹配当前版本。
    pub(crate) async fn initialize_schema(&self) -> Result<()> {
        let current = self.read_schema_version_or_zero().await?;
        if current == SCHEMA_VERSION {
            return self.validate_current_schema().await;
        }
        // v22/v23 -> v24 是加表，绝不重建既有会话；补完结构后再进入 v25 的索引重建。
        if matches!(current, 22 | 23) {
            let mut tx = self.pool.begin().await?;
            if current == 22 {
                sqlx::raw_sql(THREAD_CONTEXT_DDL).execute(&mut *tx).await?;
            }
            sqlx::raw_sql(THREAD_ATTACHMENTS_DDL)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE schema_version SET version = ?1")
                .bind(24)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        if matches!(current, 22..=24) {
            self.migrate_v24_to_v25().await?;
            return self.validate_current_schema().await;
        }
        if current != 0 || self.has_user_tables().await? {
            self.rebuild_schema().await?;
            return self.validate_current_schema().await;
        }

        let mut tx = self.pool.begin().await?;
        sqlx::raw_sql(SCHEMA_DDL).execute(&mut *tx).await?;
        sqlx::raw_sql(THREAD_CONTEXT_DDL).execute(&mut *tx).await?;
        sqlx::raw_sql(THREAD_ATTACHMENTS_DDL)
            .execute(&mut *tx)
            .await?;
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

    async fn response_items_columns(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM pragma_table_info('response_items')")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|(name,)| name).collect())
    }

    /// v24 -> v25：中文检索改为按字切分。
    ///
    /// 只重写派生数据，不触碰会话正文：新增 `search_text_seg` 列，把 FTS 索引替换为单张
    /// contentless 表（移除 trigram 表与 `item_json` 列），再按批次回填切分文本。
    ///
    /// 幂等且可断点续跑——结构改动全部带 `IF NOT EXISTS`，回填只是重算同一结果，
    /// 版本号在回填完成后才推进，因此中途退出后下次启动会从头重跑安全的部分。
    async fn migrate_v24_to_v25(&self) -> Result<()> {
        let thread_context_columns: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM pragma_table_info('thread_context')")
                .fetch_all(&self.pool)
                .await
                .context("read thread_context columns")?;
        if !thread_context_columns
            .iter()
            .any(|(name,)| name == "memory_polluted")
        {
            sqlx::query(
                "ALTER TABLE thread_context
                 ADD COLUMN memory_polluted INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await
            .context("add thread_context.memory_polluted")?;
        }
        if !self
            .response_items_columns()
            .await?
            .iter()
            .any(|name| name == "search_text_seg")
        {
            sqlx::query(
                "ALTER TABLE response_items ADD COLUMN search_text_seg TEXT NOT NULL DEFAULT ''",
            )
            .execute(&self.pool)
            .await
            .context("add response_items.search_text_seg")?;
        }
        sqlx::raw_sql(
            "DROP TRIGGER IF EXISTS response_items_fts_insert;
             DROP TRIGGER IF EXISTS response_items_fts_delete;
             DROP TRIGGER IF EXISTS response_items_fts_update;
             DROP TABLE IF EXISTS response_items_fts;
             DROP TABLE IF EXISTS response_items_fts_trigram;",
        )
        .execute(&self.pool)
        .await
        .context("drop legacy response item FTS index")?;
        sqlx::raw_sql(RESPONSE_ITEMS_FTS_DDL)
            .execute(&self.pool)
            .await
            .context("create response item FTS index")?;

        // 回填：UPDATE 触发 AFTER UPDATE 触发器，索引随之重建。
        let mut last_id = 0_i64;
        loop {
            let rows: Vec<(i64, String)> = sqlx::query_as(
                "SELECT id, search_text FROM response_items
                 WHERE id > ?1 ORDER BY id LIMIT 500",
            )
            .bind(last_id)
            .fetch_all(&self.pool)
            .await?;
            if rows.is_empty() {
                break;
            }
            let mut tx = self.pool.begin().await?;
            for (id, raw) in &rows {
                sqlx::query("UPDATE response_items SET search_text_seg = ?1 WHERE id = ?2")
                    .bind(types::search_text::segment_for_index(raw))
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                last_id = *id;
            }
            tx.commit().await?;
        }

        sqlx::query("UPDATE schema_version SET version = ?1")
            .bind(SCHEMA_VERSION)
            .execute(&self.pool)
            .await?;
        // 旧的 trigram 索引腾出的页回收到 freelist，best-effort。
        let _ = sqlx::query("PRAGMA incremental_vacuum")
            .execute(&self.pool)
            .await;
        Ok(())
    }

    async fn validate_current_schema(&self) -> Result<()> {
        sqlx::query("SELECT notes, revision, notes_stale, compaction_turn, compaction_reason, compaction_status, memory_polluted FROM thread_context LIMIT 0")
            .execute(&self.pool).await.context("thread context table is incomplete")?;
        sqlx::query(
            "SELECT id, project_id, branch_kind, branch_parent_message_id,
                    branch_parent_turn_index, branch_inherited_turn_count, branch_created_at
             FROM sessions LIMIT 0",
        )
        .execute(&self.pool)
        .await
        .context("session database schema marker is current but sessions table is incomplete")?;
        sqlx::query(
            "SELECT id, item_json, role, search_text, search_text_seg, tool_name
             FROM response_items LIMIT 0",
        )
        .execute(&self.pool)
        .await
        .context(
            "session database schema marker is current but response_items table is incomplete",
        )?;
        sqlx::query(
            "SELECT id, session_id, attachment_type, identity_key, payload_json, created_at
             FROM thread_attachments LIMIT 0",
        )
        .execute(&self.pool)
        .await
        .context(
            "session database schema marker is current but thread_attachments is incomplete",
        )?;
        sqlx::query("SELECT rowid FROM response_items_fts LIMIT 0")
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
             DROP TABLE IF EXISTS thread_attachments;
             DROP TABLE IF EXISTS thread_context;
             DROP TABLE IF EXISTS sessions;
             DROP TABLE IF EXISTS projects;
             DROP TABLE IF EXISTS schema_version;",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(SCHEMA_DDL).execute(&mut *tx).await?;
        sqlx::raw_sql(THREAD_CONTEXT_DDL).execute(&mut *tx).await?;
        sqlx::raw_sql(THREAD_ATTACHMENTS_DDL)
            .execute(&mut *tx)
            .await?;
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
