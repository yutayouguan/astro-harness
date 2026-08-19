//! 聊天相关 Tauri 命令：流式聊天、控制、中断恢复、token 计数、记忆查询、图片生成。

use proto::astro_service_client::AstroServiceClient;
use proto::{
    ChatControlAction, ChatControlRequest, ChatRequest, ImageRequest, MemoryQuery, SteerChatRequest,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use super::common::{bootstrap_workspace, friendly_error, open_sessions};
use super::providers::{
    cached_model_context_window, cached_model_info, cached_model_max_output_tokens,
    resolve_chat_targets, resolve_image_gen_targets, ImageGenTarget,
};
use crate::infra::grpc::{default_grpc_address, endpoint_url};

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ContextUsageItemDto {
    pub id: String,
    pub label: String,
    pub tokens: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextUsageSegmentDto {
    pub id: String,
    pub tokens: u32,
    pub count: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ContextUsageItemDto>,
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
        phase: String,
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
    UserInputCommitted {
        client_message_id: String,
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
    Citations {
        citations: String,
    },
    Done,
    Error {
        message: String,
    },
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

/// `start_chat` 前端入参（camelCase，与 invoke 字段对齐）。
///
/// 调用形态固定为 `invoke("start_chat", { request: { … } })`，**不**接受扁平顶层字段。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartChatRequest {
    pub content: String,
    pub provider: String,
    pub model: String,
    pub session_id: Option<String>,
    pub use_memory: Option<bool>,
    pub attachments: Option<Vec<ChatAttachmentDto>>,
    pub provider_id: Option<String>,
    pub thinking_enabled: Option<bool>,
    pub reasoning_effort: Option<String>,
    pub resume_json: Option<String>,
    pub keep_chat_bubbles: Option<i32>,
    pub interaction_mode: Option<String>,
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GenerateImageResult {
    pub path: String,
    pub provider: String,
    pub model: String,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// 启动流式聊天（内部走 Agent / Provider）。
///
/// `keep_chat_bubbles`：若提供，则在开跑前将会话 DB 截断到该数量的 user/assistant 气泡
///（编辑重发 / 再生用；缺失则不截断）。
#[tauri::command]
pub async fn start_chat(app: AppHandle, request: StartChatRequest) -> Result<String, String> {
    let StartChatRequest {
        content,
        provider,
        model,
        session_id,
        use_memory,
        attachments,
        provider_id,
        thinking_enabled,
        reasoning_effort,
        resume_json,
        keep_chat_bubbles,
        interaction_mode,
        project_root,
    } = request;
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
    let mut reasoning_effort = reasoning_effort
        .unwrap_or_else(|| "high".to_string())
        .trim()
        .to_ascii_lowercase();
    if reasoning_effort.is_empty() {
        reasoning_effort = "high".to_string();
    }
    // 透传 OpenRouter / 厂商档位（none/minimal/low/medium/high/xhigh/max）
    let interaction_mode = interaction_mode
        .unwrap_or_else(|| "agent".to_string())
        .trim()
        .to_ascii_lowercase();
    let interaction_mode = match interaction_mode.as_str() {
        "plan" | "ask" => interaction_mode,
        _ => "agent".to_string(),
    };
    let project_root = project_root.unwrap_or_default().trim().to_string();

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
    let auxiliary_targets =
        crate::meta::auxiliary_resolver::build_auxiliary_model_targets(&primary);
    // 优先用 models.json 缓存（与前端展示同源）；否则 LiteLLM/enrich；未知为 0（agent 侧再兜底）。
    let context_window = cached_model_context_window(&primary.provider_id, &primary.model)
        .or_else(|| {
            crate::meta::model_meta::enrich_from_id(&primary.model, &primary.backend_id, None)
                .context_window
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
        })
        .unwrap_or(0);
    // 当前模型最大输出 token（与 context_window 同源）；未知为 0，后端兜底默认。
    let max_output_tokens = cached_model_max_output_tokens(&primary.provider_id, &primary.model)
        .or_else(|| {
            crate::meta::model_meta::enrich_from_id(&primary.model, &primary.backend_id, None)
                .max_output_tokens
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
        })
        .unwrap_or(0);

    // OpenRouter default_parameters → 采样温度 + 扩展参数（top_p 等）
    let (temperature, additional_params_json) = {
        let info = cached_model_info(&primary.provider_id, &primary.model).or_else(|| {
            let enriched =
                crate::meta::model_meta::enrich_from_id(&primary.model, &primary.backend_id, None);
            if enriched.meta_source.is_empty() {
                None
            } else {
                Some(enriched)
            }
        });
        match info.as_ref().and_then(|m| m.default_parameters.as_ref()) {
            Some(dp) => {
                let temperature = dp.temperature.and_then(|t| {
                    let f = t as f32;
                    if f.is_finite() && (0.0..=2.0).contains(&f) {
                        Some(f)
                    } else {
                        None
                    }
                });
                let mut obj = serde_json::Map::new();
                if let Some(v) = dp.top_p.filter(|n| n.is_finite()) {
                    obj.insert("top_p".into(), serde_json::json!(v));
                }
                if let Some(v) = dp.top_k {
                    obj.insert("top_k".into(), serde_json::json!(v));
                }
                if let Some(v) = dp.frequency_penalty.filter(|n| n.is_finite()) {
                    obj.insert("frequency_penalty".into(), serde_json::json!(v));
                }
                if let Some(v) = dp.presence_penalty.filter(|n| n.is_finite()) {
                    obj.insert("presence_penalty".into(), serde_json::json!(v));
                }
                if let Some(v) = dp.repetition_penalty.filter(|n| n.is_finite()) {
                    obj.insert("repetition_penalty".into(), serde_json::json!(v));
                }
                let additional_params_json = if obj.is_empty() {
                    String::new()
                } else {
                    serde_json::Value::Object(obj).to_string()
                };
                (temperature, additional_params_json)
            }
            None => (None, String::new()),
        }
    };

    let app2 = app.clone();
    let sid2 = sid.clone();
    let event_name2 = event_name.clone();

    tauri::async_runtime::spawn(async move {
        let result = run_chat_stream(ChatStreamParams {
            app: &app2,
            event_name: &event_name2,
            grpc_address: &grpc_address,
            session_id: &sid2,
            content: &merged,
            images: &images,
            provider: &primary.backend_id,
            model: &primary.model,
            use_memory,
            api_key: &primary.api_key,
            base_url: &primary.base_url,
            image_targets: &image_targets,
            chat_fallbacks: &chat_fallbacks,
            auxiliary_targets: &auxiliary_targets,
            thinking_enabled,
            reasoning_effort: &reasoning_effort,
            resume_json: &resume_json,
            context_window,
            max_output_tokens,
            interaction_mode: &interaction_mode,
            project_root: &project_root,
            temperature,
            additional_params_json: &additional_params_json,
        })
        .await;

        if let Err(err) = result {
            let _ = app2.emit(
                &event_name2,
                ChatStreamEvent::Error {
                    message: friendly_error(&err),
                },
            );
            let _ = app2.emit(
                &event_name2,
                ChatStreamEvent::RunFinished {
                    run_id: String::new(),
                    outcome_type: "error".into(),
                    interrupts_json: "[]".into(),
                },
            );
            let _ = app2.emit(&event_name2, ChatStreamEvent::Done);
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

/// 将补充指令注入当前活动任务；活动 turn 已结束时返回 `false`。
#[tauri::command]
pub async fn steer_chat(
    session_id: String,
    expected_turn_id: String,
    client_message_id: String,
    content: String,
    attachments: Option<Vec<ChatAttachmentDto>>,
) -> Result<bool, String> {
    let sid = session_id.trim();
    if sid.is_empty() {
        return Ok(false);
    }
    let BuiltChatPayload {
        content: merged,
        images,
    } = build_chat_payload(&content, &attachments.unwrap_or_default());
    if merged.trim().is_empty() && images.is_empty() {
        return Ok(false);
    }
    let endpoint = endpoint_url(&default_grpc_address());
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|e| e.to_string())?;
    let response = client
        .steer_chat(SteerChatRequest {
            session_id: sid.to_string(),
            content: merged,
            images,
            expected_turn_id,
            client_message_id,
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(response.accepted)
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

/// 本地流式聊天主循环参数（由 `start_chat` 组装后传入）。
struct ChatStreamParams<'a> {
    app: &'a AppHandle,
    event_name: &'a str,
    grpc_address: &'a str,
    session_id: &'a str,
    content: &'a str,
    images: &'a [proto::ChatImageAttachment],
    provider: &'a str,
    model: &'a str,
    use_memory: bool,
    api_key: &'a str,
    base_url: &'a str,
    image_targets: &'a [ImageGenTarget],
    chat_fallbacks: &'a [proto::ChatFallbackTarget],
    auxiliary_targets: &'a [proto::AuxiliaryModelTarget],
    thinking_enabled: bool,
    reasoning_effort: &'a str,
    resume_json: &'a str,
    context_window: u32,
    max_output_tokens: u32,
    interaction_mode: &'a str,
    project_root: &'a str,
    temperature: Option<f32>,
    additional_params_json: &'a str,
}

/// 执行本地流式聊天主循环并向窗口发事件。
async fn run_chat_stream(p: ChatStreamParams<'_>) -> Result<(), String> {
    let endpoint = endpoint_url(p.grpc_address);
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|e| e.to_string())?;

    let primary = p.image_targets.first();
    let fallback = p.image_targets.get(1);

    let mut stream = client
        .chat(ChatRequest {
            session_id: p.session_id.to_string(),
            content: p.content.to_string(),
            provider: p.provider.to_string(),
            model: p.model.to_string(),
            tool_names: vec![],
            use_memory: p.use_memory,
            api_key: p.api_key.to_string(),
            base_url: p.base_url.to_string(),
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
            image_gen_fallback_video_model: fallback
                .map(|t| t.video_model.clone())
                .unwrap_or_default(),
            image_gen_fallback_music_model: fallback
                .map(|t| t.music_model.clone())
                .unwrap_or_default(),
            image_gen_fallback_tts_model: fallback.map(|t| t.tts_model.clone()).unwrap_or_default(),
            image_gen_vision_model: primary.map(|t| t.vision_model.clone()).unwrap_or_default(),
            image_gen_fallback_vision_model: fallback
                .map(|t| t.vision_model.clone())
                .unwrap_or_default(),
            thinking_enabled: p.thinking_enabled,
            reasoning_effort: p.reasoning_effort.to_string(),
            resume_json: p.resume_json.to_string(),
            chat_fallbacks: p.chat_fallbacks.to_vec(),
            images: p.images.to_vec(),
            auxiliary_targets: p.auxiliary_targets.to_vec(),
            context_window: p.context_window,
            max_output_tokens: p.max_output_tokens,
            interaction_mode: p.interaction_mode.to_string(),
            project_root: p.project_root.to_string(),
            temperature: p.temperature,
            additional_params_json: p.additional_params_json.to_string(),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();

    let mut saw_error = false;
    let mut saw_terminal = false;
    let mut saw_done = false;
    while let Some(event) = stream.message().await.map_err(|e| e.to_string())? {
        match event.payload {
            Some(proto::chat_event::Payload::Token(token)) => {
                let _ = p
                    .app
                    .emit(p.event_name, ChatStreamEvent::Token { content: token });
            }
            Some(proto::chat_event::Payload::Reasoning(reasoning)) => {
                let _ = p.app.emit(
                    p.event_name,
                    ChatStreamEvent::Reasoning { content: reasoning },
                );
            }
            Some(proto::chat_event::Payload::CitationsJson(json)) => {
                let _ = p
                    .app
                    .emit(p.event_name, ChatStreamEvent::Citations { citations: json });
            }
            Some(proto::chat_event::Payload::ToolCall(tc)) => {
                let _ = p.app.emit(
                    p.event_name,
                    ChatStreamEvent::ToolCall {
                        id: tc.id,
                        name: tc.name,
                        arguments_json: tc.arguments_json,
                        result: tc.result,
                        phase: tc.phase,
                        media: tc.media.into_iter().map(media_asset_dto).collect(),
                    },
                );
            }
            Some(proto::chat_event::Payload::ToolCallDelta(d)) => {
                let _ = p.app.emit(
                    p.event_name,
                    ChatStreamEvent::ToolCallDelta {
                        index: d.index,
                        id: d.id,
                        name: d.name,
                        arguments: d.arguments,
                    },
                );
            }
            Some(proto::chat_event::Payload::MemoryUpdate(mu)) => {
                let _ = p.app.emit(
                    p.event_name,
                    ChatStreamEvent::MemoryUpdate {
                        operation: mu.operation,
                        content: mu.content,
                    },
                );
            }
            Some(proto::chat_event::Payload::Hook(h)) => {
                let _ = p.app.emit(
                    p.event_name,
                    ChatStreamEvent::Hook {
                        name: h.name,
                        detail: h.detail,
                        outcome: h.outcome,
                    },
                );
            }
            Some(proto::chat_event::Payload::Usage(u)) => {
                let _ = p.app.emit(
                    p.event_name,
                    ChatStreamEvent::Usage {
                        prompt_tokens: u.prompt_tokens,
                        completion_tokens: u.completion_tokens,
                        total_tokens: u.total_tokens,
                    },
                );
            }
            Some(proto::chat_event::Payload::ContextUsage(cu)) => {
                let _ = p.app.emit(
                    p.event_name,
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
                                items: s
                                    .items
                                    .into_iter()
                                    .map(|it| ContextUsageItemDto {
                                        id: it.id,
                                        label: it.label,
                                        tokens: it.tokens,
                                    })
                                    .collect(),
                            })
                            .collect(),
                        updated_at: cu.updated_at,
                        recommend_compact: cu.recommend_compact,
                    },
                );
            }
            Some(proto::chat_event::Payload::RunStarted(rs)) => {
                let _ = p.app.emit(
                    p.event_name,
                    ChatStreamEvent::RunStarted {
                        thread_id: rs.thread_id,
                        run_id: rs.run_id,
                    },
                );
            }
            Some(proto::chat_event::Payload::UserInputCommitted(event)) => {
                let _ = p.app.emit(
                    p.event_name,
                    ChatStreamEvent::UserInputCommitted {
                        client_message_id: event.client_message_id,
                    },
                );
            }
            Some(proto::chat_event::Payload::Activity(a)) => {
                let _ = p.app.emit(
                    p.event_name,
                    ChatStreamEvent::Activity {
                        message_id: a.message_id,
                        activity_type: a.activity_type,
                        content_json: a.content_json,
                        replace: a.replace,
                    },
                );
            }
            Some(proto::chat_event::Payload::RunFinished(rf)) => {
                saw_terminal = true;
                let interrupts_json = serialize_interrupts(&rf.interrupts);
                let _ = p.app.emit(
                    p.event_name,
                    ChatStreamEvent::RunFinished {
                        run_id: rf.run_id,
                        outcome_type: rf.outcome_type,
                        interrupts_json,
                    },
                );
            }
            Some(proto::chat_event::Payload::Done(true)) => {
                saw_done = true;
                // 不立即结束：Done 之后仍可能有 background review 的 MemoryUpdate
                let _ = p.app.emit(p.event_name, ChatStreamEvent::Done);
                // 自动进化：默认关；命令内自守冷却/日限额/最低新决策，仅生成待审提案
                super::evolution_run::spawn_maybe_auto_evolution(p.app.clone());
                // 策展到期：仅报告，不入队
                super::evolution_run::spawn_maybe_curator(p.app.clone());
            }
            Some(proto::chat_event::Payload::Error(err)) => {
                saw_error = true;
                let _ = p.app.emit(
                    p.event_name,
                    ChatStreamEvent::Error {
                        message: friendly_error(&err),
                    },
                );
            }
            _ => {}
        }
    }

    // 兼容尚未发出统一终态的旧 backend / 早期失败路径。
    if saw_error && !saw_terminal {
        let _ = p.app.emit(
            p.event_name,
            ChatStreamEvent::RunFinished {
                run_id: String::new(),
                outcome_type: "error".into(),
                interrupts_json: "[]".into(),
            },
        );
    }
    if saw_error && !saw_done {
        let _ = p.app.emit(p.event_name, ChatStreamEvent::Done);
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

#[tauri::command]
pub async fn count_tokens(model: String) -> Result<u32, String> {
    let grpc_address = default_grpc_address();
    let endpoint = endpoint_url(&grpc_address);
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|e| friendly_error(&e.to_string()))?;
    let result = client
        .count_tokens(proto::CountTokensRequest {
            agent_id: String::new(),
            model,
            messages_json: String::new(),
            tools_json: String::new(),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(result.input_tokens)
}
