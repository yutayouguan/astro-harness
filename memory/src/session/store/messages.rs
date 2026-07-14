//! 富消息读写。

use anyhow::Result;
use rusqlite::params;
use serde_json::Value;

use super::{json_from_db, json_to_db, now_epoch_secs, NewMessage, SessionStore, StoredMessage};

impl SessionStore {
    /// 追加一条富消息，并递增 `sessions.message_count`（`role=tool` 时同时 `tool_call_count++`）。
    pub fn append_message(&self, msg: NewMessage<'_>) -> Result<i64> {
        let timestamp = now_epoch_secs()?;
        let tool_calls = json_to_db(&msg.tool_calls)?;
        let reasoning_details = json_to_db(&msg.reasoning_details)?;
        let codex_reasoning_items = json_to_db(&msg.codex_reasoning_items)?;
        let codex_message_items = json_to_db(&msg.codex_message_items)?;

        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO messages (
                session_id, role, content, tool_call_id, tool_calls, tool_name,
                timestamp, token_count, finish_reason,
                reasoning, reasoning_content, reasoning_details,
                codex_reasoning_items, codex_message_items
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9,
                ?10, ?11, ?12,
                ?13, ?14
             )",
            params![
                msg.session_id,
                msg.role,
                msg.content,
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
            ],
        )?;
        let id = tx.last_insert_rowid();

        if msg.role == "tool" {
            tx.execute(
                "UPDATE sessions
                 SET message_count = message_count + 1,
                     tool_call_count = tool_call_count + 1
                 WHERE id = ?1",
                params![msg.session_id],
            )?;
        } else {
            tx.execute(
                "UPDATE sessions SET message_count = message_count + 1 WHERE id = ?1",
                params![msg.session_id],
            )?;
        }

        tx.commit()?;
        Ok(id)
    }

    /// 按时间顺序读取会话内全部消息行。
    pub fn get_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason,
                    reasoning, reasoning_content, reasoning_details,
                    codex_reasoning_items, codex_message_items
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
                row.get::<_, f64>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<String>>(12)?,
                row.get::<_, Option<String>>(13)?,
                row.get::<_, Option<String>>(14)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (
                id,
                session_id,
                role,
                content,
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
            ) = row?;
            out.push(StoredMessage {
                id,
                session_id,
                role,
                content,
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
            });
        }
        Ok(out)
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
        self.create_session(
            new_id,
            "tauri",
            model.as_deref(),
            None,
            Some(source_id),
        )?;

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
                    session_id, role, content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason,
                    reasoning, reasoning_content, reasoning_details,
                    codex_reasoning_items, codex_message_items
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6,
                    ?7, ?8, ?9,
                    ?10, ?11, ?12,
                    ?13, ?14
                 )",
                params![
                    new_id,
                    m.role,
                    m.content,
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

        if let Some(title) = parent.and_then(|p| p.title).filter(|t| !t.trim().is_empty()) {
            let branched = format!("{title} · branch");
            let _ = self.set_session_title(new_id, &branched);
        }

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
