use std::path::PathBuf;

use ::session::ConversationStore;
use common::message::Message;

/// 从会话存储冷启动重建 `session_messages`（权威以 DB 为准）。
pub fn hydrate_session_messages(
    sessions: &dyn ConversationStore,
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
        "user" => {
            // 优先从 media_json 还原附图；否则纯文本
            let media = parse_media_json(m.media_json.as_deref());
            if media.is_empty() {
                Message::user(&content)
            } else {
                let urls: Vec<String> = media
                    .iter()
                    .filter_map(|a| match &a.reference {
                        common::MediaRef::DataUrl(u) => Some(u.clone()),
                        _ => None,
                    })
                    .collect();
                if urls.is_empty() {
                    let mut msg = Message::user(&content);
                    msg.media = media;
                    msg
                } else {
                    let mut msg = Message::user_with_images(&content, &urls);
                    // 保留非 data-url 附件（workspace/remote）
                    for a in media {
                        if !matches!(a.reference, common::MediaRef::DataUrl(_)) {
                            msg.media.push(a);
                        }
                    }
                    msg
                }
            }
        }
        "system" => Message::system(&content),
        "assistant" => {
            let tool_calls: Option<Vec<common::message::ToolCall>> = match m.tool_calls {
                Some(v) => Some(serde_json::from_value(v)?),
                None => None,
            };
            let mut msg = match tool_calls {
                Some(calls) if !calls.is_empty() => Message::assistant_with_tools(&content, calls),
                _ => Message::assistant(&content),
            };
            msg.reasoning = m.reasoning.filter(|r| !r.is_empty());
            msg.thought_signature =
                common::message::google_thought_signature_from_details(&m.reasoning_details);
            msg
        }
        "tool" => {
            let mut msg = match m.tool_call_id.as_deref() {
                Some(id) => Message::tool_with_id(id, &content),
                None => Message::tool(&content),
            };
            let from_col = parse_media_json(m.media_json.as_deref());
            if !from_col.is_empty() {
                msg.media = from_col;
            } else {
                let (_, media) = common::extract_tool_media(&content);
                msg.media = media;
            }
            msg
        }
        other => {
            tracing::warn!(role = other, "skip unknown role when hydrating session");
            return Ok(None);
        }
    };
    msg.compressed_content = m.compressed_content;
    // assistant 等角色若带 media_json 也还原
    if msg.media.is_empty() {
        let media = parse_media_json(m.media_json.as_deref());
        if !media.is_empty() {
            msg.media = media;
        }
    }
    Ok(Some(msg))
}

fn parse_media_json(raw: Option<&str>) -> Vec<common::MediaAsset> {
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    serde_json::from_str(s).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "skip invalid media_json on hydrate");
        Vec::new()
    })
}

/// 会话级项目根：`ASTRO_SESSION_WORKTREE=1` 且存在 `ASTRO_PROJECT_ROOT`（或 cwd git root）时启用。
pub fn resolve_session_project_root() -> Option<PathBuf> {
    let flag = std::env::var("ASTRO_SESSION_WORKTREE").unwrap_or_default();
    if flag != "1" && !flag.eq_ignore_ascii_case("true") {
        return None;
    }
    delegate::resolve_project_root(None)
        .filter(|p| delegate::find_git_root(p).is_some() || p.is_dir())
}
