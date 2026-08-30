//! 富消息读写。

use agent_db::sqlx::{self, Row};
use anyhow::Result;
use serde_json::Value;

use super::{
    json_from_db, json_to_db, now_epoch_secs, truncate_chars, BranchKind, NewMessage, SessionStore,
    StoredMessage,
};

pub(crate) async fn insert_message_row(
    executor: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    msg: NewMessage<'_>,
    timestamp: f64,
) -> Result<i64> {
    let tool_calls = json_to_db(&msg.tool_calls)?;
    let reasoning_details = json_to_db(&msg.reasoning_details)?;
    let reasoning_items = json_to_db(&msg.reasoning_items)?;
    let message_items = json_to_db(&msg.message_items)?;
    let result = sqlx::query(
        "INSERT INTO messages (
            session_id, role, content, compressed_content,
            tool_call_id, tool_calls, tool_name,
            timestamp, token_count, finish_reason,
            reasoning, reasoning_content, reasoning_details,
            reasoning_items, message_items, media_json
         ) VALUES (
            ?1, ?2, ?3, ?4,
            ?5, ?6, ?7,
            ?8, ?9, ?10,
            ?11, ?12, ?13,
            ?14, ?15, ?16
         )",
    )
    .bind(msg.session_id)
    .bind(msg.role)
    .bind(msg.content)
    .bind(msg.compressed_content)
    .bind(msg.tool_call_id)
    .bind(tool_calls)
    .bind(msg.tool_name)
    .bind(timestamp)
    .bind(msg.token_count)
    .bind(msg.finish_reason)
    .bind(msg.reasoning)
    .bind(msg.reasoning_content)
    .bind(reasoning_details)
    .bind(reasoning_items)
    .bind(message_items)
    .bind(msg.media_json)
    .execute(executor)
    .await?;
    Ok(result.last_insert_rowid())
}

impl SessionStore {
    /// 追加一条富消息，并递增 `sessions.message_count`（`role=tool` 时同时 `tool_call_count++`）。
    pub async fn append_message(&self, msg: NewMessage<'_>) -> Result<i64> {
        self.assert_session_writable(msg.session_id).await?;
        let session_id = msg.session_id;
        let is_tool = msg.role == "tool";
        let mut tx = self.pool.begin().await?;
        let id = insert_message_row(&mut *tx, msg, now_epoch_secs()?).await?;

        if is_tool {
            sqlx::query(
                "UPDATE sessions
                 SET message_count = message_count + 1,
                     tool_call_count = tool_call_count + 1
                 WHERE id = ?1",
            )
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query("UPDATE sessions SET message_count = message_count + 1 WHERE id = ?1")
                .bind(session_id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(id)
    }

    /// 按时间顺序读取会话内全部消息行。
    /// 跨全部会话取「带结构化媒体」的消息 `(session_id, message_id, media_json)`。
    ///
    /// 供 artifacts 回填：把历史生成文件关联回来源会话。单条查询，开销低。
    pub async fn media_messages(&self) -> Result<Vec<(String, i64, String)>> {
        let rows = sqlx::query(
            "SELECT session_id, id, media_json
             FROM messages
             WHERE media_json IS NOT NULL AND media_json != ''
             ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for row in rows {
            out.push((
                row.get::<String, _>(0),
                row.get::<i64, _>(1),
                row.get::<String, _>(2),
            ));
        }
        Ok(out)
    }

    pub async fn get_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        let rows = sqlx::query(
            "SELECT id, session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason,
                    reasoning, reasoning_content, reasoning_details,
                    reasoning_items, message_items, media_json
             FROM messages
             WHERE session_id = ?1
             ORDER BY timestamp ASC, id ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::new();
        for row in rows {
            let tool_calls_raw: Option<String> = row.get::<Option<String>, _>(6);
            let reasoning_details_raw: Option<String> = row.get::<Option<String>, _>(13);
            let reasoning_items_raw: Option<String> = row.get::<Option<String>, _>(14);
            let message_items_raw: Option<String> = row.get::<Option<String>, _>(15);
            out.push(StoredMessage {
                id: row.get::<i64, _>(0),
                session_id: row.get::<String, _>(1),
                role: row.get::<String, _>(2),
                content: row.get::<Option<String>, _>(3),
                compressed_content: row.get::<Option<String>, _>(4),
                tool_call_id: row.get::<Option<String>, _>(5),
                tool_calls: json_from_db(tool_calls_raw)?,
                tool_name: row.get::<Option<String>, _>(7),
                timestamp: row.get::<f64, _>(8),
                token_count: row.get::<Option<i64>, _>(9),
                finish_reason: row.get::<Option<String>, _>(10),
                reasoning: row.get::<Option<String>, _>(11),
                reasoning_content: row.get::<Option<String>, _>(12),
                reasoning_details: json_from_db(reasoning_details_raw)?,
                reasoning_items: json_from_db(reasoning_items_raw)?,
                message_items: json_from_db(message_items_raw)?,
                media_json: row.get::<Option<String>, _>(16),
            });
        }
        Ok(out)
    }

    /// 为指定消息写入 provider 视图或内部交付标记；原始 `content` 不变。
    pub async fn update_message_compressed_content(
        &self,
        message_id: i64,
        compressed_content: Option<&str>,
    ) -> Result<()> {
        sqlx::query("UPDATE messages SET compressed_content = ?1 WHERE id = ?2")
            .bind(compressed_content)
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 回写本会话最近一条 assistant 的 `reasoning_details`（保留其它键，覆盖 timeline/surfaces）。
    ///
    /// 工具循环在 assistant 落盘之后才会 `upsert_surface`；若不回写，历史恢复会丢 A2UI 卡片。
    pub async fn patch_last_assistant_reasoning_details(
        &self,
        session_id: &str,
        details: &Value,
    ) -> Result<()> {
        let existing: Option<String> = sqlx::query(
            "SELECT reasoning_details FROM messages
             WHERE session_id = ?1 AND role = 'assistant'
             ORDER BY id DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        .and_then(|row| row.get::<Option<String>, _>(0));
        let mut obj = match existing.as_deref().map(serde_json::from_str::<Value>) {
            Some(Ok(Value::Object(m))) => m,
            _ => serde_json::Map::new(),
        };
        if let Value::Object(patch) = details {
            for (k, v) in patch {
                obj.insert(k.clone(), v.clone());
            }
        }
        let json = serde_json::to_string(&Value::Object(obj))?;
        sqlx::query(
            "UPDATE messages SET reasoning_details = ?1
             WHERE id = (
               SELECT id FROM messages
               WHERE session_id = ?2 AND role = 'assistant'
               ORDER BY id DESC LIMIT 1
             )",
        )
        .bind(&json)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 将源会话消息复制到新会话（含 tool 行），截止到第 `keep_chat_bubbles` 个 user/assistant 气泡。
    ///
    /// 新会话写入 `parent_session_id = source_id`，便于谱系追溯。`keep_chat_bubbles == 0` 时仅创建空会话。
    pub async fn fork_session(
        &self,
        source_id: &str,
        new_id: &str,
        keep_chat_bubbles: usize,
    ) -> Result<()> {
        if source_id == new_id {
            anyhow::bail!("fork_session: source and target session ids must differ");
        }
        if self.get_session(new_id).await?.is_some() {
            anyhow::bail!("fork_session: target session already exists");
        }

        let parent = self.get_session(source_id).await?;
        let model = parent.as_ref().and_then(|p| p.model.clone());
        self.create_session(new_id, "tauri", model.as_deref(), None, Some(source_id))
            .await?;

        if keep_chat_bubbles == 0 {
            self.write_fork_metadata_from_messages(source_id, new_id, &[], BranchKind::Fork)
                .await?;
            return Ok(());
        }

        let messages = self.get_messages(source_id).await?;
        let Some(end) = end_inclusive_for_bubbles(&messages, keep_chat_bubbles) else {
            self.write_fork_metadata_from_messages(source_id, new_id, &messages, BranchKind::Fork)
                .await?;
            return Ok(());
        };

        let mut tx = self.pool.begin().await?;
        let mut message_count = 0i64;
        let mut tool_call_count = 0i64;
        for m in &messages[..=end] {
            let tool_calls = json_to_db(&m.tool_calls)?;
            let reasoning_details = json_to_db(&m.reasoning_details)?;
            let reasoning_items = json_to_db(&m.reasoning_items)?;
            let message_items = json_to_db(&m.message_items)?;
            sqlx::query(
                "INSERT INTO messages (
                    session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason,
                    reasoning, reasoning_content, reasoning_details,
                    reasoning_items, message_items, media_json
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                    ?8, ?9, ?10,
                    ?11, ?12, ?13,
                    ?14, ?15, ?16
                 )",
            )
            .bind(new_id)
            .bind(&m.role)
            .bind(&m.content)
            .bind(&m.compressed_content)
            .bind(&m.tool_call_id)
            .bind(&tool_calls)
            .bind(&m.tool_name)
            .bind(m.timestamp)
            .bind(m.token_count)
            .bind(&m.finish_reason)
            .bind(&m.reasoning)
            .bind(&m.reasoning_content)
            .bind(&reasoning_details)
            .bind(&reasoning_items)
            .bind(&message_items)
            .bind(&m.media_json)
            .execute(&mut *tx)
            .await?;
            message_count += 1;
            if m.role == "tool" {
                tool_call_count += 1;
            }
        }
        sqlx::query(
            "UPDATE sessions
             SET message_count = ?1, tool_call_count = ?2
             WHERE id = ?3",
        )
        .bind(message_count)
        .bind(tool_call_count)
        .bind(new_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        if let Some(title) = parent
            .and_then(|p| p.title)
            .filter(|t| !t.trim().is_empty())
        {
            let branched = format!("{title} · branch");
            let _ = self.set_session_title(new_id, &branched).await;
        }
        self.write_fork_metadata_from_messages(source_id, new_id, &messages[..=end], BranchKind::Fork)
            .await?;

        Ok(())
    }

    /// 从完整存储行分叉会话，可选仅保留最近的完整 user turn。
    ///
    /// `None` 复制完整历史。`Some(0)` 创建空子会话。
    /// 正数值从末尾第 N 条 user 行开始，该 user turn 所属的
    /// assistant/tool 行保持完整。
    pub async fn fork_session_recent_turns(
        &self,
        source_id: &str,
        new_id: &str,
        recent_turns: Option<usize>,
    ) -> Result<()> {
        if source_id == new_id {
            anyhow::bail!("fork_session_recent_turns: source and target session ids must differ");
        }
        // Forking reads the parent and then writes the child. Reserve the write
        // slot before taking that snapshot so a concurrent runtime update cannot
        // turn this transaction into SQLITE_BUSY_SNAPSHOT at the first INSERT.
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let parent_row = sqlx::query("SELECT model, title FROM sessions WHERE id = ?1")
            .bind(source_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "fork_session_recent_turns: source session not found: {source_id:?}"
                )
            })?;
        let parent: (Option<String>, Option<String>) = (
            parent_row.get::<Option<String>, _>(0),
            parent_row.get::<Option<String>, _>(1),
        );
        let target_exists: bool = sqlx::query(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
        )
        .bind(new_id)
        .fetch_one(&mut *tx)
        .await?
        .get::<bool, _>(0);
        if target_exists {
            anyhow::bail!("fork_session_recent_turns: target session already exists");
        }
        sqlx::query(
            "INSERT INTO sessions (id, source, model, parent_session_id, started_at)
             VALUES (?1, 'tauri', ?2, ?3, ?4)",
        )
        .bind(new_id)
        .bind(&parent.0)
        .bind(source_id)
        .bind(now_epoch_secs()?)
        .execute(&mut *tx)
        .await?;

        match recent_turns {
            Some(0) => {}
            None => {
                sqlx::query(
                    "INSERT INTO messages (
                        session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                        timestamp, token_count, finish_reason,
                        reasoning, reasoning_content, reasoning_details,
                        reasoning_items, message_items, media_json
                     )
                     SELECT ?1, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                            timestamp, token_count, finish_reason,
                            reasoning, reasoning_content, reasoning_details,
                            reasoning_items, message_items, media_json
                     FROM messages
                     WHERE session_id = ?2
                     ORDER BY timestamp ASC, id ASC",
                )
                .bind(new_id)
                .bind(source_id)
                .execute(&mut *tx)
                .await?;
            }
            Some(turns) => {
                let offset = i64::try_from(turns - 1).unwrap_or(i64::MAX);
                let boundary = sqlx::query(
                    "SELECT timestamp, id
                     FROM messages
                     WHERE session_id = ?1 AND role = 'user'
                     ORDER BY timestamp DESC, id DESC
                     LIMIT 1 OFFSET ?2",
                )
                .bind(source_id)
                .bind(offset)
                .fetch_optional(&mut *tx)
                .await?;
                if let Some(brow) = boundary {
                    let timestamp: f64 = brow.get::<f64, _>(0);
                    let message_id: i64 = brow.get::<i64, _>(1);
                    sqlx::query(
                        "INSERT INTO messages (
                            session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                            timestamp, token_count, finish_reason,
                            reasoning, reasoning_content, reasoning_details,
                            reasoning_items, message_items, media_json
                         )
                         SELECT ?1, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                                timestamp, token_count, finish_reason,
                                reasoning, reasoning_content, reasoning_details,
                                reasoning_items, message_items, media_json
                         FROM messages
                         WHERE session_id = ?2
                         AND (timestamp > ?3 OR (timestamp = ?3 AND id >= ?4))
                         ORDER BY timestamp ASC, id ASC",
                    )
                    .bind(new_id)
                    .bind(source_id)
                    .bind(timestamp)
                    .bind(message_id)
                    .execute(&mut *tx)
                    .await?;
                } else {
                    sqlx::query(
                        "INSERT INTO messages (
                            session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                            timestamp, token_count, finish_reason,
                            reasoning, reasoning_content, reasoning_details,
                            reasoning_items, message_items, media_json
                         )
                         SELECT ?1, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                                timestamp, token_count, finish_reason,
                                reasoning, reasoning_content, reasoning_details,
                                reasoning_items, message_items, media_json
                         FROM messages
                         WHERE session_id = ?2
                         ORDER BY timestamp ASC, id ASC",
                    )
                    .bind(new_id)
                    .bind(source_id)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
        let counts_row = sqlx::query(
            "SELECT COUNT(*), SUM(CASE WHEN role = 'tool' THEN 1 ELSE 0 END)
             FROM messages WHERE session_id = ?1",
        )
        .bind(new_id)
        .fetch_one(&mut *tx)
        .await?;
        let message_count: i64 = counts_row.get::<i64, _>(0);
        let tool_call_count: i64 = counts_row.get::<Option<i64>, _>(1).unwrap_or(0);
        sqlx::query(
            "UPDATE sessions
             SET message_count = ?1, tool_call_count = ?2
             WHERE id = ?3",
        )
        .bind(message_count)
        .bind(tool_call_count)
        .bind(new_id)
        .execute(&mut *tx)
        .await?;
        if let Some(title) = parent.1.filter(|title| !title.trim().is_empty()) {
            let branched = format!("{title} · branch");
            let result = sqlx::query("UPDATE sessions SET title = ?1 WHERE id = ?2")
                .bind(&branched)
                .bind(new_id)
                .execute(&mut *tx)
                .await;
            match result {
                Ok(_) => {}
                Err(err) if super::is_unique_constraint(&err) => {
                    let suffix: String = new_id.chars().take(8).collect();
                    let unique = format!("{} · {}", truncate_chars(&branched, 60), suffix);
                    sqlx::query("UPDATE sessions SET title = ?1 WHERE id = ?2")
                        .bind(&unique)
                        .bind(new_id)
                        .execute(&mut *tx)
                        .await?;
                }
                Err(err) => return Err(err.into()),
            }
        }
        tx.commit().await?;
        self.infer_and_write_branch_metadata(source_id, new_id, BranchKind::Agent)
            .await?;

        Ok(())
    }

    /// 将本会话截断到第 `keep_chat_bubbles` 个 user/assistant 气泡（含其后紧跟的 tool 行）。
    ///
    /// `keep_chat_bubbles == 0` 时删除全部消息。会话不存在时返回错误。用于编辑重发 / 再生前对齐 DB。
    pub async fn truncate_session_to_bubbles(
        &self,
        session_id: &str,
        keep_chat_bubbles: usize,
    ) -> Result<()> {
        if self.get_session(session_id).await?.is_none() {
            anyhow::bail!("truncate_session_to_bubbles: session not found");
        }

        let messages = self.get_messages(session_id).await?;
        let mut tx = self.pool.begin().await?;

        if keep_chat_bubbles == 0 || messages.is_empty() {
            sqlx::query("DELETE FROM messages WHERE session_id = ?1")
                .bind(session_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "UPDATE sessions
                 SET message_count = 0, tool_call_count = 0
                 WHERE id = ?1",
            )
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(());
        }

        let Some(end) = end_inclusive_for_bubbles(&messages, keep_chat_bubbles) else {
            tx.commit().await?;
            return Ok(());
        };

        if end + 1 >= messages.len() {
            tx.commit().await?;
            return Ok(());
        }

        let last_kept_id = messages[end].id;
        sqlx::query("DELETE FROM messages WHERE session_id = ?1 AND id > ?2")
            .bind(session_id)
            .bind(last_kept_id)
            .execute(&mut *tx)
            .await?;

        let mut message_count = 0i64;
        let mut tool_call_count = 0i64;
        for m in &messages[..=end] {
            message_count += 1;
            if m.role == "tool" {
                tool_call_count += 1;
            }
        }
        sqlx::query(
            "UPDATE sessions
             SET message_count = ?1, tool_call_count = ?2
             WHERE id = ?3",
        )
        .bind(message_count)
        .bind(tool_call_count)
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// 删除半开区间 `[start, end)` 内的聊天气泡（仅计 user/assistant，0-based）。
    ///
    /// 被删 assistant 之后的连续 `tool` 行一并删除。`start >= end` 时为 no-op。
    /// 用于 UI 中部「删除消息」与 DB 对齐。
    pub async fn remove_chat_bubbles(
        &self,
        session_id: &str,
        start: usize,
        end: usize,
    ) -> Result<()> {
        if self.get_session(session_id).await?.is_none() {
            anyhow::bail!("remove_chat_bubbles: session not found");
        }
        if start >= end {
            return Ok(());
        }

        let messages = self.get_messages(session_id).await?;
        if messages.is_empty() {
            return Ok(());
        }

        let ids = message_ids_in_bubble_range(&messages, start, end);
        if ids.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;
        for id in &ids {
            sqlx::query("DELETE FROM messages WHERE id = ?1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }

        let remaining = messages
            .iter()
            .filter(|m| !ids.contains(&m.id))
            .collect::<Vec<_>>();
        let message_count = remaining.len() as i64;
        let tool_call_count = remaining.iter().filter(|m| m.role == "tool").count() as i64;
        sqlx::query(
            "UPDATE sessions
             SET message_count = ?1, tool_call_count = ?2
             WHERE id = ?3",
        )
        .bind(message_count)
        .bind(tool_call_count)
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// 结束旧会话并拆出子会话：摘要消息 + 最近 `keep_tail_bubbles` 轮（含 tool）。
    pub async fn compact_and_split(
        &self,
        old_id: &str,
        new_id: &str,
        summary_text: &str,
        keep_tail_bubbles: usize,
    ) -> Result<()> {
        self.compact_and_split_if_unchanged(old_id, new_id, summary_text, keep_tail_bubbles, None)
            .await
    }

    /// 与 [`Self::compact_and_split`] 相同，但在提交前校验源会话最后一条消息未变化。
    ///
    /// `expected_last_message_id` 来自生成摘要前的快照；不匹配时整个事务回滚，
    /// 避免用过期摘要结束仍在写入的会话。
    pub async fn compact_and_split_if_unchanged(
        &self,
        old_id: &str,
        new_id: &str,
        summary_text: &str,
        keep_tail_bubbles: usize,
        expected_last_message_id: Option<i64>,
    ) -> Result<()> {
        if old_id == new_id {
            anyhow::bail!("compact_and_split: session ids must differ");
        }
        let parent = self
            .get_session(old_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("compact_and_split: source session not found"))?;
        if parent.ended_at.is_some() {
            anyhow::bail!("compact_and_split: source session already ended");
        }

        let messages = self.get_messages(old_id).await?;
        let observed_last_message_id = messages.last().map(|message| message.id);
        if expected_last_message_id.is_some()
            && observed_last_message_id != expected_last_message_id
        {
            anyhow::bail!("compact_and_split: source session changed while summarizing");
        }

        let now = now_epoch_secs()?;
        let continued_title = parent
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .map(|title| format!("{title} · continued"));
        let mut tx = self.pool.begin().await?;

        let target_exists: bool = sqlx::query(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
        )
        .bind(new_id)
        .fetch_one(&mut *tx)
        .await?
        .get::<bool, _>(0);
        if target_exists {
            anyhow::bail!("compact_and_split: target session already exists");
        }

        let current_last_message_id: Option<i64> =
            sqlx::query("SELECT MAX(id) FROM messages WHERE session_id = ?1")
                .bind(old_id)
                .fetch_one(&mut *tx)
                .await?
                .get::<Option<i64>, _>(0);
        if current_last_message_id != observed_last_message_id {
            anyhow::bail!("compact_and_split: source session changed while preparing transaction");
        }

        let changed = sqlx::query(
            "UPDATE sessions
             SET ended_at = ?1, end_reason = 'compacted'
             WHERE id = ?2 AND ended_at IS NULL",
        )
        .bind(now)
        .bind(old_id)
        .execute(&mut *tx)
        .await?;
        if changed.rows_affected() != 1 {
            anyhow::bail!("compact_and_split: source session already ended");
        }

        sqlx::query(
            "INSERT INTO sessions (
                id, source, title, model, parent_session_id, started_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(new_id)
        .bind(&parent.source)
        .bind(&continued_title)
        .bind(&parent.model)
        .bind(old_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO messages (session_id, role, content, timestamp)
             VALUES (?1, 'user', ?2, ?3)",
        )
        .bind(new_id)
        .bind(summary_text)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let mut message_count = 1i64;
        let mut tool_call_count = 0i64;
        if keep_tail_bubbles > 0 {
            if let Some(start) = start_inclusive_for_tail_bubbles(&messages, keep_tail_bubbles) {
                for (i, m) in messages[start..].iter().enumerate() {
                    let tool_calls = json_to_db(&m.tool_calls)?;
                    let reasoning_details = json_to_db(&m.reasoning_details)?;
                    let reasoning_items = json_to_db(&m.reasoning_items)?;
                    let message_items = json_to_db(&m.message_items)?;
                    let timestamp = now + (i + 1) as f64 * 0.001;
                    sqlx::query(
                        "INSERT INTO messages (
                            session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                            timestamp, token_count, finish_reason,
                            reasoning, reasoning_content, reasoning_details,
                            reasoning_items, message_items, media_json
                         ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                            ?8, ?9, ?10,
                            ?11, ?12, ?13,
                            ?14, ?15, ?16
                         )",
                    )
                    .bind(new_id)
                    .bind(&m.role)
                    .bind(&m.content)
                    .bind(&m.compressed_content)
                    .bind(&m.tool_call_id)
                    .bind(&tool_calls)
                    .bind(&m.tool_name)
                    .bind(timestamp)
                    .bind(m.token_count)
                    .bind(&m.finish_reason)
                    .bind(&m.reasoning)
                    .bind(&m.reasoning_content)
                    .bind(&reasoning_details)
                    .bind(&reasoning_items)
                    .bind(&message_items)
                    .bind(&m.media_json)
                    .execute(&mut *tx)
                    .await?;
                    message_count += 1;
                    if m.role == "tool" {
                        tool_call_count += 1;
                    }
                }
            }
        }

        sqlx::query(
            "UPDATE sessions
             SET message_count = ?1, tool_call_count = ?2
             WHERE id = ?3",
        )
        .bind(message_count)
        .bind(tool_call_count)
        .bind(new_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(())
    }

    /// 重建 OpenAI conversation 形状（assistant 带 `tool_calls` / `reasoning*`）。
    pub async fn get_messages_as_conversation(&self, session_id: &str) -> Result<Vec<Value>> {
        let messages = self.get_messages(session_id).await?;
        let mut out = Vec::with_capacity(messages.len());
        for m in messages {
            let mut obj = serde_json::Map::new();
            obj.insert("role".into(), Value::String(m.role));
            match m.content {
                Some(c) => {
                    obj.insert("content".into(), Value::String(c));
                }
                None => {
                    obj.insert("content".into(), Value::Null);
                }
            }
            if let Some(tc) = m.tool_calls {
                obj.insert("tool_calls".into(), tc);
            }
            if let Some(id) = m.tool_call_id {
                obj.insert("tool_call_id".into(), Value::String(id));
            }
            if let Some(name) = m.tool_name {
                obj.insert("name".into(), Value::String(name));
            }
            if let Some(r) = m.reasoning {
                obj.insert("reasoning".into(), Value::String(r));
            }
            if let Some(r) = m.reasoning_content {
                obj.insert("reasoning_content".into(), Value::String(r));
            }
            if let Some(r) = m.reasoning_details {
                obj.insert("reasoning_details".into(), r);
            }
            if let Some(r) = m.reasoning_items {
                obj.insert("reasoning_items".into(), r);
            }
            if let Some(r) = m.message_items {
                obj.insert("message_items".into(), r);
            }
            out.push(Value::Object(obj));
        }
        Ok(out)
    }
}

/// 返回保留尾部 `keep` 个 user/assistant 气泡（含 assistant 后连续 tool）的**起始下标**。
fn start_inclusive_for_tail_bubbles(messages: &[StoredMessage], keep: usize) -> Option<usize> {
    if keep == 0 || messages.is_empty() {
        return None;
    }
    let mut bubble_starts = Vec::new();
    for (i, m) in messages.iter().enumerate() {
        if m.role == "user" || m.role == "assistant" {
            bubble_starts.push(i);
        }
    }
    if bubble_starts.is_empty() {
        return None;
    }
    let skip = bubble_starts.len().saturating_sub(keep);
    Some(bubble_starts[skip])
}

/// 返回保留前缀的**含尾**下标：数到第 `keep` 个 user/assistant 后，再吞掉其后连续 tool 行。
///
/// `keep == 0` 或消息为空时返回 `None`（调用方视为无前缀）。
fn end_inclusive_for_bubbles(messages: &[StoredMessage], keep: usize) -> Option<usize> {
    if keep == 0 || messages.is_empty() {
        return None;
    }
    let mut bubbles = 0usize;
    for (i, m) in messages.iter().enumerate() {
        match m.role.as_str() {
            "user" | "assistant" => {
                bubbles += 1;
                if bubbles >= keep {
                    let mut last = i;
                    for (j, n) in messages.iter().enumerate().skip(i + 1) {
                        if n.role == "tool" {
                            last = j;
                        } else {
                            break;
                        }
                    }
                    return Some(last);
                }
            }
            _ => {}
        }
    }
    Some(messages.len() - 1)
}

/// 收集气泡半开区间 `[start, end)` 内的消息 id（含区间内 assistant 后的连续 tool）。
fn message_ids_in_bubble_range(messages: &[StoredMessage], start: usize, end: usize) -> Vec<i64> {
    if start >= end || messages.is_empty() {
        return Vec::new();
    }
    let mut ids = Vec::new();
    let mut bubble = 0usize;
    let mut i = 0usize;
    while i < messages.len() {
        let m = &messages[i];
        match m.role.as_str() {
            "user" | "assistant" => {
                if bubble >= start && bubble < end {
                    ids.push(m.id);
                    if m.role == "assistant" {
                        let mut j = i + 1;
                        while j < messages.len() && messages[j].role == "tool" {
                            ids.push(messages[j].id);
                            j += 1;
                        }
                        i = j;
                        bubble += 1;
                        continue;
                    }
                }
                bubble += 1;
                i += 1;
            }
            "tool" => {
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    ids
}
