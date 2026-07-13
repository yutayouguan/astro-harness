//! 会话元数据 CRUD。

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::{is_unique_constraint, now_epoch_secs, truncate_chars, SessionStore, StoredSession};

impl SessionStore {
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

    /// 若会话不存在则创建（最小字段）；已存在则 noop（`ON CONFLICT DO NOTHING`，可并发调用）。
    pub fn ensure_session(&self, id: &str, source: &str) -> Result<()> {
        let started_at = now_epoch_secs()?;
        self.conn.execute(
            "INSERT INTO sessions (id, source, started_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO NOTHING",
            params![id, source, started_at],
        )?;
        Ok(())
    }

    /// 设置会话标题；空字符串清为 `NULL`。
    ///
    /// 若命中 `title` 唯一索引，则追加短 session id 后缀以保证可写入。
    pub fn set_session_title(&self, id: &str, title: &str) -> Result<()> {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            self.conn
                .execute("UPDATE sessions SET title = NULL WHERE id = ?1", params![id])?;
            return Ok(());
        }

        let result = self.conn.execute(
            "UPDATE sessions SET title = ?1 WHERE id = ?2",
            params![trimmed, id],
        );
        match result {
            Ok(_) => Ok(()),
            Err(err) if is_unique_constraint(&err) => {
                let suffix: String = id.chars().take(8).collect();
                let unique = format!("{} · {}", truncate_chars(trimmed, 60), suffix);
                self.conn.execute(
                    "UPDATE sessions SET title = ?1 WHERE id = ?2",
                    params![unique, id],
                )?;
                Ok(())
            }
            Err(err) => Err(err.into()),
        }
    }
}
