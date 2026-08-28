use anyhow::anyhow;
use serde_json::Value;

use crate::format::format_session_search_hits;
use crate::{ConversationStore, NewMessage};

pub async fn record_message(
    store: &(impl ConversationStore + Sync),
    session_id: &str,
    role: &str,
    content: &str,
) -> anyhow::Result<i64> {
    store.ensure_session(session_id, "tauri").await?;
    store.append_message(NewMessage {
        content: Some(content),
        ..NewMessage::empty(session_id, role)
    }).await
}

pub async fn dispatch_session_tool(
    store: &(impl ConversationStore + Sync),
    name: &str,
    args: &Value,
) -> anyhow::Result<String> {
    match name {
        "session_search" => {
            let query = args["query"]
                .as_str()
                .ok_or_else(|| anyhow!("缺少 query 参数"))?;
            let limit = args["limit"].as_u64().unwrap_or(5).clamp(1, 10) as usize;
            let hits = store.search_messages(query, None, None, limit as i64).await?;
            Ok(format_session_search_hits(&hits))
        }
        other => anyhow::bail!("未知会话工具: {other}"),
    }
}
