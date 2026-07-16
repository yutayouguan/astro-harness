use std::path::PathBuf;

use common::message::Message;
use ::session::SessionStore;

/// 从 `SessionStore` 冷启动重建 `session_messages`（权威以 DB 为准）。
pub fn hydrate_session_messages(
    sessions: &SessionStore,
    session_id: &str,
) -> anyhow::Result<Vec<Message>> {
    let stored = sessions.get_messages(session_id)?;
    let mut out = Vec::with_capacity(stored.len());
    for m in stored {
        if let Some(msg) = stored_message_to_runtime(m)? {
            out.push(msg);
        }
    }
    crate::prompt::sanitize::sanitize_tool_pairs(&mut out);
    Ok(out)
}

fn stored_message_to_runtime(m: ::session::StoredMessage) -> anyhow::Result<Option<Message>> {
    let content = m.content.unwrap_or_default();
    let mut msg = match m.role.as_str() {
        "user" => Message::user(&content),
        "system" => Message::system(&content),
        "assistant" => {
            let tool_calls: Option<Vec<common::message::ToolCall>> = match m.tool_calls {
                Some(v) => Some(serde_json::from_value(v)?),
                None => None,
            };
            match tool_calls {
                Some(calls) if !calls.is_empty() => Message::assistant_with_tools(&content, calls),
                _ => Message::assistant(&content),
            }
        }
        "tool" => {
            let mut msg = match m.tool_call_id.as_deref() {
                Some(id) => Message::tool_with_id(id, &content),
                None => Message::tool(&content),
            };
            let (_, media) = common::extract_tool_media(&content);
            msg.media = media;
            msg
        }
        other => {
            tracing::warn!(role = other, "skip unknown role when hydrating session");
            return Ok(None);
        }
    };
    msg.compressed_content = m.compressed_content;
    Ok(Some(msg))
}

/// 会话级项目根：`ASTRO_SESSION_WORKTREE=1` 且存在 `ASTRO_PROJECT_ROOT`（或 cwd git root）时启用。
pub fn resolve_session_project_root() -> Option<PathBuf> {
    let flag = std::env::var("ASTRO_SESSION_WORKTREE").unwrap_or_default();
    if flag != "1" && !flag.eq_ignore_ascii_case("true") {
        return None;
    }
    delegate::resolve_project_root(None).filter(|p| delegate::find_git_root(p).is_some() || p.is_dir())
}
