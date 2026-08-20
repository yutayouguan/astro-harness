//! Deterministic SQLite message projection rebuilt from the durable rollout.

use std::collections::HashMap;

use anyhow::{Context, Result};
use rusqlite::params;
use serde_json::Value;
use types::message::{merge_google_thought_signature, Message, Role};

use super::messages::insert_message_row;
use super::{now_epoch_secs, NewMessage, SessionStore};

struct ProjectedMessage {
    role: &'static str,
    content: String,
    compressed_content: Option<String>,
    tool_calls: Option<Value>,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    reasoning: Option<String>,
    reasoning_details: Option<Value>,
    media_json: Option<String>,
}

/// Replace one session's derived `messages` rows with the response items in its rollout.
///
/// Session metadata and every other session remain untouched. Deletion, insertion, counter
/// replacement, and creation of a missing session are committed atomically.
pub fn rebuild_messages_from_rollout(
    store: &SessionStore,
    session_id: &str,
    items: &[agent_rollout::RolloutItem],
) -> Result<()> {
    anyhow::ensure!(!session_id.trim().is_empty(), "rollout session id is empty");
    let projected = project_response_items(items)?;
    let message_count = i64::try_from(projected.len()).context("rollout message count overflow")?;
    let tool_call_count = i64::try_from(
        projected
            .iter()
            .filter(|message| message.role == "tool")
            .count(),
    )
    .context("rollout tool message count overflow")?;
    let now = now_epoch_secs()?;

    let tx = store.conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO sessions (id, source, started_at)
         VALUES (?1, 'rollout', ?2)
         ON CONFLICT(id) DO NOTHING",
        params![session_id, now],
    )?;
    let projection_started_at = tx.query_row(
        "SELECT started_at FROM sessions WHERE id = ?1",
        params![session_id],
        |row| row.get::<_, f64>(0),
    )?;
    tx.execute(
        "DELETE FROM messages WHERE session_id = ?1",
        params![session_id],
    )?;
    for (index, message) in projected.into_iter().enumerate() {
        let timestamp = projection_started_at + index as f64 * 0.000_001;
        let row = NewMessage {
            session_id,
            role: message.role,
            content: Some(&message.content),
            tool_calls: message.tool_calls,
            tool_call_id: message.tool_call_id.as_deref(),
            tool_name: message.tool_name.as_deref(),
            token_count: None,
            finish_reason: None,
            reasoning: message.reasoning.as_deref(),
            reasoning_content: None,
            reasoning_details: message.reasoning_details,
            codex_reasoning_items: None,
            codex_message_items: None,
            media_json: message.media_json.as_deref(),
        };
        insert_message_row(&tx, row, message.compressed_content.as_deref(), timestamp)?;
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

fn project_response_items(items: &[agent_rollout::RolloutItem]) -> Result<Vec<ProjectedMessage>> {
    let tool_names = items
        .iter()
        .filter_map(|item| match item {
            agent_rollout::RolloutItem::ResponseItem(message) => message.tool_calls.as_ref(),
            _ => None,
        })
        .flatten()
        .map(|call| (call.id.clone(), call.name.clone()))
        .collect::<HashMap<_, _>>();

    items
        .iter()
        .filter_map(|item| match item {
            agent_rollout::RolloutItem::ResponseItem(message) => Some(message),
            _ => None,
        })
        .filter(|message| !matches!(message.role, Role::System))
        .map(|message| project_message(message, &tool_names))
        .collect()
}

fn project_message(
    message: &Message,
    tool_names: &HashMap<String, String>,
) -> Result<ProjectedMessage> {
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
        Role::System => unreachable!("system messages are filtered before projection"),
    };
    let tool_calls = message
        .tool_calls
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .context("serialize rollout tool calls")?;
    let media_json = if message.media.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&message.media).context("serialize rollout media")?)
    };
    let tool_name = message
        .tool_call_id
        .as_ref()
        .and_then(|tool_call_id| tool_names.get(tool_call_id))
        .cloned();

    Ok(ProjectedMessage {
        role,
        content: message.content_text(),
        compressed_content: message.compressed_content.clone(),
        tool_calls,
        tool_call_id: message.tool_call_id.clone(),
        tool_name,
        reasoning: message.reasoning.clone(),
        reasoning_details: merge_google_thought_signature(
            None,
            message.thought_signature.as_deref(),
        ),
        media_json,
    })
}
