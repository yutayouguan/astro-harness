//! 会话元数据 CRUD。

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::{is_unique_constraint, now_epoch_secs, truncate_chars, BillingDelta, SessionBillingRow, SessionStore, StoredSession};

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

    /// 标记会话结束（幂等：已 ended 且 reason 相同则 Ok）。
    pub fn end_session(&self, id: &str, end_reason: &str) -> Result<()> {
        if self.get_session(id)?.is_none() {
            anyhow::bail!("end_session: session not found");
        }
        let ended_at = now_epoch_secs()?;
        self.conn.execute(
            "UPDATE sessions SET ended_at = ?1, end_reason = ?2 WHERE id = ?3",
            params![ended_at, end_reason, id],
        )?;
        Ok(())
    }

    /// 会话存在且未结束。
    pub fn assert_session_writable(&self, id: &str) -> Result<()> {
        let Some(s) = self.get_session(id)? else {
            anyhow::bail!("assert_session_writable: session not found");
        };
        if s.ended_at.is_some() {
            anyhow::bail!(
                "assert_session_writable: session ended ({})",
                s.end_reason.unwrap_or_else(|| "unknown".into())
            );
        }
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

    /// 累加会话账单列；`cost_status=unknown` 的 delta 不抬高 `estimated_cost_usd`。
    pub fn update_session_billing(&self, id: &str, d: BillingDelta) -> Result<()> {
        let skip_cost = d.cost_status.as_deref() == Some("unknown");
        let cost_add = if skip_cost {
            0.0
        } else {
            d.estimated_cost_usd
        };
        self.conn.execute(
            "UPDATE sessions SET
                input_tokens = COALESCE(input_tokens, 0) + ?1,
                output_tokens = COALESCE(output_tokens, 0) + ?2,
                cache_read_tokens = COALESCE(cache_read_tokens, 0) + ?3,
                cache_write_tokens = COALESCE(cache_write_tokens, 0) + ?4,
                reasoning_tokens = COALESCE(reasoning_tokens, 0) + ?5,
                estimated_cost_usd = CASE
                    WHEN ?6 = 1 THEN estimated_cost_usd
                    ELSE COALESCE(estimated_cost_usd, 0) + ?7
                END,
                api_call_count = COALESCE(api_call_count, 0) + ?8,
                billing_provider = COALESCE(?9, billing_provider),
                billing_base_url = COALESCE(?10, billing_base_url),
                billing_mode = COALESCE(?11, billing_mode),
                cost_status = CASE
                    WHEN cost_status = 'unknown' OR ?12 = 'unknown' THEN 'unknown'
                    ELSE COALESCE(?12, cost_status)
                END,
                cost_source = COALESCE(?13, cost_source),
                pricing_version = COALESCE(?14, pricing_version),
                model = COALESCE(?15, model)
             WHERE id = ?16",
            params![
                d.input_tokens,
                d.output_tokens,
                d.cache_read_tokens,
                d.cache_write_tokens,
                d.reasoning_tokens,
                if skip_cost { 1i64 } else { 0i64 },
                cost_add,
                d.api_call_count,
                d.billing_provider,
                d.billing_base_url,
                d.billing_mode,
                d.cost_status,
                d.cost_source,
                d.pricing_version,
                d.model,
                id,
            ],
        )?;
        Ok(())
    }

    /// 读取会话账单列；无此 session 时返回 `None`。
    pub fn get_session_billing(&self, id: &str) -> Result<Option<SessionBillingRow>> {
        self.conn
            .query_row(
                "SELECT COALESCE(input_tokens, 0), COALESCE(output_tokens, 0),
                        COALESCE(cache_read_tokens, 0), COALESCE(cache_write_tokens, 0),
                        COALESCE(reasoning_tokens, 0), COALESCE(api_call_count, 0),
                        COALESCE(estimated_cost_usd, 0), actual_cost_usd,
                        cost_status, cost_source, pricing_version,
                        billing_provider, billing_base_url, billing_mode, model
                 FROM sessions WHERE id = ?1",
                params![id],
                |row| {
                    Ok(SessionBillingRow {
                        input_tokens: row.get(0)?,
                        output_tokens: row.get(1)?,
                        cache_read_tokens: row.get(2)?,
                        cache_write_tokens: row.get(3)?,
                        reasoning_tokens: row.get(4)?,
                        api_call_count: row.get(5)?,
                        estimated_cost_usd: row.get(6)?,
                        actual_cost_usd: row.get(7)?,
                        cost_status: row.get(8)?,
                        cost_source: row.get(9)?,
                        pricing_version: row.get(10)?,
                        billing_provider: row.get(11)?,
                        billing_base_url: row.get(12)?,
                        billing_mode: row.get(13)?,
                        model: row.get(14)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }
}
