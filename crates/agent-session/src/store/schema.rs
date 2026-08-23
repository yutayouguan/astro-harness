//! Schema 版本、DDL、增量迁移与 FTS 自愈。

use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};

use super::SessionStore;

pub const SCHEMA_VERSION: i32 = 19;

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
    /// 空库建表并 stamp；旧库由 [`SessionStore::open`] 调用增量迁移。
    ///
    /// 即便 `schema_version` 已到目标，仍幂等补齐缺列：合并分支可能先 stamp
    /// 了 v14（如 `archived_at`）却未加 `compressed_content`。
    pub(crate) fn migrate_schema(&self) -> Result<()> {
        let current = self.read_schema_version_or_zero()?;
        let tx = self.conn.unchecked_transaction()?;
        if current < SCHEMA_VERSION && !self.table_exists("messages")? {
            self.conn.execute_batch(SCHEMA_V11_DDL)?;
            self.conn.execute_batch(MESSAGES_FTS_V11_DDL)?;
        }

        // 必须先补齐列，再执行引用这些列的数据清洗；版本已到也要自愈半迁移库。
        if self.table_exists("messages")? {
            self.ensure_messages_compressed_content_column()?;
            self.ensure_messages_media_json_column()?;
        }
        if self.table_exists("sessions")? && !self.column_exists("sessions", "archived_at")? {
            self.conn
                .execute("ALTER TABLE sessions ADD COLUMN archived_at REAL", [])?;
        }
        if self.table_exists("sessions")? && !self.column_exists("sessions", "pinned_at")? {
            self.conn
                .execute("ALTER TABLE sessions ADD COLUMN pinned_at REAL", [])?;
        }
        if self.table_exists("projects")? && !self.column_exists("projects", "icon")? {
            self.conn
                .execute("ALTER TABLE projects ADD COLUMN icon TEXT", [])?;
        }

        if current < SCHEMA_VERSION {
            // v16→v17：删除历史用户消息末尾的 `chatModeHint`。
            if (1..17).contains(&current) && self.table_exists("messages")? {
                self.strip_legacy_chat_mode_hints()
                    .context("strip legacy chatModeHint from user messages")?;
            }
            // v17→v18：sessions 表加 project_root 列，用于按项目过滤会话。
            if self.table_exists("sessions")? && !self.column_exists("sessions", "project_root")? {
                self.conn
                    .execute("ALTER TABLE sessions ADD COLUMN project_root TEXT", [])?;
            }
            // v18→v19：引入 projects / project_roots 实体表，替代 sessions.project_root 字符串。
            if !self.table_exists("projects")? {
                self.conn.execute_batch(
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
                )?;
            }
            if self.table_exists("sessions")? && !self.column_exists("sessions", "project_id")? {
                self.conn.execute(
                    "ALTER TABLE sessions ADD COLUMN project_id TEXT REFERENCES projects(id)",
                    [],
                )?;
                // 迁移已有 project_root → projects 实体
                self.migrate_project_root_to_projects()?;
            }
            self.stamp_schema_version()?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 剥离用户消息末尾遗留的 `\n\n---\n[Mode: …]`（旧 `chatModeHint`）。
    ///
    /// UPDATE 会触发 FTS 同步；仅改写带该后缀的 user 行。
    pub(crate) fn strip_legacy_chat_mode_hints(&self) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, compressed_content FROM messages
             WHERE role = 'user'
               AND (
                 content LIKE '%' || char(10) || char(10) || '---' || char(10) || '[Mode: %'
                 OR compressed_content LIKE '%' || char(10) || char(10) || '---' || char(10) || '[Mode: %'
               )",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

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
            self.conn.execute(
                "UPDATE messages SET content = ?1, compressed_content = ?2 WHERE id = ?3",
                params![new_content, new_compressed, id],
            )?;
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

    pub(crate) fn ensure_messages_compressed_content_column(&self) -> Result<()> {
        let exists: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('messages') WHERE name = 'compressed_content'",
            [],
            |row| row.get(0),
        )?;
        if !exists {
            self.conn.execute(
                "ALTER TABLE messages ADD COLUMN compressed_content TEXT",
                [],
            )?;
        }
        Ok(())
    }

    pub(crate) fn ensure_messages_media_json_column(&self) -> Result<()> {
        let exists: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('messages') WHERE name = 'media_json'",
            [],
            |row| row.get(0),
        )?;
        if !exists {
            self.conn
                .execute("ALTER TABLE messages ADD COLUMN media_json TEXT", [])?;
        }
        Ok(())
    }

    /// v18→v19 数据迁移：将 `sessions.project_root` 去重后创建 `projects` 实体，
    /// 并将 `sessions.project_id` 指向对应 project。
    pub(crate) fn migrate_project_root_to_projects(&self) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT project_root FROM sessions
             WHERE project_root IS NOT NULL AND TRIM(project_root) != ''",
        )?;
        let roots: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        for root in &roots {
            let id = uuid::Uuid::new_v4().simple().to_string();
            let name = root.rsplit('/').find(|s| !s.is_empty()).unwrap_or(root);
            // 取当前最大 position + 1
            let next_pos: i64 = self.conn.query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM projects",
                [],
                |row| row.get(0),
            )?;
            self.conn.execute(
                "INSERT INTO projects (id, name, position) VALUES (?1, ?2, ?3)",
                params![id, name, next_pos],
            )?;
            self.conn.execute(
                "INSERT INTO project_roots (project_id, path) VALUES (?1, ?2)",
                params![id, root],
            )?;
            self.conn.execute(
                "UPDATE sessions SET project_id = ?1 WHERE project_root = ?2",
                params![id, root],
            )?;
        }
        if !roots.is_empty() {
            tracing::info!(
                count = roots.len(),
                "migrated project_root strings to project entities"
            );
        }
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

    pub(crate) fn column_exists(&self, table: &str, column: &str) -> Result<bool> {
        let sql = format!("PRAGMA table_info({table})");
        let mut stmt = self.conn.prepare(&sql)?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(names.iter().any(|name| name == column))
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
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(version.unwrap_or(0))
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
