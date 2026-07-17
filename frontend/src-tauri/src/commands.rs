//! 聊天、会话、工作区与通用 Tauri 命令（前端 `invoke` 主入口）。

use proto::astro_service_client::AstroServiceClient;
use proto::{
    ChatControlAction, ChatControlRequest, ChatRequest, FileListRequest, ImageRequest, MemoryQuery,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::grpc::{default_grpc_address, endpoint_url};
use crate::providers_commands::{
    cached_model_context_window, cached_model_max_output_tokens, resolve_chat_targets,
    resolve_image_gen_targets, ImageGenTarget,
};

fn open_sessions() -> Result<session::SessionStore, String> {
    let root = home::default_memory_dir();
    memory::ensure_workspace(&root).map_err(|e| e.to_string())?;
    session::SessionStore::open_sessions_dir(&root.join("sessions")).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextUsageSegmentDto {
    pub id: String,
    pub tokens: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaAssetDto {
    pub kind: String,
    pub mime_type: String,
    pub ref_kind: String,
    pub ref_value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

fn media_asset_dto(m: proto::MediaAsset) -> MediaAssetDto {
    MediaAssetDto {
        kind: m.kind,
        mime_type: m.mime_type,
        ref_kind: m.ref_kind,
        ref_value: m.ref_value,
        label: if m.label.is_empty() {
            None
        } else {
            Some(m.label)
        },
        id: if m.id.is_empty() { None } else { Some(m.id) },
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatStreamEvent {
    Token {
        content: String,
    },
    Reasoning {
        content: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments_json: String,
        result: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        media: Vec<MediaAssetDto>,
    },
    ToolCallDelta {
        index: u32,
        id: String,
        name: String,
        arguments: String,
    },
    MemoryUpdate {
        operation: String,
        content: String,
    },
    Hook {
        name: String,
        detail: String,
        outcome: String,
    },
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
    },
    ContextUsage {
        context_window: u32,
        total_tokens: u32,
        segments: Vec<ContextUsageSegmentDto>,
        updated_at: i64,
        recommend_compact: bool,
    },
    RunStarted {
        thread_id: String,
        run_id: String,
    },
    Activity {
        message_id: String,
        activity_type: String,
        content_json: String,
        replace: bool,
    },
    RunFinished {
        run_id: String,
        outcome_type: String,
        interrupts_json: String,
    },
    Done,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentSessionDto {
    pub session_id: String,
    pub summary: String,
    pub created_at: Option<String>,
    pub end_reason: Option<String>,
    pub archived_at: Option<String>,
    pub pinned_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemorySnapshot {
    pub memory_content: String,
    pub user_content: String,
    pub sessions: Vec<SessionSnippetDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSnippetDto {
    pub session_id: String,
    pub summary: String,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntryDto {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttachmentDto {
    pub name: String,
    pub mime: String,
    pub kind: String,
    pub size: u64,
    pub data_base64: Option<String>,
    pub local_path: Option<String>,
}

/// 将字节数格式化为可读大小（B/KB/MB…）。
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// 转义 CommonMark 图片 alt-text 中的特殊字符（`[` `]` `\` 及换行）。
fn md_escape_alt(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .flat_map(|c| match c {
            '[' | ']' | '\\' => vec!['\\', c],
            _ => vec![c],
        })
        .collect()
}

/// 用户文本 + 非图附件拼成 content；图片单独走 `images`（多模态 parts）。
struct BuiltChatPayload {
    content: String,
    images: Vec<proto::ChatImageAttachment>,
}

/// 把用户文本与附件拆成文本 content 与图片附件列表。
fn build_chat_payload(content: &str, attachments: &[ChatAttachmentDto]) -> BuiltChatPayload {
    if attachments.is_empty() {
        return BuiltChatPayload {
            content: content.to_string(),
            images: vec![],
        };
    }

    let mut out = String::new();
    let mut images = Vec::new();
    if !content.trim().is_empty() {
        out.push_str(content.trim());
        out.push_str("\n\n");
    }
    out.push_str("---\n[多媒体附件]\n");
    out.push_str("用户在本轮消息中附带了以下多媒体内容，请结合它们理解并回答。\n");

    for (i, att) in attachments.iter().enumerate() {
        out.push_str(&format!(
            "\n{}. [{}] {} ({}, {})\n",
            i + 1,
            att.kind,
            att.name,
            att.mime,
            format_size(att.size)
        ));

        if let Some(data) = att.data_base64.as_ref().filter(|s| !s.is_empty()) {
            if att.kind == "image" {
                // 控制体积：过大图片只保留元数据，不进 multimodal parts
                if data.len() <= 600_000 {
                    let mime = if att.mime.trim().is_empty() {
                        "image/png".to_string()
                    } else {
                        att.mime.clone()
                    };
                    images.push(proto::ChatImageAttachment {
                        mime,
                        data_base64: data.clone(),
                    });
                    if let Some(p) = att.local_path.as_deref().filter(|s| !s.is_empty()) {
                        out.push_str(&format!("   ![{}](<{}>)\n", md_escape_alt(&att.name), p));
                    } else {
                        out.push_str("   (图片已作为多模态附件发送)\n");
                    }
                } else if let Some(p) = att.local_path.as_deref().filter(|s| !s.is_empty()) {
                    out.push_str(&format!(
                        "   (图片体积较大，可通过 vision 工具读取)\n   ![{}](<{}>)\n",
                        md_escape_alt(&att.name),
                        p
                    ));
                } else {
                    out.push_str("   (图片已附带，体积较大，仅提供元数据；请结合文件名理解)\n");
                }
            } else if att.mime.starts_with("text/")
                || att.name.ends_with(".md")
                || att.name.ends_with(".txt")
                || att.name.ends_with(".json")
                || att.name.ends_with(".csv")
                || att.name.ends_with(".rs")
                || att.name.ends_with(".ts")
                || att.name.ends_with(".tsx")
                || att.name.ends_with(".js")
                || att.name.ends_with(".py")
            {
                if let Some(text) = decode_base64_approx(data) {
                    let clipped: String = text.chars().take(8000).collect();
                    out.push_str("   --- file content ---\n");
                    out.push_str(&clipped);
                    if text.chars().count() > 8000 {
                        out.push_str("\n   ...[truncated]");
                    }
                    out.push('\n');
                }
            } else {
                out.push_str(&format!(
                    "   (已附带二进制数据 {} bytes base64)\n",
                    data.len()
                ));
            }
        } else if att.kind == "image" {
            if let Some(p) = att.local_path.as_deref().filter(|s| !s.is_empty()) {
                out.push_str(&format!(
                    "   (图片体积较大，可通过 vision 工具读取)\n   ![{}](<{}>)\n",
                    md_escape_alt(&att.name),
                    p
                ));
            } else {
                out.push_str("   (仅元数据：体积较大或类型不支持内联)\n");
            }
        } else {
            out.push_str("   (仅元数据：体积较大或类型不支持内联)\n");
        }
    }

    BuiltChatPayload {
        content: out,
        images,
    }
}

/// 粗略估算 / 解码 Base64 附件体积，用于上限检查。
fn decode_base64_approx(input: &str) -> Option<String> {
    // 轻量解码：仅用于小文本附件预览，失败则跳过正文
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = cleaned.as_bytes();
    let mut buffer = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut bit_buf: u32 = 0;
    let mut bit_count: i8 = 0;
    for &b in bytes {
        if b == b'=' {
            break;
        }
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        bit_buf = (bit_buf << 6) | u32::from(val);
        bit_count += 6;
        if bit_count >= 8 {
            bit_count -= 8;
            buffer.push(((bit_buf >> bit_count) & 0xFF) as u8);
        }
    }
    String::from_utf8(buffer).ok()
}

#[derive(Debug, Clone, Serialize)]
pub struct AppConfigDto {
    pub grpc_address: String,
    pub memory_dir: String,
    pub workspace_dir: String,
    pub active_agent_id: String,
    pub agents: Vec<AgentInfoDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfoDto {
    pub id: String,
    pub name: String,
    pub path: String,
    pub is_default: bool,
    pub is_active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vibe: Option<String>,
}

/// 返回本机 Astro 记忆根目录。
fn memory_dir() -> String {
    home::default_memory_dir().to_string_lossy().into_owned()
}

/// 返回当前（或指定）Agent 工作区路径。
fn workspace_dir() -> String {
    home::default_agent_workspace_dir()
        .to_string_lossy()
        .into_owned()
}

/// 将内部 AgentInfo 转为前端 DTO。
fn agent_info_dto(a: home::AgentInfo) -> AgentInfoDto {
    AgentInfoDto {
        id: a.id,
        name: a.name,
        path: a.path,
        is_default: a.is_default,
        is_active: a.is_active,
        emoji: a.emoji,
        avatar: a.avatar,
        vibe: a.vibe,
    }
}

/// 确保并返回记忆根路径。
fn memory_root() -> std::path::PathBuf {
    home::default_memory_dir()
}

/// 引导默认工作区文件结构。
fn bootstrap_workspace() -> Result<(), String> {
    memory::ensure_default_workspace()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 在记忆沙箱内解析相对/绝对路径，拒绝越界。
fn resolve_memory_path(path: &str) -> Result<std::path::PathBuf, String> {
    let memory = memory_root();
    let p = std::path::PathBuf::from(path);
    // Path::starts_with is lexical — reject `..` so `{memory}/../outside` cannot pass.
    if !crate::fs_ops::is_lexically_under(&memory, &p) {
        return Err("只能访问记忆目录内的路径".into());
    }
    // When the path exists, re-check after resolving symlinks.
    if p.exists() {
        if let (Ok(memory_canon), Ok(path_canon)) = (memory.canonicalize(), p.canonicalize()) {
            if !path_canon.starts_with(&memory_canon) {
                return Err("只能访问记忆目录内的路径".into());
            }
        }
    }
    Ok(p)
}

/// 净化文件/目录名，去掉危险字符。
fn sanitize_entry_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("名称不能为空".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("名称不能包含路径分隔符".into());
    }
    if name == "." || name == ".." {
        return Err("无效的名称".into());
    }
    Ok(name.to_string())
}

/// 比较两条路径是否指向同一位置（规范化后）。
fn paths_equal(a: &std::path::Path, b: &std::path::Path) -> bool {
    if a == b {
        return true;
    }
    std::fs::canonicalize(a)
        .ok()
        .zip(std::fs::canonicalize(b).ok())
        .map(|(ca, cb)| ca == cb)
        .unwrap_or(false)
}

/// 将目录项转为前端 FileEntry DTO。
fn file_entry_dto(path: &std::path::Path, name: &str, is_dir: bool) -> FileEntryDto {
    let size = if is_dir {
        0
    } else {
        std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0)
    };
    FileEntryDto {
        path: path.to_string_lossy().to_string(),
        name: name.to_string(),
        is_dir,
        size,
    }
}

/// 读取当前应用 / Agent 运行时配置快照。
#[tauri::command]
pub async fn get_config() -> Result<AppConfigDto, String> {
    bootstrap_workspace()?;
    let root = memory_root();
    let agents = home::list_agents(&root)
        .into_iter()
        .map(agent_info_dto)
        .collect();
    Ok(AppConfigDto {
        grpc_address: default_grpc_address(),
        memory_dir: memory_dir(),
        workspace_dir: workspace_dir(),
        active_agent_id: home::active_agent_id(&root),
        agents,
    })
}

/// 列出本机全部 Agent 及其工作区路径。
#[tauri::command]
pub async fn list_agents() -> Result<Vec<AgentInfoDto>, String> {
    bootstrap_workspace()?;
    Ok(home::list_agents(&memory_root())
        .into_iter()
        .map(agent_info_dto)
        .collect())
}

/// 创建新 Agent 记忆空间（可选激活）。
#[tauri::command]
pub async fn create_agent(name: String) -> Result<AgentInfoDto, String> {
    bootstrap_workspace()?;
    let info = home::create_agent(&memory_root(), &name).map_err(|e| e.to_string())?;
    Ok(agent_info_dto(info))
}

/// 切换当前活跃 Agent。
#[tauri::command]
pub async fn set_active_agent(agent_id: String) -> Result<AppConfigDto, String> {
    bootstrap_workspace()?;
    home::set_active_agent(&memory_root(), &agent_id).map_err(|e| e.to_string())?;
    get_config().await
}

/// 创建引导页：暂存 Agent 图标（Lucide SVG / 上传图片），待 create_agent 时写入工作区。
#[tauri::command]
pub async fn set_pending_agent_icon(
    kind: String,
    data_base64: String,
    file_name: String,
) -> Result<(), String> {
    let kind = home::config::agent_icons::AgentIconKind::from_str(&kind)
        .ok_or_else(|| format!("未知图标类型: {kind}"))?;
    let bytes = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode(data_base64.trim())
            .map_err(|e| format!("图标 base64 无效: {e}"))?
    };
    home::config::agent_icons::set_pending_agent_icon(&memory_root(), kind, &bytes, &file_name)
        .map_err(|e| e.to_string())
}

/// 清除创建引导页暂存的 Agent 图标。
#[tauri::command]
pub async fn clear_pending_agent_icon(kind: Option<String>) -> Result<(), String> {
    let kind = match kind.as_deref() {
        None => None,
        Some(s) => Some(
            home::config::agent_icons::AgentIconKind::from_str(s)
                .ok_or_else(|| format!("未知图标类型: {s}"))?,
        ),
    };
    home::config::agent_icons::clear_pending_agent_icon(&memory_root(), kind)
        .map_err(|e| e.to_string())
}

/// 列出每日记忆日期。
#[tauri::command]
pub async fn list_daily_memory(agent_id: Option<String>) -> Result<Vec<String>, String> {
    bootstrap_workspace()?;
    let root = memory_root();
    let id = agent_id.unwrap_or_else(|| home::active_agent_id(&root));
    let ws = home::agent_workspace_dir(&root, &id);
    Ok(home::list_daily_memory_dates(&ws))
}

/// 读取指定日期的每日记忆 Markdown。
#[tauri::command]
pub async fn read_daily_memory(
    date: Option<String>,
    agent_id: Option<String>,
) -> Result<String, String> {
    bootstrap_workspace()?;
    let root = memory_root();
    let id = agent_id.unwrap_or_else(|| home::active_agent_id(&root));
    let ws = home::agent_workspace_dir(&root, &id);
    let date = date.unwrap_or_else(home::today_date_string);
    let path = home::ensure_daily_memory(&ws, &date).map_err(|e| e.to_string())?;
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// 写入指定日期的每日记忆。
#[tauri::command]
pub async fn write_daily_memory(
    content: String,
    date: Option<String>,
    agent_id: Option<String>,
) -> Result<(), String> {
    bootstrap_workspace()?;
    let root = memory_root();
    let id = agent_id.unwrap_or_else(|| home::active_agent_id(&root));
    let ws = home::agent_workspace_dir(&root, &id);
    let date = date.unwrap_or_else(home::today_date_string);
    let path = home::ensure_daily_memory(&ws, &date).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

/// 启动流式聊天（内部走 Agent / Provider）。
///
/// `keep_chat_bubbles`：若提供，则在开跑前将会话 DB 截断到该数量的 user/assistant 气泡
///（编辑重发 / 再生用；缺失则不截断）。
#[tauri::command]
pub async fn start_chat(
    app: AppHandle,
    content: String,
    provider: String,
    model: String,
    session_id: Option<String>,
    use_memory: Option<bool>,
    attachments: Option<Vec<ChatAttachmentDto>>,
    provider_id: Option<String>,
    thinking_enabled: Option<bool>,
    reasoning_effort: Option<String>,
    resume_json: Option<String>,
    keep_chat_bubbles: Option<i32>,
) -> Result<String, String> {
    let sid = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let use_memory = use_memory.unwrap_or(true);
    let attachments = attachments.unwrap_or_default();
    let BuiltChatPayload {
        content: merged,
        images,
    } = build_chat_payload(&content, &attachments);
    let resume_json = resume_json.unwrap_or_default();
    let grpc_address = default_grpc_address();
    let event_name = format!("chat-stream-{sid}");
    let thinking_enabled = thinking_enabled.unwrap_or(false);
    let reasoning_effort = reasoning_effort
        .unwrap_or_else(|| "high".to_string())
        .trim()
        .to_string();
    let reasoning_effort = match reasoning_effort.as_str() {
        "max" | "xhigh" => "max".to_string(),
        _ => "high".to_string(),
    };

    // 已结束（含 compacted）会话禁止再开聊，避免落到 gRPC Internal。
    {
        bootstrap_workspace()?;
        let store = open_sessions()?;
        if let Ok(Some(meta)) = store.get_session(&sid) {
            if meta.ended_at.is_some() {
                let reason = meta.end_reason.as_deref().unwrap_or("ended");
                return Err(if reason == "compacted" {
                    "会话已压实，无法继续写入；请打开续聊会话".into()
                } else {
                    format!("会话已结束（{reason}），无法继续写入")
                });
            }
        }
    }

    if let Some(keep) = keep_chat_bubbles {
        bootstrap_workspace()?;
        let store = open_sessions()?;
        store
            .ensure_session(&sid, "tauri")
            .map_err(|e| e.to_string())?;
        store
            .truncate_session_to_bubbles(&sid, keep.max(0) as usize)
            .map_err(|e| e.to_string())?;
    }

    // 从 providers.json + keyring 解析 primary 与聊天后备链
    let targets = resolve_chat_targets(provider_id.as_deref(), &provider, &model)?;
    let primary = targets
        .first()
        .cloned()
        .ok_or_else(|| "无可用聊天目标".to_string())?;
    let chat_fallbacks: Vec<proto::ChatFallbackTarget> = targets
        .iter()
        .skip(1)
        .map(|t| proto::ChatFallbackTarget {
            provider: t.backend_id.clone(),
            model: t.model.clone(),
            api_key: t.api_key.clone(),
            base_url: t.base_url.clone(),
            provider_id: t.provider_id.clone(),
        })
        .collect();
    let image_targets = resolve_image_gen_targets().unwrap_or_default();
    // 五类辅助任务（标题生成/压缩/智能审批/入梦/回合后 review）已解析目标；
    // 单个任务解析失败时静默跳过，不阻塞主聊天（见 auxiliary_resolver 内部注释）。
    let auxiliary_targets = crate::auxiliary_resolver::build_auxiliary_model_targets(&primary);
    // 优先用 models.json 缓存（与前端展示同源）；否则 LiteLLM/enrich；未知为 0（agent 侧再兜底）。
    let context_window = cached_model_context_window(&primary.provider_id, &primary.model)
        .or_else(|| {
            crate::model_meta::enrich_from_id(&primary.model, &primary.backend_id, None)
                .context_window
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
        })
        .unwrap_or(0);
    // 当前模型最大输出 token（与 context_window 同源）；未知为 0，后端兜底默认。
    let max_output_tokens = cached_model_max_output_tokens(&primary.provider_id, &primary.model)
        .or_else(|| {
            crate::model_meta::enrich_from_id(&primary.model, &primary.backend_id, None)
                .max_output_tokens
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
        })
        .unwrap_or(0);

    let app2 = app.clone();
    let sid2 = sid.clone();
    let event_name2 = event_name.clone();

    tauri::async_runtime::spawn(async move {
        let result = run_chat_stream(
            &app2,
            &event_name2,
            &grpc_address,
            &sid2,
            &merged,
            &images,
            &primary.backend_id,
            &primary.model,
            use_memory,
            &primary.api_key,
            &primary.base_url,
            &image_targets,
            &chat_fallbacks,
            &auxiliary_targets,
            thinking_enabled,
            &reasoning_effort,
            &resume_json,
            context_window,
            max_output_tokens,
        )
        .await;

        if let Err(err) = result {
            let _ = app2.emit(
                &event_name2,
                ChatStreamEvent::Error {
                    message: friendly_error(&err),
                },
            );
        }
    });

    Ok(sid)
}

/// 暂停 / 继续 / 取消进行中的聊天流。
///
/// `resume` / `stream_resume` 仅恢复流式生成，**不可**用于回答 interrupt。
#[tauri::command]
pub async fn chat_control(session_id: String, action: String) -> Result<(), String> {
    let action = match action.trim().to_ascii_lowercase().as_str() {
        "pause" => ChatControlAction::ChatControlPause,
        "resume" | "stream_resume" => ChatControlAction::ChatControlStreamResume,
        "cancel" | "stop" => ChatControlAction::ChatControlCancel,
        "new_chat" | "new-chat" => ChatControlAction::ChatControlNewChat,
        "refresh_memory" | "refresh-memory" => ChatControlAction::ChatControlRefreshMemory,
        "release_session" | "release-session" => ChatControlAction::ReleaseSession,
        other => {
            return Err(format!(
                "未知控制动作: {other}（pause|resume|stream_resume|cancel|new_chat|refresh_memory|release_session）"
            ))
        }
    };
    let grpc_address = default_grpc_address();
    let endpoint = endpoint_url(&grpc_address);
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|e| e.to_string())?;
    client
        .chat_control(ChatControlRequest {
            session_id,
            action: action as i32,
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 提交 interrupt resume（HITL 阻塞闸门）；同回合续跑，无需再调 start_chat。
#[tauri::command]
pub async fn interrupt_resume(session_id: String, resume_json: String) -> Result<(), String> {
    let items: Vec<serde_json::Value> =
        serde_json::from_str(&resume_json).map_err(|e| format!("resume_json 无效: {e}"))?;
    let resume: Vec<proto::InterruptResumeItem> = items
        .iter()
        .map(|item| proto::InterruptResumeItem {
            interrupt_id: item
                .get("interrupt_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            status: item
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("resolved")
                .to_string(),
            payload_json: match item.get("payload_json").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => item
                    .get("payload")
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            },
        })
        .collect();
    let grpc_address = default_grpc_address();
    let endpoint = endpoint_url(&grpc_address);
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|e| e.to_string())?;
    client
        .interrupt_resume(proto::InterruptResumeRequest { session_id, resume })
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 从请求/钥匙串/环境解析聊天 Provider 凭证（仅 primary；完整链见 `resolve_chat_targets`）。
#[allow(dead_code)] // cron / 其它入口仍可复用；主聊已走 resolve_chat_targets
fn resolve_chat_credentials(
    provider_id: Option<&str>,
    backend_id: &str,
) -> Result<(String, String), String> {
    let targets = resolve_chat_targets(provider_id, backend_id, "")?;
    let t = targets
        .first()
        .ok_or_else(|| "无可用聊天目标".to_string())?;
    Ok((t.api_key.clone(), t.base_url.clone()))
}

/// 执行本地流式聊天主循环并向窗口发事件。
async fn run_chat_stream(
    app: &AppHandle,
    event_name: &str,
    grpc_address: &str,
    session_id: &str,
    content: &str,
    images: &[proto::ChatImageAttachment],
    provider: &str,
    model: &str,
    use_memory: bool,
    api_key: &str,
    base_url: &str,
    image_targets: &[ImageGenTarget],
    chat_fallbacks: &[proto::ChatFallbackTarget],
    auxiliary_targets: &[proto::AuxiliaryModelTarget],
    thinking_enabled: bool,
    reasoning_effort: &str,
    resume_json: &str,
    context_window: u32,
    max_output_tokens: u32,
) -> Result<(), String> {
    let endpoint = endpoint_url(grpc_address);
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|e| e.to_string())?;

    let primary = image_targets.first();
    let fallback = image_targets.get(1);

    let mut stream = client
        .chat(ChatRequest {
            session_id: session_id.to_string(),
            content: content.to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            tool_names: vec![],
            use_memory,
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            image_gen_provider: primary.map(|t| t.provider.clone()).unwrap_or_default(),
            image_gen_model: primary.map(|t| t.model.clone()).unwrap_or_default(),
            image_gen_api_key: primary.map(|t| t.api_key.clone()).unwrap_or_default(),
            image_gen_base_url: primary.map(|t| t.base_url.clone()).unwrap_or_default(),
            image_gen_fallback_provider: fallback.map(|t| t.provider.clone()).unwrap_or_default(),
            image_gen_fallback_model: fallback.map(|t| t.model.clone()).unwrap_or_default(),
            image_gen_fallback_api_key: fallback.map(|t| t.api_key.clone()).unwrap_or_default(),
            image_gen_fallback_base_url: fallback.map(|t| t.base_url.clone()).unwrap_or_default(),
            image_gen_video_model: primary.map(|t| t.video_model.clone()).unwrap_or_default(),
            image_gen_music_model: primary.map(|t| t.music_model.clone()).unwrap_or_default(),
            image_gen_tts_model: primary.map(|t| t.tts_model.clone()).unwrap_or_default(),
            image_gen_fallback_tts_model: fallback.map(|t| t.tts_model.clone()).unwrap_or_default(),
            image_gen_vision_model: primary.map(|t| t.vision_model.clone()).unwrap_or_default(),
            image_gen_fallback_vision_model: fallback
                .map(|t| t.vision_model.clone())
                .unwrap_or_default(),
            thinking_enabled,
            reasoning_effort: reasoning_effort.to_string(),
            resume_json: resume_json.to_string(),
            chat_fallbacks: chat_fallbacks.to_vec(),
            images: images.to_vec(),
            auxiliary_targets: auxiliary_targets.to_vec(),
            context_window,
            max_output_tokens,
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();

    while let Some(event) = stream.message().await.map_err(|e| e.to_string())? {
        match event.payload {
            Some(proto::chat_event::Payload::Token(token)) => {
                let _ = app.emit(event_name, ChatStreamEvent::Token { content: token });
            }
            Some(proto::chat_event::Payload::Reasoning(reasoning)) => {
                let _ = app.emit(
                    event_name,
                    ChatStreamEvent::Reasoning { content: reasoning },
                );
            }
            Some(proto::chat_event::Payload::ToolCall(tc)) => {
                let _ = app.emit(
                    event_name,
                    ChatStreamEvent::ToolCall {
                        id: tc.id,
                        name: tc.name,
                        arguments_json: tc.arguments_json,
                        result: tc.result,
                        media: tc.media.into_iter().map(media_asset_dto).collect(),
                    },
                );
            }
            Some(proto::chat_event::Payload::ToolCallDelta(d)) => {
                let _ = app.emit(
                    event_name,
                    ChatStreamEvent::ToolCallDelta {
                        index: d.index,
                        id: d.id,
                        name: d.name,
                        arguments: d.arguments,
                    },
                );
            }
            Some(proto::chat_event::Payload::MemoryUpdate(mu)) => {
                let _ = app.emit(
                    event_name,
                    ChatStreamEvent::MemoryUpdate {
                        operation: mu.operation,
                        content: mu.content,
                    },
                );
            }
            Some(proto::chat_event::Payload::Hook(h)) => {
                let _ = app.emit(
                    event_name,
                    ChatStreamEvent::Hook {
                        name: h.name,
                        detail: h.detail,
                        outcome: h.outcome,
                    },
                );
            }
            Some(proto::chat_event::Payload::Usage(u)) => {
                let _ = app.emit(
                    event_name,
                    ChatStreamEvent::Usage {
                        prompt_tokens: u.prompt_tokens,
                        completion_tokens: u.completion_tokens,
                        total_tokens: u.total_tokens,
                    },
                );
            }
            Some(proto::chat_event::Payload::ContextUsage(cu)) => {
                let _ = app.emit(
                    event_name,
                    ChatStreamEvent::ContextUsage {
                        context_window: cu.context_window,
                        total_tokens: cu.total_tokens,
                        segments: cu
                            .segments
                            .into_iter()
                            .map(|s| ContextUsageSegmentDto {
                                id: s.id,
                                tokens: s.tokens,
                                count: s.count,
                            })
                            .collect(),
                        updated_at: cu.updated_at,
                        recommend_compact: cu.recommend_compact,
                    },
                );
            }
            Some(proto::chat_event::Payload::RunStarted(rs)) => {
                let _ = app.emit(
                    event_name,
                    ChatStreamEvent::RunStarted {
                        thread_id: rs.thread_id,
                        run_id: rs.run_id,
                    },
                );
            }
            Some(proto::chat_event::Payload::Activity(a)) => {
                let _ = app.emit(
                    event_name,
                    ChatStreamEvent::Activity {
                        message_id: a.message_id,
                        activity_type: a.activity_type,
                        content_json: a.content_json,
                        replace: a.replace,
                    },
                );
            }
            Some(proto::chat_event::Payload::RunFinished(rf)) => {
                let interrupts_json = serialize_interrupts(&rf.interrupts);
                let _ = app.emit(
                    event_name,
                    ChatStreamEvent::RunFinished {
                        run_id: rf.run_id,
                        outcome_type: rf.outcome_type,
                        interrupts_json,
                    },
                );
            }
            Some(proto::chat_event::Payload::Done(true)) => {
                // 不立即结束：Done 之后仍可能有 background review 的 MemoryUpdate
                let _ = app.emit(event_name, ChatStreamEvent::Done);
            }
            Some(proto::chat_event::Payload::Error(err)) => {
                let _ = app.emit(
                    event_name,
                    ChatStreamEvent::Error {
                        message: friendly_error(&err),
                    },
                );
                break;
            }
            _ => {}
        }
    }

    Ok(())
}

fn serialize_interrupts(items: &[proto::Interrupt]) -> String {
    let arr: Vec<_> = items
        .iter()
        .map(|i| {
            serde_json::json!({
                "id": i.id,
                "reason": i.reason,
                "message": i.message,
                "tool_call_id": i.tool_call_id,
                "response_schema_json": i.response_schema_json,
                "expires_at": i.expires_at,
                "metadata_json": i.metadata_json,
            })
        })
        .collect();
    serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
}

/// 由 MIME 推断常用文件扩展名。
fn mime_ext(mime: &str) -> &str {
    if mime.contains("jpeg") || mime.contains("jpg") {
        "jpg"
    } else if mime.contains("webp") {
        "webp"
    } else {
        "png"
    }
}

/// 通过 gRPC GenerateImage 拉取图片并落盘。
async fn generate_image_via_grpc(
    target: &ImageGenTarget,
    prompt: &str,
    width: i32,
    height: i32,
) -> Result<(Vec<u8>, String), String> {
    let grpc_address = default_grpc_address();
    let endpoint = endpoint_url(&grpc_address);
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|e| friendly_error(&e.to_string()))?;

    let mut stream = client
        .generate_image(ImageRequest {
            prompt: prompt.to_string(),
            provider: target.provider.clone(),
            model: target.model.clone(),
            width,
            height,
            count: 1,
            api_key: target.api_key.clone(),
            base_url: target.base_url.clone(),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();

    let mut image_data: Option<Vec<u8>> = None;
    let mut last_error: Option<String> = None;

    while let Some(event) = stream.message().await.map_err(|e| e.to_string())? {
        match event.payload {
            Some(proto::image_event::Payload::ImageData(data)) => {
                image_data = Some(data);
            }
            Some(proto::image_event::Payload::Error(err)) => {
                last_error = Some(err);
            }
            Some(proto::image_event::Payload::Progress(_)) => {}
            None => {}
        }
    }

    if let Some(data) = image_data {
        Ok((data, "image/png".to_string()))
    } else {
        Err(last_error.unwrap_or_else(|| format!("{} 未返回图片", target.display_name)))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GenerateImageResult {
    pub path: String,
    pub provider: String,
    pub model: String,
}

/// 按 providers 面板已开启的 Google→OpenAI 主备生成图片，写入 workspace/generated/images/
#[tauri::command]
pub async fn generate_image(
    prompt: String,
    size: Option<String>,
) -> Result<GenerateImageResult, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("prompt 不能为空".to_string());
    }

    let targets = resolve_image_gen_targets()?;
    let (width, height) = parse_size(size.as_deref());

    let mut errors: Vec<String> = Vec::new();
    for target in &targets {
        match generate_image_via_grpc(target, &prompt, width, height).await {
            Ok((data, mime)) => {
                let dir = home::generated_dir(
                    &home::default_agent_workspace_dir(),
                    home::GeneratedKind::Images,
                );
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let filename = format!(
                    "img-{}-{}.{}",
                    chrono::Local::now().format("%Y%m%d-%H%M%S"),
                    &uuid::Uuid::new_v4().simple().to_string()[..8],
                    mime_ext(&mime)
                );
                let path = dir.join(&filename);
                std::fs::write(&path, &data).map_err(|e| e.to_string())?;
                return Ok(GenerateImageResult {
                    path: path.display().to_string(),
                    provider: target.provider.clone(),
                    model: target.model.clone(),
                });
            }
            Err(err) => {
                errors.push(format!("{} ({}): {err}", target.display_name, target.model));
            }
        }
    }

    Err(format!("图片生成失败：{}", errors.join("；")))
}

/// 解析图片尺寸字符串（如 1024x1024）。
fn parse_size(size: Option<&str>) -> (i32, i32) {
    match size.map(str::trim).filter(|s| !s.is_empty()) {
        Some("512x512") => (512, 512),
        Some("1024x1536") | Some("portrait") => (1024, 1536),
        Some("1536x1024") | Some("landscape") => (1536, 1024),
        _ => (1024, 1024),
    }
}

/// 将底层错误转为面向用户的中文提示。
fn friendly_error(err: &str) -> String {
    if err.contains("transport") || err.contains("Connection refused") || err.contains("connect") {
        if crate::grpc::embed_backend_enabled() {
            "无法连接后端服务。内嵌 backend 可能尚未就绪或启动失败，请查看日志后重试。".into()
        } else {
            "无法连接后端服务。请先运行：cargo run -p backend（或去掉 ASTRO_EMBED_BACKEND=0 使用内嵌）"
                .into()
        }
    } else {
        err.to_string()
    }
}

/// 召回 MEMORY/USER 并搜索会话摘要。
#[tauri::command]
pub async fn query_memory(query: String, limit: Option<i32>) -> Result<MemorySnapshot, String> {
    let grpc_address = default_grpc_address();
    let endpoint = endpoint_url(&grpc_address);
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|e| friendly_error(&e.to_string()))?;

    let result = client
        .query_memory(MemoryQuery {
            query,
            limit: limit.unwrap_or(10),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();

    Ok(MemorySnapshot {
        memory_content: result.memory_content,
        user_content: result.user_content,
        sessions: result
            .sessions
            .into_iter()
            .map(|s| SessionSnippetDto {
                session_id: s.session_id,
                summary: s.summary,
            })
            .collect(),
    })
}

/// 读取本地会话消息，用于进入「智能对话」时恢复上次记录。
/// `session_id` 为空时取最近一条消息所属会话。
#[tauri::command]
pub async fn get_chat_history(
    session_id: Option<String>,
    limit: Option<i32>,
) -> Result<ChatHistoryDto, String> {
    let store = open_sessions()?;
    let limit = limit.unwrap_or(200).clamp(1, 500) as usize;

    let sid = match session_id.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => match store.latest_session_id().map_err(|e| e.to_string())? {
            Some(s) => s,
            None => {
                return Ok(ChatHistoryDto {
                    session_id: None,
                    messages: vec![],
                    end_reason: None,
                    ended_at: None,
                });
            }
        },
    };

    let meta = store.get_session(&sid).map_err(|e| e.to_string())?;
    let end_reason = meta.as_ref().and_then(|s| s.end_reason.clone());
    let ended_at = meta.as_ref().and_then(|s| s.ended_at);

    let messages = store
        .build_chat_history(&sid, limit)
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

    Ok(ChatHistoryDto {
        session_id: Some(sid),
        messages,
        end_reason,
        ended_at,
    })
}

/// 从当前会话分支：复制截止到第 `keep_chat_bubbles` 条聊天气泡的消息到新会话。
#[tauri::command]
pub async fn fork_chat_session(
    source_session_id: String,
    keep_chat_bubbles: i32,
    new_session_id: Option<String>,
) -> Result<String, String> {
    let source = source_session_id.trim();
    if source.is_empty() {
        return Err("source_session_id 不能为空".into());
    }
    let keep = keep_chat_bubbles.max(0) as usize;
    let new_id = new_session_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    if new_id == source {
        return Err("新会话 id 不能与源会话相同".into());
    }

    let store = open_sessions()?;
    store
        .fork_session(source, &new_id, keep)
        .map_err(|e| e.to_string())?;
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
    let store = open_sessions()?;
    store
        .remove_chat_bubbles(sid, start, end)
        .map_err(|e| e.to_string())
}

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
        summary,
        created_at,
        end_reason: s.end_reason,
        archived_at,
        pinned_at,
    }
}

/// 按 active / archived 筛选会话供侧栏展示。
#[tauri::command]
pub async fn list_sessions(
    filter: String,
    limit: Option<i32>,
) -> Result<Vec<RecentSessionDto>, String> {
    let filter = parse_session_filter(&filter)?;
    let store = open_sessions()?;
    let limit = limit.unwrap_or(50).clamp(1, 200) as usize;
    Ok(store
        .list_sessions(filter, limit)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(recent_session_dto)
        .collect())
}

/// 兼容旧调用：仅列出未归档会话。
#[tauri::command]
pub async fn list_recent_sessions(limit: Option<i32>) -> Result<Vec<RecentSessionDto>, String> {
    list_sessions("active".into(), limit).await
}

/// 重命名会话。
#[tauri::command]
pub async fn rename_session(session_id: String, title: String) -> Result<(), String> {
    let title = validate_session_title(&title)?;
    open_sessions()?
        .set_session_title(&session_id, &title)
        .map_err(|e| e.to_string())
}

/// 强制重新生成会话标题（覆盖现有标题）。
#[tauri::command]
pub async fn regenerate_session_title(
    app: AppHandle,
    session_id: String,
) -> Result<String, String> {
    use futures::StreamExt;
    use providers::registry::ProviderRegistry;
    use providers::trait_::{ChatMessage, ProviderConfig};

    use crate::auxiliary_resolver::{
        primary_chat_target_for_session, resolve_auxiliary_targets, ResolvedTarget,
    };
    use crate::session_events::{
        emit_session_event, now_ts_ms, SessionEventDto, SessionMetadataChangedDto,
    };

    let sid = session_id.trim().to_string();
    if sid.is_empty() {
        return Err("session_id 不能为空".into());
    }

    let (user, assistant) = {
        let store = open_sessions()?;
        store
            .first_turn_text(&sid)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "会话尚无完整首轮对话，无法生成标题".to_string())?
    };

    let primary = primary_chat_target_for_session(&sid)?;
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
        let registry = ProviderRegistry::default();
        let provider = registry
            .get(&target.backend_id)
            .ok_or_else(|| format!("不支持的提供商后端: {}", target.backend_id))?;
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
        };
        let messages = vec![ChatMessage::text("user", prompt)];
        let mut stream = provider
            .chat_stream(messages, vec![], &config)
            .await
            .map_err(|e| e.to_string())?;
        let mut out = String::new();
        while let Some(item) = stream.next().await {
            let chunk = item.map_err(|e| e.to_string())?;
            if let Some(token) = chunk.token {
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
    let title = common::sanitize_title(&raw, 40);
    if title.is_empty() {
        return Err("模型未返回可用标题".into());
    }

    open_sessions()?
        .set_session_title(&sid, &title)
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
        },
    );

    Ok(title)
}

/// 归档会话。
#[tauri::command]
pub async fn archive_session(session_id: String) -> Result<(), String> {
    open_sessions()?
        .archive_session(&session_id)
        .map_err(|e| e.to_string())
}

/// 取消归档会话。
#[tauri::command]
pub async fn unarchive_session(session_id: String) -> Result<(), String> {
    open_sessions()?
        .unarchive_session(&session_id)
        .map_err(|e| e.to_string())
}

/// 置顶会话。
#[tauri::command]
pub async fn pin_session(session_id: String) -> Result<(), String> {
    open_sessions()?
        .pin_session(&session_id)
        .map_err(|e| e.to_string())
}

/// 取消置顶。
#[tauri::command]
pub async fn unpin_session(session_id: String) -> Result<(), String> {
    open_sessions()?
        .unpin_session(&session_id)
        .map_err(|e| e.to_string())
}

/// 先尽量释放运行时会话，再永久删除数据库记录。
///
/// `release_session` 失败（backend 未启动等）不阻断删库；release 幂等，
/// 若删库失败可重试（再次 best-effort release + 删库）。
#[tauri::command]
pub async fn delete_session_permanently(session_id: String) -> Result<(), String> {
    if let Err(e) = chat_control(session_id.clone(), "release_session".into()).await {
        tracing::warn!(
            session = %session_id,
            error = %e,
            "release_session before delete failed; deleting DB anyway"
        );
    }
    open_sessions()?
        .delete_session_permanently(&session_id)
        .map_err(|e| e.to_string())
}

/// 在沙箱内列举工作区 / 文件空间路径。
#[tauri::command]
pub async fn list_files(path: Option<String>) -> Result<Vec<FileEntryDto>, String> {
    bootstrap_workspace()?;
    // 默认进入当前激活 Agent 的工作空间（而非整个 ~/.astro）
    let root = path.unwrap_or_else(workspace_dir);
    let root_path = std::path::PathBuf::from(&root);
    if !root_path.exists() {
        return Ok(vec![]);
    }

    let mut entries = Vec::new();
    let read = std::fs::read_dir(&root_path).map_err(|e| e.to_string())?;
    for entry in read.flatten() {
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.map(|m| m.len() as i64).unwrap_or(0);
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        entries.push(FileEntryDto {
            path: entry.path().to_string_lossy().to_string(),
            name,
            is_dir,
            size,
        });
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    Ok(entries)
}

/// 安全读取文本文件内容。
#[tauri::command]
pub async fn read_file(path: String) -> Result<String, String> {
    let p = resolve_memory_path(&path)?;
    if p.is_dir() {
        return Err("路径是目录".into());
    }
    std::fs::read_to_string(&p).map_err(|e| e.to_string())
}

/// 用系统默认应用打开记忆目录内的文件/文件夹（不走 shell.open 的 URL 校验）。
#[tauri::command]
pub async fn open_path_externally(path: String) -> Result<(), String> {
    let p = resolve_memory_path(&path)?;
    if !p.exists() {
        return Err("文件不存在".into());
    }
    open_path_with_system(&p)
}

/// 用系统默认应用打开路径。
fn open_path_with_system(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        // `start` 需要空标题参数，路径单独传入
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = path;
        Err("当前平台不支持用系统应用打开文件".into())
    }
}

/// 在系统文件管理器中显示路径。
#[tauri::command]
pub async fn reveal_in_folder(path: String) -> Result<(), String> {
    let p = resolve_memory_path(&path)?;
    if !p.exists() {
        return Err("路径不存在".into());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R"])
            .arg(&p)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", p.to_string_lossy()))
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let parent = p.parent().unwrap_or(&p);
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// 将路径移入废纸篓。
#[tauri::command]
pub async fn trash_paths(paths: Vec<String>) -> Result<u32, String> {
    let mut ok = 0u32;
    for path in paths {
        let p = resolve_memory_path(&path)?;
        let memory = memory_root();
        if paths_equal(&p, &memory) {
            return Err("不能删除数据根目录".into());
        }
        for agent in home::list_agents(&memory) {
            if paths_equal(&p, std::path::Path::new(&agent.path)) {
                return Err("不能删除 Agent 工作区根目录".into());
            }
        }
        if !p.exists() {
            continue;
        }
        trash::delete(&p).map_err(|e| e.to_string())?;
        ok += 1;
    }
    Ok(ok)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileBase64Dto {
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub base64: String,
}

/// 根据扩展名猜测 MIME 类型。
fn guess_mime(name: &str) -> String {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        "json" => "application/json",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "text/javascript",
        "ts" | "tsx" => "text/typescript",
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
    .into()
}

/// 以 Base64 读取文件（附件 / 图标）。
#[tauri::command]
pub async fn read_file_base64(path: String) -> Result<FileBase64Dto, String> {
    use base64::Engine;
    let p = resolve_memory_path(&path)?;
    if !p.is_file() {
        return Err("不是文件".into());
    }
    let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("文件过大（>32MB）".into());
    }
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    Ok(FileBase64Dto {
        mime: guess_mime(&name),
        size: bytes.len() as u64,
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        name,
    })
}

/// 读取用户主动提供的本地文件（拖放 / 系统剪贴板路径），不限记忆沙箱；上限 32MB。
#[tauri::command]
pub async fn read_user_file_base64(path: String) -> Result<FileBase64Dto, String> {
    use base64::Engine;
    let raw = path.trim();
    if raw.is_empty() {
        return Err("路径为空".into());
    }
    let p = std::path::PathBuf::from(raw);
    if !p.is_absolute() {
        return Err("需要绝对路径".into());
    }
    if !p.is_file() {
        return Err("不是文件".into());
    }
    let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("文件过大（>32MB）".into());
    }
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    Ok(FileBase64Dto {
        mime: guess_mime(&name),
        size: bytes.len() as u64,
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        name,
    })
}

/// 将记忆沙箱内文件复制到系统下载目录；返回跨平台展示路径（`~/Downloads/...`）。
#[tauri::command]
pub async fn download_file_to_downloads(path: String) -> Result<String, String> {
    let src = resolve_memory_path(&path)?;
    if !src.is_file() {
        return Err("不是文件".into());
    }
    let name = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "无效文件名".to_string())?;
    let dest_dir = home::user_downloads_dir();
    let dest = crate::fs_ops::unique_dest_name(&dest_dir, name);
    std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
    Ok(home::display_user_path(&dest))
}

/// 将 Base64 内容写入系统下载目录；返回跨平台展示路径（`~/Downloads/...`）。
#[tauri::command]
pub async fn download_bytes_to_downloads(
    filename: String,
    base64_data: String,
) -> Result<String, String> {
    use base64::Engine;
    let name = sanitize_entry_name(&filename)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data.trim())
        .map_err(|e| format!("Base64 解码失败: {e}"))?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("文件过大（>32MB）".into());
    }
    let dest_dir = home::user_downloads_dir();
    let dest = crate::fs_ops::unique_dest_name(&dest_dir, &name);
    std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    Ok(home::display_user_path(&dest))
}

/// 复制选中路径到剪贴板。
#[tauri::command]
pub async fn copy_paths_to_clipboard(paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Err("没有可复制的文件".into());
    }
    let mut resolved = Vec::new();
    for path in &paths {
        let p = resolve_memory_path(path)?;
        if !p.exists() {
            return Err(format!("路径不存在: {}", p.display()));
        }
        resolved.push(p);
    }
    crate::clipboard_files::write_paths(&resolved)
}

/// 列出系统剪贴板中的文件路径（仅文件，不含目录）；供聊天输入粘贴附件。
#[tauri::command]
pub async fn list_clipboard_file_paths() -> Result<Vec<String>, String> {
    let paths = crate::clipboard_files::read_paths()?;
    let files: Vec<String> = paths
        .into_iter()
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    if files.is_empty() {
        return Err("剪贴板中没有文件".into());
    }
    Ok(files)
}

/// 从剪贴板粘贴路径列表。
#[tauri::command]
pub async fn paste_paths_from_clipboard(
    dest_dir: String,
    mode: Option<String>,
) -> Result<Vec<FileEntryDto>, String> {
    let dest_dir = resolve_memory_path(&dest_dir)?;
    if !dest_dir.is_dir() {
        return Err("目标不是目录".into());
    }
    let sources = crate::clipboard_files::read_paths()?;
    let cut = mode.as_deref() == Some("cut");
    let mut out = Vec::new();
    for src in sources {
        if !src.exists() {
            continue;
        }
        // 外部源可不在 memory 内；目标必须在 memory 内（dest_dir 已 resolve）
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "无效文件名".to_string())?;
        let candidate = dest_dir.join(name);
        // cut + 同目录：与 move_paths 一致，no-op
        if cut && paths_equal(&src, &candidate) {
            out.push(file_entry_dto(&src, name, src.is_dir()));
            continue;
        }
        let dest = crate::fs_ops::unique_dest_name(&dest_dir, name);
        if src.is_dir() && crate::fs_ops::is_same_or_subdir(&src, &dest_dir) {
            return Err("不能粘贴到自身或其子目录".into());
        }
        if cut {
            // 仅当源也在 memory 沙箱内才允许 move；否则强制 copy
            let can_move = resolve_memory_path(&src.to_string_lossy()).is_ok();
            if can_move {
                crate::fs_ops::move_path(&src, &dest)?;
            } else {
                crate::fs_ops::copy_path_recursive(&src, &dest)?;
            }
        } else {
            crate::fs_ops::copy_path_recursive(&src, &dest)?;
        }
        let final_name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(name)
            .to_string();
        out.push(file_entry_dto(&dest, &final_name, dest.is_dir()));
    }
    if out.is_empty() {
        return Err("剪贴板中没有可粘贴的文件".into());
    }
    Ok(out)
}

/// 安全写入文本文件。
#[tauri::command]
pub async fn write_file(
    path: String,
    content: String,
    session_id: Option<String>,
    as_artifact: Option<bool>,
) -> Result<(), String> {
    let p = resolve_memory_path(&path)?;
    if p.is_dir() {
        return Err("路径是目录".into());
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, content).map_err(|e| e.to_string())?;

    if as_artifact.unwrap_or(false) {
        let mem = home::default_memory_dir();
        if let Ok(db) = artifacts::open_default(&mem) {
            let _ = db.register(
                &path,
                artifacts::ArtifactSource::AgentWrite,
                session_id.as_deref(),
                None,
                None,
            );
        }
    }
    Ok(())
}

/// Tauri 命令：create_file。
#[tauri::command]
pub async fn create_file(parent: String, name: String) -> Result<FileEntryDto, String> {
    let name = sanitize_entry_name(&name)?;
    let parent = resolve_memory_path(&parent)?;
    if !parent.is_dir() {
        return Err("父路径不是目录".into());
    }
    let path = parent.join(&name);
    if path.exists() {
        return Err("文件或目录已存在".into());
    }
    std::fs::write(&path, "").map_err(|e| e.to_string())?;
    Ok(file_entry_dto(&path, &name, false))
}

/// Tauri 命令：create_directory。
#[tauri::command]
pub async fn create_directory(parent: String, name: String) -> Result<FileEntryDto, String> {
    let name = sanitize_entry_name(&name)?;
    let parent = resolve_memory_path(&parent)?;
    if !parent.is_dir() {
        return Err("父路径不是目录".into());
    }
    let path = parent.join(&name);
    if path.exists() {
        return Err("文件或目录已存在".into());
    }
    std::fs::create_dir(&path).map_err(|e| e.to_string())?;
    Ok(file_entry_dto(&path, &name, true))
}

/// 重命名或移动沙箱内路径。
#[tauri::command]
pub async fn rename_path(path: String, new_name: String) -> Result<FileEntryDto, String> {
    let new_name = sanitize_entry_name(&new_name)?;
    let p = resolve_memory_path(&path)?;
    let memory = memory_root();
    if paths_equal(&p, &memory) {
        return Err("不能重命名数据根目录".into());
    }
    for agent in home::list_agents(&memory) {
        if paths_equal(&p, std::path::Path::new(&agent.path)) {
            return Err("不能重命名 Agent 工作区根目录".into());
        }
    }
    if !p.exists() {
        return Err("路径不存在".into());
    }
    let current_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if current_name == new_name {
        return Ok(file_entry_dto(&p, &new_name, p.is_dir()));
    }
    let parent = p.parent().ok_or_else(|| "无父目录".to_string())?;
    let dest = parent.join(&new_name);
    if dest.exists() {
        return Err("文件或目录已存在".into());
    }
    std::fs::rename(&p, &dest).map_err(|e| e.to_string())?;
    Ok(file_entry_dto(&dest, &new_name, dest.is_dir()))
}

/// 复制选中路径到目标目录。
#[tauri::command]
pub async fn copy_paths(
    sources: Vec<String>,
    dest_dir: String,
) -> Result<Vec<FileEntryDto>, String> {
    let dest_dir = resolve_memory_path(&dest_dir)?;
    if !dest_dir.is_dir() {
        return Err("目标不是目录".into());
    }
    let mut out = Vec::new();
    for src in sources {
        let src = resolve_memory_path(&src)?;
        if !src.exists() {
            return Err(format!("源不存在: {}", src.display()));
        }
        if src.is_dir() && crate::fs_ops::is_same_or_subdir(&src, &dest_dir) {
            return Err("不能复制到自身或其子目录".into());
        }
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "无效文件名".to_string())?;
        let dest = crate::fs_ops::unique_dest_name(&dest_dir, name);
        if crate::fs_ops::is_same_or_subdir(&src, &dest) {
            return Err("不能复制到自身或其子目录".into());
        }
        crate::fs_ops::copy_path_recursive(&src, &dest)?;
        let final_name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(name)
            .to_string();
        out.push(file_entry_dto(&dest, &final_name, dest.is_dir()));
    }
    Ok(out)
}

/// 移动选中路径到目标目录。
#[tauri::command]
pub async fn move_paths(
    sources: Vec<String>,
    dest_dir: String,
) -> Result<Vec<FileEntryDto>, String> {
    let dest_dir = resolve_memory_path(&dest_dir)?;
    if !dest_dir.is_dir() {
        return Err("目标不是目录".into());
    }
    let memory = memory_root();
    let mut out = Vec::new();
    for src in sources {
        let src = resolve_memory_path(&src)?;
        if paths_equal(&src, &memory) {
            return Err("不能移动数据根目录".into());
        }
        for agent in home::list_agents(&memory) {
            if paths_equal(&src, std::path::Path::new(&agent.path)) {
                return Err("不能移动 Agent 工作区根目录".into());
            }
        }
        if !src.exists() {
            return Err(format!("源不存在: {}", src.display()));
        }
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "无效文件名".to_string())?;
        let candidate = dest_dir.join(name);
        let same = paths_equal(&src, &candidate);
        let dest = if same {
            candidate
        } else {
            crate::fs_ops::unique_dest_name(&dest_dir, name)
        };
        if paths_equal(&src, &dest) {
            out.push(file_entry_dto(&src, name, src.is_dir()));
            continue;
        }
        crate::fs_ops::move_path(&src, &dest)?;
        let final_name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(name)
            .to_string();
        out.push(file_entry_dto(&dest, &final_name, dest.is_dir()));
    }
    Ok(out)
}

/// Tauri 命令：delete_path。
#[tauri::command]
pub async fn delete_path(path: String) -> Result<(), String> {
    let p = resolve_memory_path(&path)?;
    let memory = memory_root();
    if paths_equal(&p, &memory) {
        return Err("不能删除数据根目录".into());
    }
    // 不能删除任一 Agent 工作区根目录
    for agent in home::list_agents(&memory) {
        if paths_equal(&p, std::path::Path::new(&agent.path)) {
            return Err("不能删除 Agent 工作区根目录".into());
        }
    }
    if !p.exists() {
        return Err("路径不存在".into());
    }
    if p.is_dir() {
        std::fs::remove_dir_all(&p).map_err(|e| e.to_string())?;
    } else {
        std::fs::remove_file(&p).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct CronJobDto {
    pub id: String,
    pub schedule: String,
    pub task: String,
    pub title: String,
    pub agent_id: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub show_in_chat: bool,
}

#[derive(Debug, Deserialize)]
pub struct AddCronJobArgs {
    pub schedule: String,
    pub task: String,
    pub title: String,
    #[serde(default = "default_agent")]
    pub agent_id: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub show_in_chat: bool,
}

#[derive(Debug, Deserialize)]
pub struct ExtractCronJobArgs {
    /// 自然语言描述，如「每天早上九点提醒我喝水」
    pub text: String,
    /// 可选：指定提供商配置 id；缺省用当前激活提供商
    pub provider_id: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtractCronJobDto {
    pub schedule: String,
    pub task: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCronJobArgs {
    pub id: String,
    pub schedule: String,
    pub task: String,
    pub title: String,
    #[serde(default = "default_agent")]
    pub agent_id: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub show_in_chat: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CronRunDto {
    pub id: String,
    pub job_id: String,
    pub title: String,
    pub agent_id: String,
    pub schedule: String,
    pub task: String,
    pub fired_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub summary: String,
    pub output: String,
    pub error: Option<String>,
    pub session_id: Option<String>,
    pub trigger: String,
}

#[derive(Debug, Deserialize)]
pub struct ListCronRunsArgs {
    pub job_id: Option<String>,
    pub agent_id: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    #[serde(default = "default_run_limit")]
    pub limit: u32,
}

/// 定时任务运行列表默认条数上限。
fn default_run_limit() -> u32 {
    100
}

/// 解析默认 / 活跃 Agent id。
fn default_agent() -> String {
    home::DEFAULT_AGENT_ID.into()
}

/// CronJob → 前端任务 DTO。
fn job_to_dto(j: cron::CronJob) -> CronJobDto {
    CronJobDto {
        id: j.id,
        schedule: j.schedule,
        task: j.task,
        title: j.title,
        agent_id: j.agent_id,
        provider_id: j.provider_id,
        model: j.model,
        enabled: j.enabled,
        created_at: j.created_at,
        last_run_at: j.last_run_at,
        next_run_at: j.next_run_at,
        show_in_chat: j.show_in_chat,
    }
}

/// CronRun → 前端运行记录 DTO。
fn run_to_dto(r: cron::run_db::CronRunRow) -> CronRunDto {
    CronRunDto {
        id: r.id,
        job_id: r.job_id,
        title: r.title,
        agent_id: r.agent_id,
        schedule: r.schedule,
        task: r.task,
        fired_at: r.fired_at,
        finished_at: r.finished_at,
        status: r.status,
        summary: r.summary,
        output: r.output,
        error: r.error,
        session_id: r.session_id,
        trigger: r.trigger,
    }
}

/// 为定时任务解析 Provider/模型/密钥。
fn resolve_creds_for_job(
    job: &cron::CronJob,
) -> Result<agent::exec::cron::CronExecCredentials, String> {
    use crate::providers_commands::{
        find_provider, find_provider_by_backend, resolve_chat_targets,
    };

    let provider_cfg = if let Some(id) = job.provider_id.as_deref().filter(|s| !s.is_empty()) {
        find_provider(id).or_else(|_| find_provider_by_backend(id))?
    } else {
        let state = crate::providers_commands::get_providers_state()?;
        let id = state
            .active_provider_id
            .or_else(|| state.providers.first().map(|p| p.id.clone()))
            .ok_or_else(|| "请先在「模型提供商」中配置至少一个提供商".to_string())?;
        find_provider(&id)?
    };

    let model = job
        .model
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| provider_cfg.model.clone());

    let targets = resolve_chat_targets(
        Some(provider_cfg.id.as_str()),
        provider_cfg.kind.backend_id(),
        &model,
    )?;
    let primary = targets
        .first()
        .ok_or_else(|| "未能解析聊天目标链，请检查模型提供商配置".to_string())?;

    Ok(agent::exec::cron::CronExecCredentials {
        provider: primary.backend_id.clone(),
        model: primary.model.clone(),
        api_key: primary.api_key.clone(),
        base_url: primary.base_url.clone(),
        targets,
    })
}

/// 列出定时任务。
#[tauri::command]
pub async fn list_cron_jobs() -> Result<Vec<CronJobDto>, String> {
    bootstrap_workspace()?;
    let store = cron::CronStore::open_default().map_err(|e| e.to_string())?;
    let jobs = store.list().map_err(|e| e.to_string())?;
    Ok(jobs.into_iter().map(job_to_dto).collect())
}

/// 用 Extractor 从自然语言提炼 schedule/task/title（不落库，仅填表）。
#[tauri::command]
pub async fn extract_cron_job(args: ExtractCronJobArgs) -> Result<ExtractCronJobDto, String> {
    bootstrap_workspace()?;
    let text = args.text.trim();
    if text.is_empty() {
        return Err("请输入自然语言描述".into());
    }

    use crate::providers_commands::{find_provider, resolve_api_key};
    use providers::client::ProviderClient;

    let provider_cfg = if let Some(id) = args.provider_id.as_deref().filter(|s| !s.is_empty()) {
        find_provider(id)?
    } else {
        let state = crate::providers_commands::get_providers_state()?;
        let id = state
            .active_provider_id
            .or_else(|| state.providers.first().map(|p| p.id.clone()))
            .ok_or_else(|| "请先在「模型提供商」中配置至少一个提供商".to_string())?;
        find_provider(&id)?
    };

    let (has, _src, _env, key) = resolve_api_key(&provider_cfg);
    if provider_cfg.kind.requires_api_key() && !has {
        return Err(format!(
            "未配置 API Key。请在「模型提供商」中为 {} 保存密钥。",
            provider_cfg.display_name
        ));
    }
    let api_key = key.unwrap_or_default();
    let model = args
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(provider_cfg.model.as_str())
        .to_string();
    if model.trim().is_empty() {
        return Err("当前提供商未设置默认模型".into());
    }

    let base_url = if provider_cfg.endpoint.trim().is_empty() {
        None
    } else {
        Some(provider_cfg.endpoint.trim_end_matches('/').to_string())
    };
    let client = ProviderClient::from_config(provider_cfg.kind.backend_id(), api_key, base_url);

    let extractor = client
        .extractor::<cron::CronJobExtract>(&model)
        .preamble(cron::cron_extract_preamble())
        .build();

    let raw = extractor
        .extract(text)
        .await
        .map_err(|e| format!("抽取失败: {e}"))?;
    let normalized = cron::normalize_cron_extract(raw).map_err(|e| e.to_string())?;

    Ok(ExtractCronJobDto {
        schedule: normalized.schedule,
        task: normalized.task,
        title: normalized.title.unwrap_or_default(),
    })
}

/// 新增定时任务。
#[tauri::command]
pub async fn add_cron_job(args: AddCronJobArgs) -> Result<CronJobDto, String> {
    bootstrap_workspace()?;
    let store = cron::CronStore::open_default().map_err(|e| e.to_string())?;
    let j = store
        .add_job(cron::NewCronJob {
            schedule: args.schedule,
            task: args.task,
            title: args.title,
            agent_id: args.agent_id,
            provider_id: args.provider_id,
            model: args.model,
            show_in_chat: args.show_in_chat,
        })
        .map_err(|e| e.to_string())?;
    Ok(job_to_dto(j))
}

/// 更新定时任务。
#[tauri::command]
pub async fn update_cron_job(args: UpdateCronJobArgs) -> Result<CronJobDto, String> {
    bootstrap_workspace()?;
    let store = cron::CronStore::open_default().map_err(|e| e.to_string())?;
    let j = store
        .update_job(
            &args.id,
            cron::NewCronJob {
                schedule: args.schedule,
                task: args.task,
                title: args.title,
                agent_id: args.agent_id,
                provider_id: args.provider_id,
                model: args.model,
                show_in_chat: args.show_in_chat,
            },
        )
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "未找到定时任务".to_string())?;
    Ok(job_to_dto(j))
}

/// 删除定时任务。
#[tauri::command]
pub async fn remove_cron_job(id: String) -> Result<bool, String> {
    bootstrap_workspace()?;
    let store = cron::CronStore::open_default().map_err(|e| e.to_string())?;
    store.remove(&id).map_err(|e| e.to_string())
}

/// Tauri 命令：set_cron_job_enabled。
#[tauri::command]
pub async fn set_cron_job_enabled(id: String, enabled: bool) -> Result<bool, String> {
    bootstrap_workspace()?;
    let store = cron::CronStore::open_default().map_err(|e| e.to_string())?;
    store.set_enabled(&id, enabled).map_err(|e| e.to_string())
}

/// 立即执行一次定时任务。
#[tauri::command]
pub async fn run_cron_job_now(id: String) -> Result<CronRunDto, String> {
    bootstrap_workspace()?;
    let store = cron::CronStore::open_default().map_err(|e| e.to_string())?;
    let job = store
        .list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|j| j.id == id || j.id.starts_with(&id))
        .ok_or_else(|| "未找到定时任务".to_string())?;
    let creds = resolve_creds_for_job(&job)?;
    let label = {
        let t = job.title.trim();
        if !t.is_empty() {
            t.to_string()
        } else {
            let task = job.task.trim();
            let mut it = task.chars();
            let head: String = it.by_ref().take(48).collect();
            if it.next().is_some() {
                format!("{head}…")
            } else {
                head
            }
        }
    };
    let row = match agent::exec::cron::execute_job(&job, creds, "manual").await {
        Ok(row) => row,
        Err(err) => {
            common::notify_kind(
                common::ImportantKind::CronFailure,
                format!("{label}\n{err}"),
            );
            return Err(err.to_string());
        }
    };
    let _ = store.touch_last_run(&job.id, Some(row.fired_at.clone()));
    if row.status == "success" {
        let body = if row.summary.trim().is_empty() {
            label
        } else {
            format!("{label}\n{}", common::truncate_notify(&row.summary, 120))
        };
        common::notify_kind(common::ImportantKind::CronSuccess, body);
    } else {
        let detail = row
            .error
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| common::truncate_notify(s, 120))
            .or_else(|| {
                let s = row.summary.trim();
                if s.is_empty() {
                    None
                } else {
                    Some(common::truncate_notify(s, 120))
                }
            })
            .unwrap_or_else(|| row.status.clone());
        common::notify_kind(
            common::ImportantKind::CronFailure,
            format!("{label}\n{detail}"),
        );
    }
    Ok(run_to_dto(row))
}

/// 列出定时任务运行记录。
#[tauri::command]
pub async fn list_cron_runs(args: ListCronRunsArgs) -> Result<Vec<CronRunDto>, String> {
    bootstrap_workspace()?;
    let db = cron::CronRunDb::open_default().map_err(|e| e.to_string())?;
    let rows = db
        .list_filtered(cron::run_db::CronRunFilters {
            job_id: args.job_id,
            agent_id: args.agent_id,
            date_from: args.date_from,
            date_to: args.date_to,
            limit: args.limit,
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(run_to_dto).collect())
}

/// Tauri 命令：list_cron_job_runs。
#[tauri::command]
pub async fn list_cron_job_runs(id: String) -> Result<Vec<CronRunDto>, String> {
    list_cron_runs(ListCronRunsArgs {
        job_id: Some(id),
        agent_id: None,
        date_from: None,
        date_to: None,
        limit: default_run_limit(),
    })
    .await
}

// Keep unused import quiet for FileListRequest if we later wire gRPC ListFiles
#[allow(dead_code)]
/// 构造 proto FileListRequest（内部辅助）。
fn _proto_file_list_request() -> FileListRequest {
    FileListRequest {
        path: String::new(),
        depth: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_session_filter, validate_session_title};
    use session::SessionListFilter;

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
