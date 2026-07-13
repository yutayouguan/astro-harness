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
