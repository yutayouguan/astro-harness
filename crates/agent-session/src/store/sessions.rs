//! 会话元数据 CRUD。

use agent_db::sqlx::{self, Row};
use anyhow::Result;

use super::{
    is_unique_constraint, now_epoch_secs, truncate_chars, BillingDelta, SessionBillingRow,
    SessionStore, StoredSession,
};

impl SessionStore {
    /// 按 id 读取会话元数据。
    pub async fn get_session(&self, id: &str) -> Result<Option<StoredSession>> {
        let row = sqlx::query(
            "SELECT id, source, title, started_at, ended_at, end_reason,
                    model, parent_session_id, message_count, tool_call_count,
                    archived_at, pinned_at,
                    branch_kind, branch_parent_message_id,
                    branch_parent_turn_index, branch_inherited_turn_count,
                    branch_created_at
             FROM sessions WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| StoredSession {
            id: r.get(0),
            source: r.get(1),
            title: r.get(2),
            started_at: r.get(3),
            ended_at: r.get(4),
            end_reason: r.get(5),
            model: r.get(6),
            parent_session_id: r.get(7),
            message_count: r.get(8),
            tool_call_count: r.get(9),
            archived_at: r.get(10),
            pinned_at: r.get(11),
            branch_kind: r.get(12),
            branch_parent_message_id: r.get(13),
            branch_parent_turn_index: r.get(14),
            branch_inherited_turn_count: r.get(15),
            branch_created_at: r.get(16),
        }))
    }

    /// 插入一条会话元数据（最小字段集）。
    pub async fn create_session(
        &self,
        id: &str,
        source: &str,
        model: Option<&str>,
        user_id: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> Result<()> {
        let started_at = now_epoch_secs()?;
        sqlx::query(
            "INSERT INTO sessions (
                id, source, model, user_id, parent_session_id, started_at,
                branch_kind, branch_inherited_turn_count, branch_created_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                CASE WHEN ?5 IS NULL THEN NULL ELSE 'fork' END,
                CASE WHEN ?5 IS NULL THEN NULL ELSE 0 END,
                CASE WHEN ?5 IS NULL THEN NULL ELSE ?6 END
             )",
        )
        .bind(id)
        .bind(source)
        .bind(model)
        .bind(user_id)
        .bind(parent_session_id)
        .bind(started_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 若会话不存在则创建（最小字段）；已存在则 noop（`ON CONFLICT DO NOTHING`，可并发调用）。
    pub async fn ensure_session(&self, id: &str, source: &str) -> Result<()> {
        let started_at = now_epoch_secs()?;
        sqlx::query(
            "INSERT INTO sessions (id, source, started_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(id)
        .bind(source)
        .bind(started_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 设置会话的项目根目录；用于按项目过滤会话列表。
    pub async fn set_session_project_root(
        &self,
        id: &str,
        project_root: Option<&str>,
    ) -> Result<()> {
        sqlx::query("UPDATE sessions SET project_root = ?1 WHERE id = ?2")
            .bind(project_root)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 标记会话结束；会话须存在。重复调用会覆盖 ended_at / end_reason。
    pub async fn end_session(&self, id: &str, end_reason: &str) -> Result<()> {
        if self.get_session(id).await?.is_none() {
            anyhow::bail!("end_session: session not found");
        }
        let ended_at = now_epoch_secs()?;
        sqlx::query("UPDATE sessions SET ended_at = ?1, end_reason = ?2 WHERE id = ?3")
            .bind(ended_at)
            .bind(end_reason)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 会话存在且未结束。
    pub async fn assert_session_writable(&self, id: &str) -> Result<()> {
        let Some(s) = self.get_session(id).await? else {
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
    pub async fn set_session_title(&self, id: &str, title: &str) -> Result<()> {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            sqlx::query("UPDATE sessions SET title = NULL WHERE id = ?1")
                .bind(id)
                .execute(&self.pool)
                .await?;
            return Ok(());
        }

        let result = sqlx::query("UPDATE sessions SET title = ?1 WHERE id = ?2")
            .bind(trimmed)
            .bind(id)
            .execute(&self.pool)
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(err) if is_unique_constraint(&err) => {
                let suffix: String = id.chars().take(8).collect();
                let unique = format!("{} · {}", truncate_chars(trimmed, 60), suffix);
                sqlx::query("UPDATE sessions SET title = ?1 WHERE id = ?2")
                    .bind(unique)
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }
            Err(err) => Err(err.into()),
        }
    }

    /// 标记会话已归档。
    pub async fn archive_session(&self, id: &str) -> Result<()> {
        let result = sqlx::query("UPDATE sessions SET archived_at = ?1 WHERE id = ?2")
            .bind(now_epoch_secs()?)
            .bind(id)
            .execute(&self.pool)
            .await?;
        anyhow::ensure!(
            result.rows_affected() == 1,
            "archive_session: session not found"
        );
        Ok(())
    }

    /// 取消会话归档。
    pub async fn unarchive_session(&self, id: &str) -> Result<()> {
        let result = sqlx::query("UPDATE sessions SET archived_at = NULL WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        anyhow::ensure!(
            result.rows_affected() == 1,
            "unarchive_session: session not found"
        );
        Ok(())
    }

    /// 置顶会话。
    pub async fn pin_session(&self, id: &str) -> Result<()> {
        let result = sqlx::query("UPDATE sessions SET pinned_at = ?1 WHERE id = ?2")
            .bind(now_epoch_secs()?)
            .bind(id)
            .execute(&self.pool)
            .await?;
        anyhow::ensure!(
            result.rows_affected() == 1,
            "pin_session: session not found"
        );
        Ok(())
    }

    /// 取消置顶。
    pub async fn unpin_session(&self, id: &str) -> Result<()> {
        let result = sqlx::query("UPDATE sessions SET pinned_at = NULL WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        anyhow::ensure!(
            result.rows_affected() == 1,
            "unpin_session: session not found"
        );
        Ok(())
    }

    /// 仅在标题为空时设置标题。
    pub async fn set_session_title_if_empty(&self, id: &str, title: &str) -> Result<bool> {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return Ok(false);
        }
        let result = sqlx::query(
            "UPDATE sessions
             SET title = ?1
             WHERE id = ?2 AND (title IS NULL OR TRIM(title) = '')",
        )
        .bind(trimmed)
        .bind(id)
        .execute(&self.pool)
        .await;
        match result {
            Ok(r) => Ok(r.rows_affected() == 1),
            Err(err) if is_unique_constraint(&err) => {
                let suffix: String = id.chars().take(8).collect();
                let unique = format!("{} · {}", truncate_chars(trimmed, 60), suffix);
                let r = sqlx::query(
                    "UPDATE sessions
                     SET title = ?1
                     WHERE id = ?2 AND (title IS NULL OR TRIM(title) = '')",
                )
                .bind(unique)
                .bind(id)
                .execute(&self.pool)
                .await?;
                Ok(r.rows_affected() == 1)
            }
            Err(err) => Err(err.into()),
        }
    }

    /// 永久删除会话与其 Responses items。
    ///
    /// 若存在以本会话为 `parent_session_id` 的分支，先断开引用再删，
    /// 避免外键拦住父会话删除（子分支会话本身保留）。
    pub async fn delete_session_permanently(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE sessions SET parent_session_id = NULL WHERE parent_session_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM response_items WHERE session_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let result = sqlx::query("DELETE FROM sessions WHERE id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        anyhow::ensure!(
            result.rows_affected() == 1,
            "delete_session_permanently: session not found"
        );
        tx.commit().await?;
        Ok(())
    }

    /// 按 item 顺序读取最早可完成的非空 user → assistant 文本配对。
    pub async fn first_turn_text(&self, session_id: &str) -> Result<Option<(String, String)>> {
        let rows = sqlx::query(
            "SELECT role, search_text
             FROM response_items
             WHERE session_id = ?1
               AND role IN ('user', 'assistant')
               AND TRIM(search_text) != ''
             ORDER BY timestamp ASC, id ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        let mut candidate_user = None;
        for row in &rows {
            let role: String = row.get(0);
            let content: String = row.get(1);
            match role.as_str() {
                "user" => candidate_user = Some(content),
                "assistant" => {
                    if let Some(user) = candidate_user.take() {
                        return Ok(Some((user, content)));
                    }
                }
                _ => {}
            }
        }
        Ok(None)
    }

    /// 累加会话账单列；`cost_status=unknown` 的 delta 不抬高 `estimated_cost_usd`。
    pub async fn update_session_billing(&self, id: &str, d: BillingDelta) -> Result<()> {
        let skip_cost = d.cost_status.as_deref() == Some("unknown");
        let cost_add = if skip_cost { 0.0 } else { d.estimated_cost_usd };
        sqlx::query(
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
        )
        .bind(d.input_tokens)
        .bind(d.output_tokens)
        .bind(d.cache_read_tokens)
        .bind(d.cache_write_tokens)
        .bind(d.reasoning_tokens)
        .bind(if skip_cost { 1i64 } else { 0i64 })
        .bind(cost_add)
        .bind(d.api_call_count)
        .bind(d.billing_provider)
        .bind(d.billing_base_url)
        .bind(d.billing_mode)
        .bind(d.cost_status)
        .bind(d.cost_source)
        .bind(d.pricing_version)
        .bind(d.model)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 读取会话账单列；无此 session 时返回 `None`。
    pub async fn get_session_billing(&self, id: &str) -> Result<Option<SessionBillingRow>> {
        let row = sqlx::query(
            "SELECT COALESCE(input_tokens, 0), COALESCE(output_tokens, 0),
                    COALESCE(cache_read_tokens, 0), COALESCE(cache_write_tokens, 0),
                    COALESCE(reasoning_tokens, 0), COALESCE(api_call_count, 0),
                    COALESCE(estimated_cost_usd, 0), actual_cost_usd,
                    cost_status, cost_source, pricing_version,
                    billing_provider, billing_base_url, billing_mode, model
             FROM sessions WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| SessionBillingRow {
            input_tokens: r.get(0),
            output_tokens: r.get(1),
            cache_read_tokens: r.get(2),
            cache_write_tokens: r.get(3),
            reasoning_tokens: r.get(4),
            api_call_count: r.get(5),
            estimated_cost_usd: r.get(6),
            actual_cost_usd: r.get(7),
            cost_status: r.get(8),
            cost_source: r.get(9),
            pricing_version: r.get(10),
            billing_provider: r.get(11),
            billing_base_url: r.get(12),
            billing_mode: r.get(13),
            model: r.get(14),
        }))
    }
}
