//! Schema 版本、DDL、增量迁移与 FTS 自愈。

use agent_db::sqlx;
use anyhow::{Context, Result};

use super::SessionStore;

pub const SCHEMA_VERSION: i32 = 20;

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
    archived_at REAL,
    pinned_at REAL,
    api_call_count INTEGER DEFAULT 0,
    project_root TEXT,
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
    compressed_content TEXT,
    media_json TEXT,
    tool_call_id TEXT,
    tool_calls TEXT,
    tool_name TEXT,
    timestamp REAL NOT NULL,
    token_count INTEGER,
    finish_reason TEXT,
    reasoning TEXT,
    reasoning_content TEXT,
    reasoning_details TEXT,
    reasoning_items TEXT,
    message_items TEXT
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
    /// 空库建表并 stamp；旧库由 [`SessionStore::open`] 调用增量迁移。
    ///
    /// 即便 `schema_version` 已到目标，仍幂等补齐缺列：合并分支可能先 stamp
    /// 了 v14（如 `archived_at`）却未加 `compressed_content`。
    pub(crate) async fn migrate_schema(&self) -> Result<()> {
        let current = self.read_schema_version_or_zero().await?;
        if current < SCHEMA_VERSION && !self.table_exists("messages").await? {
            sqlx::raw_sql(SCHEMA_V11_DDL).execute(&self.pool).await?;
            sqlx::raw_sql(MESSAGES_FTS_V11_DDL).execute(&self.pool).await?;
        }

        if self.table_exists("messages").await? {
            self.ensure_messages_compressed_content_column().await?;
            self.ensure_messages_media_json_column().await?;
        }
        if self.table_exists("sessions").await? && !self.column_exists("sessions", "archived_at").await? {
            sqlx::query("ALTER TABLE sessions ADD COLUMN archived_at REAL")
                .execute(&self.pool)
                .await?;
        }
        if self.table_exists("sessions").await? && !self.column_exists("sessions", "pinned_at").await? {
            sqlx::query("ALTER TABLE sessions ADD COLUMN pinned_at REAL")
                .execute(&self.pool)
                .await?;
        }

        if current < SCHEMA_VERSION {
            if (1..17).contains(&current) && self.table_exists("messages").await? {
                self.strip_legacy_chat_mode_hints()
                    .await
                    .context("strip legacy chatModeHint from user messages")?;
            }
            if self.table_exists("sessions").await? && !self.column_exists("sessions", "project_root").await? {
                sqlx::query("ALTER TABLE sessions ADD COLUMN project_root TEXT")
                    .execute(&self.pool)
                    .await?;
            }
            if !self.table_exists("projects").await? {
                sqlx::raw_sql(
                    "CREATE TABLE IF NOT EXISTS projects (
                        id TEXT PRIMARY KEY,
                        name TEXT NOT NULL,
                        position INTEGER NOT NULL DEFAULT 0,
                        created_at TEXT NOT NULL DEFAULT (datetime('now')),
                        updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                    );
                    CREATE TABLE IF NOT EXISTS project_roots (
                        project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                        path TEXT NOT NULL,
                        PRIMARY KEY (project_id, path)
                    );",
                )
                .execute(&self.pool)
                .await?;
            }
            if self.table_exists("projects").await?
                && !self.column_exists("projects", "icon").await?
            {
                sqlx::query("ALTER TABLE projects ADD COLUMN icon TEXT")
                    .execute(&self.pool)
                    .await?;
            }
            if self.table_exists("sessions").await?
                && !self.column_exists("sessions", "project_id").await?
            {
                sqlx::query(
                    "ALTER TABLE sessions ADD COLUMN project_id TEXT REFERENCES projects(id)",
                )
                .execute(&self.pool)
                .await?;
                self.migrate_project_root_to_projects().await?;
            }
            if self.table_exists("sessions").await? {
                if !self.column_exists("sessions", "branch_kind").await? {
                    sqlx::query("ALTER TABLE sessions ADD COLUMN branch_kind TEXT")
                        .execute(&self.pool).await?;
                }
                if !self.column_exists("sessions", "branch_parent_message_id").await? {
                    sqlx::query("ALTER TABLE sessions ADD COLUMN branch_parent_message_id INTEGER")
                        .execute(&self.pool).await?;
                }
                if !self.column_exists("sessions", "branch_parent_turn_index").await? {
                    sqlx::query("ALTER TABLE sessions ADD COLUMN branch_parent_turn_index INTEGER")
                        .execute(&self.pool).await?;
                }
                if !self.column_exists("sessions", "branch_inherited_turn_count").await? {
                    sqlx::query("ALTER TABLE sessions ADD COLUMN branch_inherited_turn_count INTEGER")
                        .execute(&self.pool).await?;
                }
                if !self.column_exists("sessions", "branch_created_at").await? {
                    sqlx::query("ALTER TABLE sessions ADD COLUMN branch_created_at REAL")
                        .execute(&self.pool).await?;
                }
            }
            self.stamp_schema_version().await?;
        }
        Ok(())
    }

    /// 剥离用户消息末尾遗留的 `\n\n---\n[Mode: …]`（旧 `chatModeHint`）。
    ///
    /// UPDATE 会触发 FTS 同步；仅改写带该后缀的 user 行。
    pub(crate) async fn strip_legacy_chat_mode_hints(&self) -> Result<()> {
        let rows: Vec<(i64, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT id, content, compressed_content FROM messages
             WHERE role = 'user'
               AND (
                 content LIKE '%' || char(10) || char(10) || '---' || char(10) || '[Mode: %'
                 OR compressed_content LIKE '%' || char(10) || char(10) || '---' || char(10) || '[Mode: %'
               )",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut updated = 0usize;
        for (id, content, compressed) in rows {
            let new_content = content
                .as_deref()
                .and_then(strip_legacy_chat_mode_hint)
                .or_else(|| content.clone());
            let new_compressed = compressed
                .as_deref()
                .and_then(strip_legacy_chat_mode_hint)
                .or_else(|| compressed.clone());
            if new_content == content && new_compressed == compressed {
                continue;
            }
            sqlx::query(
                "UPDATE messages SET content = ?1, compressed_content = ?2 WHERE id = ?3",
            )
            .bind(&new_content)
            .bind(&new_compressed)
            .bind(id)
            .execute(&self.pool)
            .await?;
            updated += 1;
        }
        if updated > 0 {
            tracing::info!(
                updated,
                "stripped legacy chatModeHint suffixes from user messages"
            );
        }
        Ok(())
    }

    pub(crate) async fn ensure_messages_compressed_content_column(&self) -> Result<()> {
        if !self.column_exists("messages", "compressed_content").await? {
            sqlx::query("ALTER TABLE messages ADD COLUMN compressed_content TEXT")
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn ensure_messages_media_json_column(&self) -> Result<()> {
        if !self.column_exists("messages", "media_json").await? {
            sqlx::query("ALTER TABLE messages ADD COLUMN media_json TEXT")
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// v18→v19 数据迁移：将 `sessions.project_root` 去重后创建 `projects` 实体，
    /// 并将 `sessions.project_id` 指向对应 project。
    pub(crate) async fn migrate_project_root_to_projects(&self) -> Result<()> {
        let roots: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT project_root FROM sessions
             WHERE project_root IS NOT NULL AND TRIM(project_root) != ''",
        )
        .fetch_all(&self.pool)
        .await?;

        for (root,) in &roots {
            let id = uuid::Uuid::new_v4().simple().to_string();
            let name = root
                .rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or(root);
            let (next_pos,): (i64,) = sqlx::query_as(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM projects",
            )
            .fetch_one(&self.pool)
            .await?;
            sqlx::query("INSERT INTO projects (id, name, position) VALUES (?1, ?2, ?3)")
                .bind(&id)
                .bind(name)
                .bind(next_pos)
                .execute(&self.pool)
                .await?;
            sqlx::query("INSERT INTO project_roots (project_id, path) VALUES (?1, ?2)")
                .bind(&id)
                .bind(root)
                .execute(&self.pool)
                .await?;
            sqlx::query("UPDATE sessions SET project_id = ?1 WHERE project_root = ?2")
                .bind(&id)
                .bind(root)
                .execute(&self.pool)
                .await?;
        }
        if !roots.is_empty() {
            tracing::info!(
                count = roots.len(),
                "migrated project_root strings to project entities"
            );
        }
        Ok(())
    }

    pub(crate) async fn stamp_schema_version(&self) -> Result<()> {
        sqlx::query("DELETE FROM schema_version")
            .execute(&self.pool)
            .await?;
        sqlx::query("INSERT INTO schema_version (version) VALUES (?1)")
            .bind(SCHEMA_VERSION)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn table_exists(&self, name: &str) -> Result<bool> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        )
        .bind(name)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    pub(crate) async fn column_exists(&self, table: &str, column: &str) -> Result<bool> {
        let sql = format!("SELECT name FROM pragma_table_info('{table}')");
        let rows: Vec<(String,)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().any(|(name,)| name == column))
    }

    pub(crate) async fn drop_messages_fts_objects(&self) -> Result<()> {
        sqlx::raw_sql(
            "DROP TRIGGER IF EXISTS sync_messages_to_fts;
             DROP TRIGGER IF EXISTS sync_messages_fts_update;
             DROP TRIGGER IF EXISTS sync_messages_fts_delete;
             DROP TRIGGER IF EXISTS messages_fts_insert;
             DROP TRIGGER IF EXISTS messages_fts_delete;
             DROP TRIGGER IF EXISTS messages_fts_update;
             DROP TABLE IF EXISTS messages_fts;
             DROP TABLE IF EXISTS messages_fts_trigram;",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn rebuild_messages_fts_v11(&self) -> Result<()> {
        self.drop_messages_fts_objects().await?;
        sqlx::raw_sql(MESSAGES_FTS_V11_DDL)
            .execute(&self.pool)
            .await?;
        sqlx::raw_sql(
            "INSERT INTO messages_fts(rowid, content, tool_name, tool_calls)
             SELECT id, content, tool_name, tool_calls FROM messages;
             INSERT INTO messages_fts_trigram(rowid, content, tool_name, tool_calls)
             SELECT id, content, tool_name, tool_calls FROM messages;",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 为仅存在于 `messages` 的 `session_id` 补齐 `sessions` 行（幂等）。
    pub(crate) async fn backfill_sessions_from_messages(&self) -> Result<()> {
        sqlx::query(
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
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 检测并重建失效的 `messages_fts` 触发器（坏触发器 / 错误 delete 语法）。
    pub(crate) async fn repair_messages_fts_if_needed(&self) -> Result<()> {
        if !self.needs_messages_fts_repair().await? {
            return Ok(());
        }
        self.rebuild_messages_fts_v11()
            .await
            .context("repair messages_fts triggers")?;
        Ok(())
    }

    pub(crate) async fn needs_messages_fts_repair(&self) -> Result<bool> {
        let (broken,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'trigger'
               AND name IN (
                 'sync_messages_to_fts',
                 'sync_messages_fts_update',
                 'sync_messages_fts_delete'
               )",
        )
        .fetch_one(&self.pool)
        .await?;
        if broken > 0 {
            return Ok(true);
        }

        let delete_sql: Option<(String,)> = sqlx::query_as(
            "SELECT sql FROM sqlite_master
             WHERE type = 'trigger' AND name = 'messages_fts_delete'",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(delete_sql
            .as_ref()
            .is_some_and(|(sql,)| sql.contains("'delete'")))
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

/// 去掉用户消息末尾旧版 `chatModeHint`（`\n\n---\n[Mode: Agent|Plan|Ask|MultiTask]…`）。
///
/// 无匹配时返回 `None`（调用方保留原文）。不触碰同形态的 Agent 创建提示等其它 `---` 段。
pub(crate) fn strip_legacy_chat_mode_hint(content: &str) -> Option<String> {
    const MARKER: &str = "\n\n---\n[Mode: ";
    let idx = content.rfind(MARKER)?;
    let rest = &content[idx + MARKER.len()..];
    let mode_ok = rest.starts_with("Agent]")
        || rest.starts_with("Plan]")
        || rest.starts_with("Ask]")
        || rest.starts_with("MultiTask]");
    if !mode_ok {
        return None;
    }
    Some(content[..idx].to_string())
}

#[cfg(test)]
mod strip_hint_tests {
    use super::strip_legacy_chat_mode_hint;

    #[test]
    fn strips_agent_plan_ask_multitask_suffix() {
        for mode in ["Agent", "Plan", "Ask", "MultiTask"] {
            let raw = format!("hello world\n\n---\n[Mode: {mode}] tools enabled blah");
            assert_eq!(
                strip_legacy_chat_mode_hint(&raw).as_deref(),
                Some("hello world"),
                "mode={mode}"
            );
        }
    }

    #[test]
    fn leaves_unrelated_separator_alone() {
        let raw = "body\n\n---\n创建 Agent 提示（非 Mode）";
        assert_eq!(strip_legacy_chat_mode_hint(raw), None);
    }

    #[test]
    fn leaves_clean_user_text_alone() {
        assert_eq!(strip_legacy_chat_mode_hint("just a question"), None);
    }
}
