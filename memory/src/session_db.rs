//! 会话摘要的 SQLite 持久化与 FTS5 全文检索。
//!
//! 每条会话以 `session_id` 为主键存储摘要文本；虚拟 FTS 表通过触发器与主表同步。
//! 供 `session_search` 工具按关键词召回历史对话片段。

use rusqlite::{params, Connection};
use std::path::PathBuf;

/// 会话表、FTS 虚拟表及同步触发器的 DDL；在 [`SessionDb::new`] 时批量执行。
const SESSIONS_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    summary TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE VIRTUAL TABLE IF NOT EXISTS sessions_fts USING fts5(
    session_id UNINDEXED,
    summary
);

CREATE TRIGGER IF NOT EXISTS sessions_ai AFTER INSERT ON sessions BEGIN
    INSERT INTO sessions_fts(session_id, summary) VALUES (new.session_id, new.summary);
END;

CREATE TRIGGER IF NOT EXISTS sessions_au AFTER UPDATE ON sessions BEGIN
    UPDATE sessions_fts SET summary = new.summary WHERE session_id = old.session_id;
END;

CREATE TRIGGER IF NOT EXISTS sessions_ad AFTER DELETE ON sessions BEGIN
    DELETE FROM sessions_fts WHERE session_id = old.session_id;
END;
"#;

/// FTS 检索或列表查询返回的会话摘要片段。
#[derive(Debug, Clone)]
pub struct SessionSnippet {
    /// 会话唯一标识。
    pub session_id: String,
    /// 完整摘要正文。
    pub summary: String,
    /// 创建时间（ISO 8601 字符串）；旧数据可能为 `None`。
    pub created_at: Option<String>,
    /// FTS `snippet` 高亮片段；非检索路径（如 `list_recent`）为 `None`。
    pub highlight: Option<String>,
}

/// 基于 SQLite 的会话摘要存储，内置 FTS5 全文索引。
pub struct SessionDb {
    /// 底层数据库连接；生命周期与 `SessionDb` 绑定。
    conn: Connection,
}

impl SessionDb {
    /// 打开或创建会话数据库，自动建表并启用 WAL。
    ///
    /// 父目录不存在时会递归创建。
    pub fn new(path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SESSIONS_DDL)?;
        Ok(Self { conn })
    }

    /// 插入或更新会话摘要；冲突时覆盖 `summary` 并刷新 `updated_at`。
    ///
    /// FTS 索引由触发器自动维护。
    pub fn save_session(&self, session_id: &str, summary: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (session_id, summary)
             VALUES (?1, ?2)
             ON CONFLICT(session_id) DO UPDATE SET
                 summary = excluded.summary,
                 updated_at = datetime('now')",
            params![session_id, summary],
        )?;
        Ok(())
    }

    /// 按 FTS 关键词检索会话摘要，按相关度排序，最多 10 条。
    ///
    /// 空查询回退为 [`list_recent`](Self::list_recent)(10)，避免 FTS5 `MATCH ""` 语法错误。
    pub fn search(&self, query: &str) -> anyhow::Result<Vec<SessionSnippet>> {
        let query = query.trim();
        // 空查询：返回最近会话，避免 FTS5 MATCH "" 语法错误
        if query.is_empty() {
            return self.list_recent(10);
        }

        let mut stmt = self.conn.prepare(
            "SELECT s.session_id, s.summary, s.created_at,
                    snippet(sessions_fts, 1, '[', ']', '...', 32) AS highlight
             FROM sessions_fts
             JOIN sessions s ON s.session_id = sessions_fts.session_id
             WHERE sessions_fts MATCH ?1
             ORDER BY rank
             LIMIT 10",
        )?;
        let results = stmt
            .query_map(params![query], |row| {
                Ok(SessionSnippet {
                    session_id: row.get(0)?,
                    summary: row.get(1)?,
                    created_at: row.get(2)?,
                    highlight: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(results)
    }

    /// 按 `updated_at` 降序列出最近 `limit` 条会话；无 FTS 高亮。
    pub fn list_recent(&self, limit: usize) -> anyhow::Result<Vec<SessionSnippet>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, summary, created_at
             FROM sessions
             ORDER BY updated_at DESC
             LIMIT ?1",
        )?;
        let results = stmt
            .query_map(params![limit as i64], |row| {
                Ok(SessionSnippet {
                    session_id: row.get(0)?,
                    summary: row.get(1)?,
                    created_at: row.get(2)?,
                    highlight: None,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(results)
    }
}
