//! 会话消息的 SQLite 持久化与 FTS5 全文检索。
//!
//! 消息表与虚拟 FTS 表通过触发器保持同步；WAL 模式提升并发读性能。
//! 供 `build_conversation_context` 组合「最近轮次 + 关键词召回窗口」构建对话上下文。

use rusqlite::{params, Connection};
use std::path::PathBuf;

/// 消息表、FTS 虚拟表及同步触发器的 DDL；在 `MessageDb::new` 时批量执行。
const MESSAGES_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    message_id UNINDEXED,
    content,
    tokenize='unicode61'
);

CREATE TRIGGER IF NOT EXISTS sync_messages_to_fts AFTER INSERT ON messages
BEGIN
    INSERT INTO messages_fts(message_id, content) VALUES (new.id, new.content);
END;

CREATE TRIGGER IF NOT EXISTS sync_messages_fts_update AFTER UPDATE ON messages
BEGIN
    UPDATE messages_fts SET content = new.content WHERE message_id = old.id;
END;

CREATE TRIGGER IF NOT EXISTS sync_messages_fts_delete AFTER DELETE ON messages
BEGIN
    DELETE FROM messages_fts WHERE message_id = old.id;
END;
"#;

/// 对话上下文中的单条消息，可标记为 FTS 召回的锚点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrolledMessage {
    /// 消息主键，与 `messages.id` 一致。
    pub id: i64,
    /// 角色标识（如 `user`、`assistant`）。
    pub role: String,
    /// 消息正文。
    pub content: String,
    /// 是否为 FTS 召回窗口的中心锚点消息。
    pub is_anchor: bool,
}

/// 基于 SQLite 的会话消息存储，内置 FTS5 全文索引。
pub struct MessageDb {
    /// 底层数据库连接；生命周期与 `MessageDb` 绑定。
    conn: Connection,
}

impl MessageDb {
    /// 打开或创建消息数据库，自动建表并启用 WAL。
    ///
    /// 父目录不存在时会递归创建。
    pub fn new(path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(MESSAGES_DDL)?;
        Ok(Self { conn })
    }

    /// 插入一条消息并返回自增 `id`；FTS 索引由触发器自动更新。
    pub fn insert_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
    ) -> anyhow::Result<i64> {
        self.conn.execute(
            "INSERT INTO messages (session_id, role, content) VALUES (?1, ?2, ?3)",
            params![session_id, role, content],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 在指定会话内按 FTS 查询召回消息 id 列表，按相关度排序。
    ///
    /// 空查询直接返回空列表，不执行 MATCH。
    pub fn recall_message_ids(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<i64>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let mut stmt = self.conn.prepare(
            "SELECT fts.message_id
             FROM messages_fts fts
             JOIN messages m ON fts.message_id = m.id
             WHERE m.session_id = ?1 AND messages_fts MATCH ?2
             ORDER BY rank
             LIMIT ?3",
        )?;
        let ids = stmt
            .query_map(params![session_id, query, limit as i64], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// 以 `around_message_id` 为中心，取前后各 `window_size` 条消息（含中心），按 id 升序。
    pub fn scroll_context_window(
        &self,
        session_id: &str,
        around_message_id: i64,
        window_size: i64,
    ) -> anyhow::Result<Vec<ScrolledMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, role, content
             FROM messages
             WHERE session_id = ?1
               AND id BETWEEN (?2 - ?3) AND (?2 + ?3)
             ORDER BY id ASC",
        )?;
        let rows = stmt
            .query_map(
                params![session_id, around_message_id, window_size],
                |row| {
                    let id: i64 = row.get(0)?;
                    Ok(ScrolledMessage {
                        id,
                        role: row.get(1)?,
                        content: row.get(2)?,
                        is_anchor: id == around_message_id,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 取指定会话最近 `limit` 条消息，按时间正序返回（`is_anchor` 均为 false）。
    pub fn recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<ScrolledMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, role, content
             FROM messages
             WHERE session_id = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let mut rows: Vec<ScrolledMessage> = stmt
            .query_map(params![session_id, limit as i64], |row| {
                let id: i64 = row.get(0)?;
                Ok(ScrolledMessage {
                    id,
                    role: row.get(1)?,
                    content: row.get(2)?,
                    is_anchor: false,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// 返回全局最近一条消息所属的 `session_id`，用于恢复上次对话；无消息时返回 `None`。
    pub fn latest_session_id(&self) -> anyhow::Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT session_id FROM messages ORDER BY id DESC LIMIT 1")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }
}

/// 构建会话上下文：先取最近 `recent_turns` 条，再按 FTS 关键词各召回最多 3 个锚点及其窗口。
///
/// 合并后按 `id` 排序并去重。`fts_keywords` 为 `None` 时仅返回最近消息。
pub fn build_conversation_context(
    db: &MessageDb,
    session_id: &str,
    recent_turns: usize,
    fts_keywords: Option<&str>,
) -> anyhow::Result<Vec<ScrolledMessage>> {
    let mut context = db.recent_messages(session_id, recent_turns)?;

    if let Some(keywords) = fts_keywords {
        for anchor_id in db.recall_message_ids(session_id, keywords, 3)? {
            let window = db.scroll_context_window(session_id, anchor_id, 5)?;
            context.extend(window);
        }
        context.sort_by_key(|m| m.id);
        context.dedup_by_key(|m| m.id);
    }

    Ok(context)
}
