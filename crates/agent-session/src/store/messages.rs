//! 富消息读写。

use anyhow::Result;
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::Value;

use super::{
    is_unique_constraint, json_from_db, json_to_db, now_epoch_secs, truncate_chars, NewMessage,
    SessionStore, StoredMessage,
};

pub(crate) fn insert_message_row(
    tx: &Transaction<'_>,
    msg: NewMessage<'_>,
    compressed_content: Option<&str>,
    timestamp: f64,
) -> Result<i64> {
    let tool_calls = json_to_db(&msg.tool_calls)?;
    let reasoning_details = json_to_db(&msg.reasoning_details)?;
    let codex_reasoning_items = json_to_db(&msg.codex_reasoning_items)?;
    let codex_message_items = json_to_db(&msg.codex_message_items)?;
    tx.execute(
        "INSERT INTO messages (
            session_id, role, content, compressed_content,
            tool_call_id, tool_calls, tool_name,
            timestamp, token_count, finish_reason,
            reasoning, reasoning_content, reasoning_details,
            codex_reasoning_items, codex_message_items, media_json
         ) VALUES (
            ?1, ?2, ?3, ?4,
            ?5, ?6, ?7,
            ?8, ?9, ?10,
            ?11, ?12, ?13,
            ?14, ?15, ?16
         )",
        params![
            msg.session_id,
            msg.role,
            msg.content,
            compressed_content,
            msg.tool_call_id,
            tool_calls,
            msg.tool_name,
            timestamp,
            msg.token_count,
            msg.finish_reason,
            msg.reasoning,
            msg.reasoning_content,
            reasoning_details,
            codex_reasoning_items,
            codex_message_items,
            msg.media_json,
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

impl SessionStore {
    /// 追加一条富消息，并递增 `sessions.message_count`（`role=tool` 时同时 `tool_call_count++`）。
    pub fn append_message(&self, msg: NewMessage<'_>) -> Result<i64> {
        self.assert_session_writable(msg.session_id)?;
        let session_id = msg.session_id;
        let is_tool = msg.role == "tool";
        let tx = self.conn.unchecked_transaction()?;
        let id = insert_message_row(&tx, msg, None, now_epoch_secs()?)?;

        if is_tool {
            tx.execute(
                "UPDATE sessions
                 SET message_count = message_count + 1,
                     tool_call_count = tool_call_count + 1
                 WHERE id = ?1",
                params![session_id],
            )?;
        } else {
            tx.execute(
                "UPDATE sessions SET message_count = message_count + 1 WHERE id = ?1",
                params![session_id],
            )?;
        }

        tx.commit()?;
        Ok(id)
    }

    /// 按时间顺序读取会话内全部消息行。
    /// 跨全部会话取「带结构化媒体」的消息 `(session_id, message_id, media_json)`。
    ///
    /// 供 artifacts 回填：把历史生成文件关联回来源会话。单条查询，开销低。
    pub fn media_messages(&self) -> Result<Vec<(String, i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, id, media_json
             FROM messages
             WHERE media_json IS NOT NULL AND media_json != ''
             ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason,
                    reasoning, reasoning_content, reasoning_details,
                    codex_reasoning_items, codex_message_items, media_json
             FROM messages
             WHERE session_id = ?1
             ORDER BY timestamp ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, f64>(8)?,
                row.get::<_, Option<i64>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<String>>(12)?,
                row.get::<_, Option<String>>(13)?,
                row.get::<_, Option<String>>(14)?,
                row.get::<_, Option<String>>(15)?,
                row.get::<_, Option<String>>(16)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (
                id,
                session_id,
                role,
                content,
                compressed_content,
                tool_call_id,
                tool_calls_raw,
                tool_name,
                timestamp,
                token_count,
                finish_reason,
                reasoning,
                reasoning_content,
                reasoning_details_raw,
                codex_reasoning_items_raw,
                codex_message_items_raw,
                media_json,
            ) = row?;
            out.push(StoredMessage {
                id,
                session_id,
                role,
                content,
                compressed_content,
                tool_call_id,
                tool_calls: json_from_db(tool_calls_raw)?,
                tool_name,
                timestamp,
                token_count,
                finish_reason,
                reasoning,
                reasoning_content,
                reasoning_details: json_from_db(reasoning_details_raw)?,
                codex_reasoning_items: json_from_db(codex_reasoning_items_raw)?,
                codex_message_items: json_from_db(codex_message_items_raw)?,
                media_json,
            });
        }
        Ok(out)
    }

    /// 为指定消息写入 provider 视图或内部交付标记；原始 `content` 不变。
    pub fn update_message_compressed_content(
        &self,
        message_id: i64,
        compressed_content: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET compressed_content = ?1 WHERE id = ?2",
            params![compressed_content, message_id],
        )?;
        Ok(())
    }

    /// 回写本会话最近一条 assistant 的 `reasoning_details`（保留其它键，覆盖 timeline/surfaces）。
    ///
    /// 工具循环在 assistant 落盘之后才会 `upsert_surface`；若不回写，历史恢复会丢 A2UI 卡片。
    pub fn patch_last_assistant_reasoning_details(
        &self,
        session_id: &str,
        details: &Value,
    ) -> Result<()> {
        // 该列可能为 SQL NULL：闭包按 Option<String> 读，避免 NULL 触发
        // "Invalid column type Null"（.optional() 仅处理「无行」，不处理「列为 NULL」）。
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT reasoning_details FROM messages
             WHERE session_id = ?1 AND role = 'assistant'
             ORDER BY id DESC LIMIT 1",
                params![session_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
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
        self.conn.execute(
            "UPDATE messages SET reasoning_details = ?1
             WHERE id = (
               SELECT id FROM messages
               WHERE session_id = ?2 AND role = 'assistant'
               ORDER BY id DESC LIMIT 1
             )",
            params![json, session_id],
        )?;
        Ok(())
    }

    /// 将源会话消息复制到新会话（含 tool 行），截止到第 `keep_chat_bubbles` 个 user/assistant 气泡。
    ///
    /// 新会话写入 `parent_session_id = source_id`，便于谱系追溯。`keep_chat_bubbles == 0` 时仅创建空会话。
    pub fn fork_session(
        &self,
        source_id: &str,
        new_id: &str,
        keep_chat_bubbles: usize,
    ) -> Result<()> {
        if source_id == new_id {
            anyhow::bail!("fork_session: source and target session ids must differ");
        }
        if self.get_session(new_id)?.is_some() {
            anyhow::bail!("fork_session: target session already exists");
        }

        let parent = self.get_session(source_id)?;
        let model = parent.as_ref().and_then(|p| p.model.clone());
        self.create_session(new_id, "tauri", model.as_deref(), None, Some(source_id))?;

        if keep_chat_bubbles == 0 {
            return Ok(());
        }

        let messages = self.get_messages(source_id)?;
        let Some(end) = end_inclusive_for_bubbles(&messages, keep_chat_bubbles) else {
            return Ok(());
        };

        let tx = self.conn.unchecked_transaction()?;
        let mut message_count = 0i64;
        let mut tool_call_count = 0i64;
        for m in &messages[..=end] {
            let tool_calls = json_to_db(&m.tool_calls)?;
            let reasoning_details = json_to_db(&m.reasoning_details)?;
            let codex_reasoning_items = json_to_db(&m.codex_reasoning_items)?;
            let codex_message_items = json_to_db(&m.codex_message_items)?;
            tx.execute(
                "INSERT INTO messages (
                    session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason,
                    reasoning, reasoning_content, reasoning_details,
                    codex_reasoning_items, codex_message_items, media_json
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                    ?8, ?9, ?10,
                    ?11, ?12, ?13,
                    ?14, ?15, ?16
                 )",
                params![
                    new_id,
                    m.role,
                    m.content,
                    m.compressed_content,
                    m.tool_call_id,
                    tool_calls,
                    m.tool_name,
                    m.timestamp,
                    m.token_count,
                    m.finish_reason,
                    m.reasoning,
                    m.reasoning_content,
                    reasoning_details,
                    codex_reasoning_items,
                    codex_message_items,
                    m.media_json,
                ],
            )?;
            message_count += 1;
            if m.role == "tool" {
                tool_call_count += 1;
            }
        }
        tx.execute(
            "UPDATE sessions
             SET message_count = ?1, tool_call_count = ?2
             WHERE id = ?3",
            params![message_count, tool_call_count, new_id],
        )?;
        tx.commit()?;

        if let Some(title) = parent
            .and_then(|p| p.title)
            .filter(|t| !t.trim().is_empty())
        {
            let branched = format!("{title} · branch");
            let _ = self.set_session_title(new_id, &branched);
        }

        Ok(())
    }

    /// Fork a session from complete stored rows, optionally retaining only the
    /// most recent complete user turns.
    ///
    /// `None` copies the full history. `Some(0)` creates an empty child
    /// session. A positive value starts at the Nth user row counted from the
    /// end, so assistant/tool rows belonging to that user turn remain intact.
    pub fn fork_session_recent_turns(
        &self,
        source_id: &str,
        new_id: &str,
        recent_turns: Option<usize>,
    ) -> Result<()> {
        if source_id == new_id {
            anyhow::bail!("fork_session_recent_turns: source and target session ids must differ");
        }
        let tx = self.conn.unchecked_transaction()?;
        let parent = tx
            .query_row(
                "SELECT model, title FROM sessions WHERE id = ?1",
                params![source_id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "fork_session_recent_turns: source session not found: {source_id:?}"
                )
            })?;
        let target_exists = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
            params![new_id],
            |row| row.get::<_, bool>(0),
        )?;
        if target_exists {
            anyhow::bail!("fork_session_recent_turns: target session already exists");
        }
        tx.execute(
            "INSERT INTO sessions (id, source, model, parent_session_id, started_at)
             VALUES (?1, 'tauri', ?2, ?3, ?4)",
            params![new_id, parent.0, source_id, now_epoch_secs()?],
        )?;

        let copy_sql = "INSERT INTO messages (
                session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                timestamp, token_count, finish_reason,
                reasoning, reasoning_content, reasoning_details,
                codex_reasoning_items, codex_message_items, media_json
             )
             SELECT ?1, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason,
                    reasoning, reasoning_content, reasoning_details,
                    codex_reasoning_items, codex_message_items, media_json
             FROM messages
             WHERE session_id = ?2";
        match recent_turns {
            Some(0) => {}
            None => {
                tx.execute(
                    &format!("{copy_sql} ORDER BY timestamp ASC, id ASC"),
                    params![new_id, source_id],
                )?;
            }
            Some(turns) => {
                let offset = i64::try_from(turns - 1).unwrap_or(i64::MAX);
                let boundary = tx
                    .query_row(
                        "SELECT timestamp, id
                         FROM messages
                         WHERE session_id = ?1 AND role = 'user'
                         ORDER BY timestamp DESC, id DESC
                         LIMIT 1 OFFSET ?2",
                        params![source_id, offset],
                        |row| Ok((row.get::<_, f64>(0)?, row.get::<_, i64>(1)?)),
                    )
                    .optional()?;
                if let Some((timestamp, message_id)) = boundary {
                    tx.execute(
                        &format!(
                            "{copy_sql}
                             AND (timestamp > ?3 OR (timestamp = ?3 AND id >= ?4))
                             ORDER BY timestamp ASC, id ASC"
                        ),
                        params![new_id, source_id, timestamp, message_id],
                    )?;
                } else {
                    tx.execute(
                        &format!("{copy_sql} ORDER BY timestamp ASC, id ASC"),
                        params![new_id, source_id],
                    )?;
                }
            }
        }
        let (message_count, tool_call_count) = tx.query_row(
            "SELECT COUNT(*), SUM(CASE WHEN role = 'tool' THEN 1 ELSE 0 END)
             FROM messages WHERE session_id = ?1",
            params![new_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                ))
            },
        )?;
        tx.execute(
            "UPDATE sessions
             SET message_count = ?1, tool_call_count = ?2
             WHERE id = ?3",
            params![message_count, tool_call_count, new_id],
        )?;
        if let Some(title) = parent.1.filter(|title| !title.trim().is_empty()) {
            let branched = format!("{title} · branch");
            if let Err(error) = tx.execute(
                "UPDATE sessions SET title = ?1 WHERE id = ?2",
                params![branched, new_id],
            ) {
                if !is_unique_constraint(&error) {
                    return Err(error.into());
                }
                let suffix: String = new_id.chars().take(8).collect();
                let unique = format!("{} · {}", truncate_chars(&branched, 60), suffix);
                tx.execute(
                    "UPDATE sessions SET title = ?1 WHERE id = ?2",
                    params![unique, new_id],
                )?;
            }
        }
        tx.commit()?;

        Ok(())
    }

    /// 将本会话截断到第 `keep_chat_bubbles` 个 user/assistant 气泡（含其后紧跟的 tool 行）。
    ///
    /// `keep_chat_bubbles == 0` 时删除全部消息。会话不存在时返回错误。用于编辑重发 / 再生前对齐 DB。
    pub fn truncate_session_to_bubbles(
        &self,
        session_id: &str,
        keep_chat_bubbles: usize,
    ) -> Result<()> {
        if self.get_session(session_id)?.is_none() {
            anyhow::bail!("truncate_session_to_bubbles: session not found");
        }

        let messages = self.get_messages(session_id)?;
        let tx = self.conn.unchecked_transaction()?;

        if keep_chat_bubbles == 0 || messages.is_empty() {
            tx.execute(
                "DELETE FROM messages WHERE session_id = ?1",
                params![session_id],
            )?;
            tx.execute(
                "UPDATE sessions
                 SET message_count = 0, tool_call_count = 0
                 WHERE id = ?1",
                params![session_id],
            )?;
            tx.commit()?;
            return Ok(());
        }

        let Some(end) = end_inclusive_for_bubbles(&messages, keep_chat_bubbles) else {
            // 气泡不足 keep 时视为已满足前缀，无需删尾
            tx.commit()?;
            return Ok(());
        };

        if end + 1 >= messages.len() {
            tx.commit()?;
            return Ok(());
        }

        let last_kept_id = messages[end].id;
        tx.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND id > ?2",
            params![session_id, last_kept_id],
        )?;

        let mut message_count = 0i64;
        let mut tool_call_count = 0i64;
        for m in &messages[..=end] {
            message_count += 1;
            if m.role == "tool" {
                tool_call_count += 1;
            }
        }
        tx.execute(
            "UPDATE sessions
             SET message_count = ?1, tool_call_count = ?2
             WHERE id = ?3",
            params![message_count, tool_call_count, session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 删除半开区间 `[start, end)` 内的聊天气泡（仅计 user/assistant，0-based）。
    ///
    /// 被删 assistant 之后的连续 `tool` 行一并删除。`start >= end` 时为 no-op。
    /// 用于 UI 中部「删除消息」与 DB 对齐。
    pub fn remove_chat_bubbles(&self, session_id: &str, start: usize, end: usize) -> Result<()> {
        if self.get_session(session_id)?.is_none() {
            anyhow::bail!("remove_chat_bubbles: session not found");
        }
        if start >= end {
            return Ok(());
        }

        let messages = self.get_messages(session_id)?;
        if messages.is_empty() {
            return Ok(());
        }

        let ids = message_ids_in_bubble_range(&messages, start, end);
        if ids.is_empty() {
            return Ok(());
        }

        let tx = self.conn.unchecked_transaction()?;
        for id in &ids {
            tx.execute("DELETE FROM messages WHERE id = ?1", params![id])?;
        }

        let remaining = messages
            .iter()
            .filter(|m| !ids.contains(&m.id))
            .collect::<Vec<_>>();
        let message_count = remaining.len() as i64;
        let tool_call_count = remaining.iter().filter(|m| m.role == "tool").count() as i64;
        tx.execute(
            "UPDATE sessions
             SET message_count = ?1, tool_call_count = ?2
             WHERE id = ?3",
            params![message_count, tool_call_count, session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 结束旧会话并拆出子会话：摘要消息 + 最近 `keep_tail_bubbles` 轮（含 tool）。
    pub fn compact_and_split(
        &self,
        old_id: &str,
        new_id: &str,
        summary_text: &str,
        keep_tail_bubbles: usize,
    ) -> Result<()> {
        self.compact_and_split_if_unchanged(old_id, new_id, summary_text, keep_tail_bubbles, None)
    }

    /// 与 [`Self::compact_and_split`] 相同，但在提交前校验源会话最后一条消息未变化。
    ///
    /// `expected_last_message_id` 来自生成摘要前的快照；不匹配时整个事务回滚，
    /// 避免用过期摘要结束仍在写入的会话。
    pub fn compact_and_split_if_unchanged(
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
            .get_session(old_id)?
            .ok_or_else(|| anyhow::anyhow!("compact_and_split: source session not found"))?;
        if parent.ended_at.is_some() {
            anyhow::bail!("compact_and_split: source session already ended");
        }

        let messages = self.get_messages(old_id)?;
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
        let tx = self.conn.unchecked_transaction()?;

        let target_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
            params![new_id],
            |row| row.get(0),
        )?;
        if target_exists {
            anyhow::bail!("compact_and_split: target session already exists");
        }

        let current_last_message_id: Option<i64> = tx.query_row(
            "SELECT MAX(id) FROM messages WHERE session_id = ?1",
            params![old_id],
            |row| row.get(0),
        )?;
        if current_last_message_id != observed_last_message_id {
            anyhow::bail!("compact_and_split: source session changed while preparing transaction");
        }

        let changed = tx.execute(
            "UPDATE sessions
             SET ended_at = ?1, end_reason = 'compacted'
             WHERE id = ?2 AND ended_at IS NULL",
            params![now, old_id],
        )?;
        if changed != 1 {
            anyhow::bail!("compact_and_split: source session already ended");
        }

        tx.execute(
            "INSERT INTO sessions (
                id, source, title, model, parent_session_id, started_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                new_id,
                parent.source,
                continued_title,
                parent.model,
                old_id,
                now,
            ],
        )?;
        tx.execute(
            "INSERT INTO messages (session_id, role, content, timestamp)
             VALUES (?1, 'user', ?2, ?3)",
            params![new_id, summary_text, now],
        )?;

        let mut message_count = 1i64;
        let mut tool_call_count = 0i64;
        if keep_tail_bubbles > 0 {
            if let Some(start) = start_inclusive_for_tail_bubbles(&messages, keep_tail_bubbles) {
                for (i, m) in messages[start..].iter().enumerate() {
                    let tool_calls = json_to_db(&m.tool_calls)?;
                    let reasoning_details = json_to_db(&m.reasoning_details)?;
                    let codex_reasoning_items = json_to_db(&m.codex_reasoning_items)?;
                    let codex_message_items = json_to_db(&m.codex_message_items)?;
                    // 摘要按当前时间写入；尾部需重排 timestamp，确保显示在摘要之后。
                    let timestamp = now + (i + 1) as f64 * 0.001;
                    tx.execute(
                        "INSERT INTO messages (
                            session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                            timestamp, token_count, finish_reason,
                            reasoning, reasoning_content, reasoning_details,
                            codex_reasoning_items, codex_message_items, media_json
                         ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                            ?8, ?9, ?10,
                            ?11, ?12, ?13,
                            ?14, ?15, ?16
                         )",
                        params![
                            new_id,
                            m.role,
                            m.content,
                            m.compressed_content,
                            m.tool_call_id,
                            tool_calls,
                            m.tool_name,
                            timestamp,
                            m.token_count,
                            m.finish_reason,
                            m.reasoning,
                            m.reasoning_content,
                            reasoning_details,
                            codex_reasoning_items,
                            codex_message_items,
                            m.media_json,
                        ],
                    )?;
                    message_count += 1;
                    if m.role == "tool" {
                        tool_call_count += 1;
                    }
                }
            }
        }

        tx.execute(
            "UPDATE sessions
             SET message_count = ?1, tool_call_count = ?2
             WHERE id = ?3",
            params![message_count, tool_call_count, new_id],
        )?;
        tx.commit()?;

        Ok(())
    }

    /// 重建 OpenAI conversation 形状（assistant 带 `tool_calls` / `reasoning*`）。
    pub fn get_messages_as_conversation(&self, session_id: &str) -> Result<Vec<Value>> {
        let messages = self.get_messages(session_id)?;
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
            if let Some(r) = m.codex_reasoning_items {
                obj.insert("codex_reasoning_items".into(), r);
            }
            if let Some(r) = m.codex_message_items {
                obj.insert("codex_message_items".into(), r);
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
    // 气泡不足 keep：整段都算前缀
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
                // 孤立 tool（无前缀 assistant）按不计入气泡，但若落在已选区间尾随已被吞
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    ids
}
