//! 从持久 rollout 确定性重建的 SQLite 消息投影。

use std::collections::{HashMap, HashSet, VecDeque};

use agent_db::sqlx::{self, Row};
use anyhow::{Context, Result};
use serde_json::Value;
use types::message::{merge_google_thought_signature, Message, MessageContent, Role};
use types::{MediaAsset, MediaKind, MediaRef};

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

/// 用 rollout 中的 response items 替换单个会话的派生 `messages` 行。
///
/// 会话元数据及其他会话保持不变。删除、插入、计数器替换
/// 以及缺失会话的创建在同一事务中原子提交。
pub async fn rebuild_messages_from_rollout(
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

    let mut tx = store.pool.begin().await?;
    sqlx::query(
        "INSERT INTO sessions (id, source, started_at)
         VALUES (?1, 'rollout', ?2)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(session_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    let projection_started_at: f64 =
        sqlx::query("SELECT started_at FROM sessions WHERE id = ?1")
            .bind(session_id)
            .fetch_one(&mut *tx)
            .await?
            .get(0);
    sqlx::query("DELETE FROM messages WHERE session_id = ?1")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    for (index, message) in projected.into_iter().enumerate() {
        let timestamp = projection_started_at + index as f64 * 0.000_001;
        let row = NewMessage {
            session_id,
            role: message.role,
            content: Some(&message.content),
            compressed_content: message.compressed_content.as_deref(),
            tool_calls: message.tool_calls,
            tool_call_id: message.tool_call_id.as_deref(),
            tool_name: message.tool_name.as_deref(),
            token_count: None,
            finish_reason: None,
            reasoning: message.reasoning.as_deref(),
            reasoning_content: None,
            reasoning_details: message.reasoning_details,
            reasoning_items: None,
            message_items: None,
            media_json: message.media_json.as_deref(),
        };
        insert_message_row(&mut *tx, row, timestamp).await?;
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

fn project_response_items(items: &[agent_rollout::RolloutItem]) -> Result<Vec<ProjectedMessage>> {
    let messages = items
        .iter()
        .filter_map(|item| match item {
            agent_rollout::RolloutItem::ResponseItem(message)
                if !matches!(message.role, Role::System) =>
            {
                Some(message)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    validate_role_order(&messages)?;

    let mut pending_tool_names = HashMap::<String, VecDeque<String>>::new();
    let mut projected = Vec::with_capacity(messages.len());
    for message in messages {
        let tool_name = message.tool_call_id.as_ref().and_then(|tool_call_id| {
            let name = pending_tool_names
                .get_mut(tool_call_id)
                .and_then(VecDeque::pop_front);
            if pending_tool_names
                .get(tool_call_id)
                .is_some_and(VecDeque::is_empty)
            {
                pending_tool_names.remove(tool_call_id);
            }
            name
        });
        if matches!(message.role, Role::Assistant) {
            for call in message.tool_calls.iter().flatten() {
                pending_tool_names
                    .entry(call.id.clone())
                    .or_default()
                    .push_back(call.name.clone());
            }
        }
        projected.push(project_message(message, tool_name)?);
    }
    Ok(projected)
}

fn validate_role_order(messages: &[&Message]) -> Result<()> {
    for pair in messages.windows(2) {
        let duplicate_user =
            matches!(pair[0].role, Role::User) && matches!(pair[1].role, Role::User);
        let duplicate_assistant =
            matches!(pair[0].role, Role::Assistant) && matches!(pair[1].role, Role::Assistant);
        anyhow::ensure!(
            !duplicate_user && !duplicate_assistant,
            "invalid rollout message order: adjacent {:?} messages",
            pair[1].role
        );
    }
    Ok(())
}

fn project_message(message: &Message, tool_name: Option<String>) -> Result<ProjectedMessage> {
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
    let media = merged_message_media(message)?;
    let media_json = if media.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&media).context("serialize rollout media")?)
    };

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

fn merged_message_media(message: &Message) -> Result<Vec<MediaAsset>> {
    let mut merged = message.media.clone();
    if let MessageContent::Parts(parts) = &message.content {
        for part in parts {
            let media = match part.kind.as_str() {
                "image_url" => {
                    let image = part
                        .image_url
                        .as_ref()
                        .context("image_url part is missing its URL payload")?;
                    Some(media_from_part_url(MediaKind::Image, &image.url, None)?)
                }
                "audio_url" => {
                    let audio = part
                        .audio_url
                        .as_ref()
                        .context("audio_url part is missing its URL payload")?;
                    Some(media_from_part_url(
                        MediaKind::Audio,
                        &audio.url,
                        Some(&audio.mime_type),
                    )?)
                }
                "video_url" => {
                    let video = part
                        .video_url
                        .as_ref()
                        .context("video_url part is missing its URL payload")?;
                    Some(media_from_part_url(
                        MediaKind::Video,
                        &video.url,
                        Some(&video.mime_type),
                    )?)
                }
                _ => None,
            };
            if let Some(media) = media {
                merged.push(media);
            }
        }
    }

    let mut seen = HashSet::new();
    merged.retain(|media| seen.insert(media_identity(media)));
    Ok(merged)
}

fn media_from_part_url(
    kind: MediaKind,
    url: &str,
    declared_mime: Option<&str>,
) -> Result<MediaAsset> {
    let url = url.trim();
    anyhow::ensure!(!url.is_empty(), "media part URL is empty");
    if let Some(data) = url.strip_prefix("data:") {
        let metadata = data
            .split_once(',')
            .map(|(metadata, _)| metadata)
            .context("malformed data media URL")?;
        let mime_type = metadata.split(';').next().unwrap_or_default();
        anyhow::ensure!(
            !mime_type.is_empty() && mime_matches_kind(kind, mime_type),
            "data media MIME {mime_type:?} does not match {kind:?}"
        );
        return Ok(MediaAsset::data_url(kind, url, mime_type));
    }
    anyhow::ensure!(
        url.starts_with("https://") || url.starts_with("http://"),
        "unsupported media part URL: {url}"
    );
    let mime_type = declared_mime
        .map(str::trim)
        .filter(|mime| !mime.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| inferred_remote_mime(kind, url));
    anyhow::ensure!(
        mime_matches_kind(kind, &mime_type),
        "remote media MIME {mime_type:?} does not match {kind:?}"
    );
    Ok(MediaAsset {
        kind,
        mime_type,
        reference: MediaRef::RemoteUri(url.into()),
        label: None,
        id: None,
    })
}

fn mime_matches_kind(kind: MediaKind, mime_type: &str) -> bool {
    let expected = match kind {
        MediaKind::Image => "image/",
        MediaKind::Audio => "audio/",
        MediaKind::Video => "video/",
        MediaKind::File => return true,
    };
    mime_type.starts_with(expected)
}

fn inferred_remote_mime(kind: MediaKind, url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let extension = path.rsplit_once('.').map(|(_, extension)| extension);
    match (kind, extension) {
        (MediaKind::Image, Some("jpg" | "jpeg")) => "image/jpeg",
        (MediaKind::Image, Some("webp")) => "image/webp",
        (MediaKind::Image, Some("gif")) => "image/gif",
        (MediaKind::Image, Some("svg")) => "image/svg+xml",
        (MediaKind::Image, Some("png")) => "image/png",
        (MediaKind::Audio, Some("mp3")) => "audio/mpeg",
        (MediaKind::Audio, Some("wav")) => "audio/wav",
        (MediaKind::Audio, Some("m4a")) => "audio/mp4",
        (MediaKind::Video, Some("webm")) => "video/webm",
        (MediaKind::Video, Some("mov")) => "video/quicktime",
        (MediaKind::Video, Some("mp4")) => "video/mp4",
        (MediaKind::Image, _) => "image/*",
        (MediaKind::Audio, _) => "audio/*",
        (MediaKind::Video, _) => "video/*",
        (MediaKind::File, _) => "application/octet-stream",
    }
    .into()
}

fn media_identity(media: &MediaAsset) -> (MediaKindIdentity, String) {
    let kind = match media.kind {
        MediaKind::Image => MediaKindIdentity::Image,
        MediaKind::Audio => MediaKindIdentity::Audio,
        MediaKind::Video => MediaKindIdentity::Video,
        MediaKind::File => MediaKindIdentity::File,
    };
    let reference = match &media.reference {
        MediaRef::WorkspacePath(value) | MediaRef::DataUrl(value) | MediaRef::RemoteUri(value) => {
            value.as_str()
        }
    };
    (kind, reference.to_string())
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum MediaKindIdentity {
    Image,
    Audio,
    Video,
    File,
}
