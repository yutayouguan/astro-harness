//! 会话管理 Tauri 命令：历史记录、分叉、归档、置顶、删除、标题生成。

use serde::Serialize;
use tauri::AppHandle;
use uuid::Uuid;

use super::common::open_sessions;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentSessionDto {
    pub session_id: String,
    pub project_id: Option<String>,
    pub summary: String,
    pub created_at: Option<String>,
    pub end_reason: Option<String>,
    pub archived_at: Option<String>,
    pub pinned_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatHistoryActivityDto {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub input: Option<String>,
    pub output: Option<String>,
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<ChatHistoryMediaDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatHistoryMediaDto {
    pub kind: String,
    pub path: String,
}

/// 从 `messages.media_json`（MediaAsset 数组）提取 UI 预览用 kind/path。
fn history_media_from_json(media: Option<&serde_json::Value>) -> Vec<ChatHistoryMediaDto> {
    let Some(serde_json::Value::Array(arr)) = media else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            let kind = item.get("kind")?.as_str()?;
            let kind =
                match kind {
                    "image" | "video" | "audio" | "html" => kind,
                    "file" => {
                        // 文件类：仅 html 进入内嵌预览
                        let path = media_ref_path(item.get("reference")?)?;
                        if path.rsplit('.').next().is_some_and(|e| {
                            matches!(e.to_ascii_lowercase().as_str(), "html" | "htm")
                        }) {
                            "html"
                        } else {
                            return None;
                        }
                    }
                    _ => return None,
                };
            let path = media_ref_path(item.get("reference")?)?;
            if path.is_empty() {
                return None;
            }
            Some(ChatHistoryMediaDto {
                kind: kind.to_string(),
                path: path.to_string(),
            })
        })
        .collect()
}

fn media_ref_path(reference: &serde_json::Value) -> Option<&str> {
    reference
        .get("workspace_path")
        .or_else(|| reference.get("data_url"))
        .or_else(|| reference.get("remote_uri"))
        .and_then(|v| v.as_str())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatHistoryMessageDto {
    pub id: String,
    pub role: String,
    pub content: String,
    pub reasoning: Option<String>,
    pub activities: Vec<ChatHistoryActivityDto>,
    pub segments: Option<serde_json::Value>,
    pub ui_surfaces: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatHistoryDto {
    pub session_id: Option<String>,
    pub messages: Vec<ChatHistoryMessageDto>,
    /// 会话结束原因（如 `compacted`）；未结束为 `None`
    pub end_reason: Option<String>,
    /// 结束时间（epoch 秒）；未结束为 `None`
    pub ended_at: Option<f64>,
    /// 临时 Side 会话不进入普通会话列表，离开时丢弃。
    pub ephemeral: bool,
    pub parent_session_id: Option<String>,
    /// 模型仍可见、但 UI 不重复展示的继承回合数。
    pub excluded_turn_count: i64,
}

fn exclude_inherited_turns(
    messages: Vec<ChatHistoryMessageDto>,
    inherited_turn_count: i64,
) -> Vec<ChatHistoryMessageDto> {
    let inherited_turn_count = inherited_turn_count.max(0) as usize;
    if inherited_turn_count == 0 {
        return messages;
    }
    let mut seen_users = 0usize;
    let first_local = messages.iter().position(|message| {
        if message.role == "user" {
            seen_users += 1;
        }
        seen_users > inherited_turn_count
    });
    first_local
        .map(|index| messages.into_iter().skip(index).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_session_filter(filter: &str) -> Result<session::SessionListFilter, String> {
    match filter {
        "active" => Ok(session::SessionListFilter::Active),
        "archived" => Ok(session::SessionListFilter::Archived),
        _ => Err("invalid session filter".into()),
    }
}

fn validate_session_title(title: &str) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("session title cannot be empty".into());
    }
    Ok(title.to_string())
}

fn recent_session_dto(s: session::RecentSession) -> RecentSessionDto {
    let summary = s
        .title
        .filter(|t| !t.trim().is_empty())
        .or(s.preview)
        .unwrap_or_default();
    let created_at =
        chrono::DateTime::from_timestamp(s.started_at as i64, 0).map(|dt| dt.to_rfc3339());
    let archived_at = s
        .archived_at
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp as i64, 0))
        .map(|dt| dt.to_rfc3339());
    let pinned_at = s
        .pinned_at
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp as i64, 0))
        .map(|dt| dt.to_rfc3339());
    RecentSessionDto {
        session_id: s.id,
        project_id: s.project_id,
        summary,
        created_at,
        end_reason: s.end_reason,
        archived_at,
        pinned_at,
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// 读取本地会话消息，用于进入「智能对话」时恢复上次记录。
/// `session_id` 为空时取最近一条消息所属会话。
#[tauri::command]
pub async fn get_chat_history(
    session_id: Option<String>,
    limit: Option<i32>,
) -> Result<ChatHistoryDto, String> {
    let store = open_sessions().await?;
    let limit = limit.unwrap_or(200).clamp(1, 500) as usize;

    let sid = match session_id.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => match store.latest_session_id().await.map_err(|e| e.to_string())? {
            Some(s) => s,
            None => {
                return Ok(ChatHistoryDto {
                    session_id: None,
                    messages: vec![],
                    end_reason: None,
                    ended_at: None,
                    ephemeral: false,
                    parent_session_id: None,
                    excluded_turn_count: 0,
                });
            }
        },
    };

    let meta = store.get_session(&sid).await.map_err(|e| e.to_string())?;
    let end_reason = meta.as_ref().and_then(|s| s.end_reason.clone());
    let ended_at = meta.as_ref().and_then(|s| s.ended_at);
    let ephemeral = meta
        .as_ref()
        .is_some_and(|session| session.branch_kind.as_deref() == Some("side"));
    let parent_session_id = ephemeral
        .then(|| {
            meta.as_ref()
                .and_then(|session| session.parent_session_id.clone())
        })
        .flatten();
    let excluded_turn_count = if ephemeral {
        meta.as_ref()
            .and_then(|session| session.branch_inherited_turn_count)
            .unwrap_or(0)
            .max(0)
    } else {
        0
    };

    let mut messages = store
        .build_chat_history(&sid, usize::MAX)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|m| ChatHistoryMessageDto {
            id: format!("db-{}", m.id),
            role: m.role,
            content: m.content,
            reasoning: m.reasoning,
            activities: m
                .activities
                .into_iter()
                .map(|a| ChatHistoryActivityDto {
                    id: a.id,
                    kind: a.kind,
                    title: a.title,
                    input: a.input,
                    output: a.output,
                    status: a.status,
                    media: history_media_from_json(a.media.as_ref()),
                })
                .collect(),
            segments: m.segments,
            ui_surfaces: m.ui_surfaces,
        })
        .collect();
    if ephemeral {
        messages = exclude_inherited_turns(messages, excluded_turn_count);
    }
    if messages.len() > limit {
        messages = messages.split_off(messages.len() - limit);
    }

    Ok(ChatHistoryDto {
        session_id: Some(sid),
        messages,
        end_reason,
        ended_at,
        ephemeral,
        parent_session_id,
        excluded_turn_count,
    })
}

/// 从当前会话分支。
///
/// 新调用方使用 `source_message_id + boundary`，分别对应 Codex 的
/// `lastTurnId` / `beforeTurnId`。`keep_chat_bubbles` 仅保留给旧调用方。
#[tauri::command]
pub async fn fork_chat_session(
    source_session_id: String,
    keep_chat_bubbles: Option<i32>,
    new_session_id: Option<String>,
    source_message_id: Option<i64>,
    boundary: Option<String>,
    ephemeral: Option<bool>,
    exclude_turns: Option<bool>,
) -> Result<String, String> {
    let source = source_session_id.trim();
    if source.is_empty() {
        return Err("source_session_id 不能为空".into());
    }
    let new_id = new_session_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    if new_id == source {
        return Err("新会话 id 不能与源会话相同".into());
    }

    let store = open_sessions().await?;
    let is_ephemeral = ephemeral.unwrap_or(false);
    if is_ephemeral && exclude_turns != Some(true) {
        return Err("ephemeral fork requires exclude_turns=true".into());
    }
    let boundary = boundary.as_deref().unwrap_or("through_turn");
    if !matches!(boundary, "through_turn" | "before_turn") {
        return Err(format!("invalid fork boundary: {boundary}"));
    }
    if source_message_id.is_none() && boundary == "before_turn" {
        return Err("before_turn fork requires source_message_id".into());
    }

    if let Some(message_id) = source_message_id {
        match (boundary, is_ephemeral) {
            ("through_turn", false) => {
                store
                    .fork_session_at_user_message(source, &new_id, message_id)
                    .await
            }
            ("through_turn", true) => {
                store
                    .fork_side_session_at_user_message(source, &new_id, message_id)
                    .await
            }
            ("before_turn", false) => {
                store
                    .fork_session_before_user_message(source, &new_id, message_id)
                    .await
            }
            ("before_turn", true) => {
                store
                    .fork_side_session_before_user_message(source, &new_id, message_id)
                    .await
            }
            _ => unreachable!("fork boundary validated above"),
        }
        .map_err(|e| e.to_string())?;
    } else if is_ephemeral {
        let message_id = store
            .get_messages(source)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.id)
            .ok_or_else(|| "cannot fork an empty session".to_string())?;
        store
            .fork_side_session_at_user_message(source, &new_id, message_id)
            .await
            .map_err(|e| e.to_string())?;
    } else {
        let keep = match keep_chat_bubbles {
            Some(keep) => keep.max(0) as usize,
            None => store
                .get_messages(source)
                .await
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|message| matches!(message.role.as_str(), "user" | "assistant"))
                .count(),
        };
        store
            .fork_session(source, &new_id, keep)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(new_id)
}

/// 删除当前会话聊天气泡半开区间 `[start, end)`（0-based，仅计 user/assistant）。
#[tauri::command]
pub async fn remove_chat_bubbles(session_id: String, start: i32, end: i32) -> Result<(), String> {
    let sid = session_id.trim();
    if sid.is_empty() {
        return Err("session_id 不能为空".into());
    }
    let start = start.max(0) as usize;
    let end = end.max(0) as usize;
    if start >= end {
        return Ok(());
    }
    let store = open_sessions().await?;
    store
        .remove_chat_bubbles(sid, start, end)
        .await
        .map_err(|e| e.to_string())
}

/// 按 active / archived 筛选会话供侧栏展示。
/// `project_id` 优先；兼容旧调用仍支持 `project_root`。
#[tauri::command]
pub async fn list_sessions(
    filter: String,
    limit: Option<i32>,
    project_root: Option<String>,
    project_id: Option<String>,
) -> Result<Vec<RecentSessionDto>, String> {
    let filter = parse_session_filter(&filter)?;
    let store = open_sessions().await?;
    let limit = limit.unwrap_or(50).clamp(1, 200) as usize;
    let sessions = if let Some(pid) = project_id.filter(|s| !s.is_empty()) {
        store
            .list_sessions_by_project(filter, limit, &pid)
            .await
            .map_err(|e| e.to_string())?
    } else {
        store
            .list_sessions_filtered(filter, limit, project_root.as_deref())
            .await
            .map_err(|e| e.to_string())?
    };
    Ok(sessions.into_iter().map(recent_session_dto).collect())
}

/// 兼容旧调用：仅列出未归档会话。
#[tauri::command]
pub async fn list_recent_sessions(limit: Option<i32>) -> Result<Vec<RecentSessionDto>, String> {
    list_sessions("active".into(), limit, None, None).await
}

/// 设置会话的项目根目录（兼容旧调用，内部转 project_id）。
#[tauri::command]
pub async fn set_session_project_root(
    session_id: String,
    project_root: Option<String>,
) -> Result<(), String> {
    let store = open_sessions().await?;
    // 兼容：尝试按 root 路径查找 project 并关联
    if let Some(root) = project_root.as_deref().filter(|r| !r.is_empty()) {
        if let Ok(Some(proj)) = store.find_project_by_root(root).await {
            return store
                .assign_session_to_project(&session_id, &proj.id)
                .await
                .map_err(|e| e.to_string());
        }
    }
    store
        .set_session_project_root(&session_id, project_root.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// 重命名会话。
#[tauri::command]
pub async fn rename_session(session_id: String, title: String) -> Result<(), String> {
    let title = validate_session_title(&title)?;
    open_sessions()
        .await?
        .set_session_title(&session_id, &title)
        .await
        .map_err(|e| e.to_string())
}

/// 强制重新生成会话标题（覆盖现有标题）。
#[tauri::command]
pub async fn regenerate_session_title(
    app: AppHandle,
    session_id: String,
) -> Result<String, String> {
    use crate::infra::thread_events::{
        emit_session_event, now_ts_ms, SessionEventDto, SessionMetadataChangedDto,
    };
    use crate::meta::auxiliary_resolver::{
        primary_chat_target_for_session, resolve_auxiliary_targets, ResolvedTarget,
    };

    let sid = session_id.trim().to_string();
    if sid.is_empty() {
        return Err("session_id 不能为空".into());
    }

    let (user, assistant) = {
        let store = open_sessions().await?;
        store
            .first_turn_text(&sid)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "会话尚无完整首轮对话，无法生成标题".to_string())?
    };

    let primary = primary_chat_target_for_session(&sid).await?;
    let targets = resolve_auxiliary_targets(memory::AuxiliaryKind::TitleGeneration, &primary)?;
    let chain: Vec<&ResolvedTarget> = std::iter::once(&targets.preferred)
        .chain(targets.fallback.as_ref())
        .collect();

    let prompt = format!(
        "Generate a short chat session title for the conversation below.\n\
         Rules:\n\
         - Reply with ONLY the title text\n\
         - No quotes, markdown, or explanation\n\
         - Prefer the same language as the user message\n\
         - At most 40 characters\n\n\
         User:\n{user}\n\n\
         Assistant:\n{assistant}"
    );

    async fn complete_one(target: &ResolvedTarget, prompt: &str) -> Result<String, String> {
        use futures::StreamExt;
        use providers::types::message::Message as ProviderMessage;
        use providers::types::stream::StreamChunk;
        use providers::ProviderConfig;

        let config = ProviderConfig {
            api_key: target.api_key.clone(),
            base_url: if target.provider.endpoint.trim().is_empty() {
                None
            } else {
                Some(target.provider.endpoint.trim_end_matches('/').to_string())
            },
            model: target.model.clone(),
            temperature: 0.3,
            max_tokens: 64,
            thinking_enabled: false,
            reasoning_effort: "high".to_string(),
            additional_params: serde_json::Value::Null,
            previous_interaction_id: None,
            api_mode: String::new(),
        };
        let messages = vec![ProviderMessage::user_text(prompt)];
        let mut stream =
            providers::dispatch::chat_stream(&target.backend_id, messages, vec![], &config)
                .await
                .map_err(|e| e.to_string())?;
        let mut out = String::new();
        while let Some(item) = stream.next().await {
            let chunk = item.map_err(|e| e.to_string())?;
            if let StreamChunk::Text(token) = chunk {
                out.push_str(&token);
            }
        }
        if out.trim().is_empty() {
            return Err("模型未返回任何内容".into());
        }
        Ok(out)
    }

    let mut last_err = "title generation failed".to_string();
    let mut raw = None;
    for target in chain {
        match complete_one(target, &prompt).await {
            Ok(text) => {
                raw = Some(text);
                break;
            }
            Err(err) => {
                tracing::warn!(
                    backend = %target.backend_id,
                    error = %err,
                    "regenerate title target failed; trying next"
                );
                last_err = err;
            }
        }
    }
    let raw = raw.ok_or(last_err)?;
    let title = types::sanitize_title(&raw, 40);
    if title.is_empty() {
        return Err("模型未返回可用标题".into());
    }

    open_sessions()
        .await?
        .set_session_title(&sid, &title)
        .await
        .map_err(|e| e.to_string())?;

    emit_session_event(
        &app,
        SessionEventDto {
            session_id: Some(sid.clone()),
            agent_id: String::new(),
            ts_ms: now_ts_ms(),
            memory_updated: None,
            pending_changed: None,
            session_metadata_changed: Some(SessionMetadataChangedDto {
                title: title.clone(),
            }),
            agent_thread_changed: None,
            resync_required: None,
        },
    );

    Ok(title)
}

/// 归档会话。
#[tauri::command]
pub async fn archive_session(session_id: String) -> Result<(), String> {
    open_sessions()
        .await?
        .archive_session(&session_id)
        .await
        .map_err(|e| e.to_string())
}

/// 取消归档会话。
#[tauri::command]
pub async fn unarchive_session(session_id: String) -> Result<(), String> {
    open_sessions()
        .await?
        .unarchive_session(&session_id)
        .await
        .map_err(|e| e.to_string())
}

/// 置顶会话。
#[tauri::command]
pub async fn pin_session(session_id: String) -> Result<(), String> {
    open_sessions()
        .await?
        .pin_session(&session_id)
        .await
        .map_err(|e| e.to_string())
}

/// 取消置顶。
#[tauri::command]
pub async fn unpin_session(session_id: String) -> Result<(), String> {
    open_sessions()
        .await?
        .unpin_session(&session_id)
        .await
        .map_err(|e| e.to_string())
}

/// 先尽量释放运行时会话，再永久删除数据库记录。
///
/// `release_session` 失败（backend 未启动等）不阻断删库；release 幂等，
/// 若删库失败可重试（再次 best-effort release + 删库）。
#[tauri::command]
pub async fn delete_session_permanently(app: AppHandle, session_id: String) -> Result<(), String> {
    if let Err(e) =
        super::chat::chat_control(app.clone(), session_id.clone(), "release_session".into()).await
    {
        tracing::warn!(
            session = %session_id,
            error = %e,
            "release_session before delete failed; deleting DB anyway"
        );
    }
    open_sessions()
        .await?
        .delete_session_permanently(&session_id)
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Project commands
// ---------------------------------------------------------------------------

const DEFAULT_PROJECT_NAME: &str = "主空间";
const DEFAULT_PROJECT_ICON: &str = "astro-space";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub roots: Vec<String>,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
}

fn project_to_dto(p: session::Project) -> ProjectDto {
    ProjectDto {
        id: p.id,
        name: p.name,
        icon: p.icon,
        roots: p.roots,
        position: p.position,
        created_at: p.created_at,
        updated_at: p.updated_at,
    }
}

fn default_project_root() -> Result<String, String> {
    let root = home::agent_workspace_dir(&home::default_memory_dir(), home::DEFAULT_AGENT_ID);
    std::fs::create_dir_all(&root)
        .map_err(|e| format!("create default project workspace {}: {e}", root.display()))?;
    Ok(root.to_string_lossy().into_owned())
}

async fn ensure_default_project_in_store(
    store: &session::SessionStore,
) -> Result<ProjectDto, String> {
    let root = default_project_root()?;
    let project = store
        .ensure_default_project(DEFAULT_PROJECT_NAME, DEFAULT_PROJECT_ICON, &root)
        .await
        .map_err(|e| e.to_string())?;
    Ok(project_to_dto(project))
}

#[tauri::command]
pub async fn list_projects() -> Result<Vec<ProjectDto>, String> {
    let store = open_sessions().await?;
    ensure_default_project_in_store(&store).await?;
    Ok(store
        .list_projects()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(project_to_dto)
        .collect())
}

#[tauri::command]
pub async fn create_project(name: String, roots: Vec<String>) -> Result<ProjectDto, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("project name cannot be empty".into());
    }
    let store = open_sessions().await?;
    let root_refs: Vec<&str> = roots.iter().map(|s| s.as_str()).collect();
    let proj = store
        .create_project(name, &root_refs)
        .await
        .map_err(|e| e.to_string())?;
    Ok(project_to_dto(proj))
}

#[tauri::command]
pub async fn update_project(
    project_id: String,
    name: Option<String>,
    icon: Option<Option<String>>,
    roots: Option<Vec<String>>,
) -> Result<ProjectDto, String> {
    let store = open_sessions().await?;
    let root_strs: Option<Vec<&str>> = roots
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect());
    let icon_ref: Option<Option<&str>> = icon.as_ref().map(|opt| opt.as_deref());
    let proj = store
        .update_project(&project_id, name.as_deref(), icon_ref, root_strs.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(project_to_dto(proj))
}

#[tauri::command]
pub async fn delete_project(project_id: String) -> Result<Vec<String>, String> {
    let store = open_sessions().await?;
    store
        .delete_project(&project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn move_project(
    project_id: String,
    before_project_id: Option<String>,
) -> Result<(), String> {
    let store = open_sessions().await?;
    store
        .move_project(&project_id, before_project_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn assign_session_to_project(
    session_id: String,
    project_id: String,
) -> Result<(), String> {
    let store = open_sessions().await?;
    store
        .assign_session_to_project(&session_id, &project_id)
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Session status / side-session / project stubs
// ---------------------------------------------------------------------------

/// List status of each session (active / idle / terminated).
#[tauri::command]
pub async fn list_session_statuses() -> Result<Vec<serde_json::Value>, String> {
    // Stub added during sqlx-migration merge – full implementation pending.
    Ok(Vec::new())
}

/// Discard an ephemeral side session that is no longer needed.
#[tauri::command]
pub async fn discard_side_session(session_id: String) -> Result<(), String> {
    let store = open_sessions().await?;
    let Some(metadata) = store
        .get_session(&session_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    if metadata.branch_kind.as_deref() != Some("side") {
        return Err("refusing to discard a non-side session".into());
    }
    store
        .delete_session_permanently(&session_id)
        .await
        .map_err(|e| e.to_string())
}

/// Ensure the built-in default project exists, creating it if absent.
#[tauri::command]
pub async fn ensure_default_project() -> Result<ProjectDto, String> {
    let store = open_sessions().await?;
    ensure_default_project_in_store(&store).await
}

/// Assign session to project only if it is not already assigned.
#[tauri::command]
pub async fn assign_session_to_project_if_unassigned(
    session_id: String,
    project_id: String,
) -> Result<(), String> {
    let store = open_sessions().await?;
    // Best-effort: silently ignore if already assigned.
    let _ = store
        .assign_session_to_project(&session_id, &project_id)
        .await;
    Ok(())
}

/// 清理上次进程遗留的临时 Side 会话。
pub async fn cleanup_stale_side_sessions() -> Result<usize, String> {
    let store = open_sessions().await?;
    store
        .delete_stale_side_sessions()
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        exclude_inherited_turns, parse_session_filter, validate_session_title,
        ChatHistoryMessageDto,
    };
    use session::SessionListFilter;

    fn history_message(id: &str, role: &str) -> ChatHistoryMessageDto {
        ChatHistoryMessageDto {
            id: id.into(),
            role: role.into(),
            content: id.into(),
            reasoning: None,
            activities: Vec::new(),
            segments: None,
            ui_surfaces: None,
        }
    }

    #[test]
    fn excludes_only_inherited_turns_from_side_history() {
        let messages = vec![
            history_message("u1", "user"),
            history_message("a1", "assistant"),
            history_message("u2", "user"),
            history_message("a2", "assistant"),
        ];

        let visible = exclude_inherited_turns(messages, 1);
        assert_eq!(
            visible
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["u2", "a2"]
        );
    }

    #[test]
    fn inherited_only_side_history_is_empty() {
        let messages = vec![
            history_message("u1", "user"),
            history_message("a1", "assistant"),
        ];
        assert!(exclude_inherited_turns(messages, 1).is_empty());
    }

    #[test]
    fn parses_supported_session_filters() {
        assert_eq!(
            parse_session_filter("active").unwrap(),
            SessionListFilter::Active
        );
        assert_eq!(
            parse_session_filter("archived").unwrap(),
            SessionListFilter::Archived
        );
    }

    #[test]
    fn rejects_unsupported_session_filters() {
        assert_eq!(
            parse_session_filter("all").unwrap_err(),
            "invalid session filter"
        );
        assert_eq!(
            parse_session_filter(" active ").unwrap_err(),
            "invalid session filter"
        );
    }

    #[test]
    fn validates_and_trims_session_titles() {
        assert_eq!(
            validate_session_title("  New title  ").unwrap(),
            "New title"
        );
        assert_eq!(
            validate_session_title(" \n\t ").unwrap_err(),
            "session title cannot be empty"
        );
    }
}
