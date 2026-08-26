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
use crate::infra::thread_events::{
    accepted_turn_id, emit_chat_events, emit_session_status, managed_bridge,
    submission_failure_events, ThreadEventsBridge, THREAD_EVENTS_READY_TIMEOUT,
};

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

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatStreamEvent {
    Token {
        content: String,
    },
    AsyncMessage {
        id: String,
        content: String,
    },
    TextReconcile {
        content: String,
    },
    Reasoning {
        content: String,
    },
    ReasoningReconcile {
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
    #[allow(dead_code)] // Thread protocol currently has no first-class citation payload.
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
    pub project_id: Option<String>,
    /// Legacy compatibility; project_id now resolves the authoritative cwd.
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
        project_id,
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
    let event_name = format!("chat_stream_{sid}");
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
    let requested_project_id = project_id
        .unwrap_or_default()
        .trim()
        .to_string();
    let legacy_project_root = project_root.unwrap_or_default().trim().to_string();

    // 会话的项目归属与执行 roots 在提交前一次性确定，避免 UI effect 与首轮工具竞态。
    let (project_id, project_root, workspace_roots) = {
        bootstrap_workspace()?;
        let store = open_sessions()?;
        store.ensure_session(&sid, "tauri").map_err(|e| e.to_string())?;
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
        let workspace = super::common::workspace_dir();
        std::fs::create_dir_all(&workspace).map_err(|e| e.to_string())?;
        let default_project = store
            .ensure_default_project(std::path::Path::new(&workspace))
            .map_err(|e| e.to_string())?;
        let target_id = if requested_project_id.is_empty() {
            default_project.id.as_str()
        } else {
            requested_project_id.as_str()
        };
        if store
            .get_project(target_id)
            .map_err(|e| e.to_string())?
            .is_none()
        {
            return Err(format!("项目不存在：{target_id}"));
        }
        store
            .assign_session_to_project_if_unassigned(&sid, target_id)
            .map_err(|e| e.to_string())?;
        let project = store
            .project_for_session(&sid)
            .map_err(|e| e.to_string())?
            .unwrap_or(default_project);
        let mut roots = project.roots;
        // 仅为旧客户端保留：没有可用项目 root 时才接受旧 projectRoot。
        if roots.is_empty() && !legacy_project_root.is_empty() {
            roots.push(legacy_project_root);
        }
        let cwd = roots.first().cloned().unwrap_or(workspace);
        (project.id, cwd, roots)
    };

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

    let image_primary = image_targets.first();
    let image_fallback = image_targets.get(1);
    let chat_request = ChatRequest {
        session_id: sid.clone(),
        content: merged,
        provider: primary.backend_id.clone(),
        model: primary.model.clone(),
        tool_names: vec![],
        use_memory,
        api_key: primary.api_key.clone(),
        base_url: primary.base_url.clone(),
        image_gen_provider: image_primary
            .map(|target| target.provider.clone())
            .unwrap_or_default(),
        image_gen_model: image_primary
            .map(|target| target.model.clone())
            .unwrap_or_default(),
        image_gen_api_key: image_primary
            .map(|target| target.api_key.clone())
            .unwrap_or_default(),
        image_gen_base_url: image_primary
            .map(|target| target.base_url.clone())
            .unwrap_or_default(),
        image_gen_fallback_provider: image_fallback
            .map(|target| target.provider.clone())
            .unwrap_or_default(),
        image_gen_fallback_model: image_fallback
            .map(|target| target.model.clone())
            .unwrap_or_default(),
        image_gen_fallback_api_key: image_fallback
            .map(|target| target.api_key.clone())
            .unwrap_or_default(),
        image_gen_fallback_base_url: image_fallback
            .map(|target| target.base_url.clone())
            .unwrap_or_default(),
        image_gen_video_model: image_primary
            .map(|target| target.video_model.clone())
            .unwrap_or_default(),
        image_gen_music_model: image_primary
            .map(|target| target.music_model.clone())
            .unwrap_or_default(),
        image_gen_tts_model: image_primary
            .map(|target| target.tts_model.clone())
            .unwrap_or_default(),
        image_gen_fallback_video_model: image_fallback
            .map(|target| target.video_model.clone())
            .unwrap_or_default(),
        image_gen_fallback_music_model: image_fallback
            .map(|target| target.music_model.clone())
            .unwrap_or_default(),
        image_gen_fallback_tts_model: image_fallback
            .map(|target| target.tts_model.clone())
            .unwrap_or_default(),
        image_gen_vision_model: image_primary
            .map(|target| target.vision_model.clone())
            .unwrap_or_default(),
        image_gen_fallback_vision_model: image_fallback
            .map(|target| target.vision_model.clone())
            .unwrap_or_default(),
        thinking_enabled,
        reasoning_effort,
        resume_json,
        chat_fallbacks,
        images,
        auxiliary_targets,
        context_window,
        max_output_tokens,
        interaction_mode,
        project_root,
        temperature,
        additional_params_json,
        // Initial submissions are not queued steer messages and therefore do
        // not participate in client-side optimistic delivery reconciliation.
        client_message_id: String::new(),
        project_id,
        workspace_roots,
    };

    let bridge = managed_bridge(&app).inner().clone();
    let sid2 = sid.clone();
    let activation = bridge.activate(sid2.clone()).await;
    emit_session_status(&app, &sid, "active", Vec::new(), None);
    let app2 = app.clone();
    let event_name2 = event_name.clone();

    tauri::async_runtime::spawn(async move {
        let result = async {
            bridge.wait_ready_for(THREAD_EVENTS_READY_TIMEOUT).await?;
            let endpoint = endpoint_url(&grpc_address);
            let mut client = AstroServiceClient::connect(endpoint)
                .await
                .map_err(|error| error.to_string())?;
            let response = client
                .submit_turn(proto::SubmitTurnRequest {
                    connection_id: bridge.connection_id().into(),
                    chat: Some(chat_request),
                    mode: "start_or_steer".into(),
                    expected_turn_id: String::new(),
                })
                .await
                .map_err(|error| error.to_string())?
                .into_inner();
            let turn_id = accepted_turn_id(response)?;
            let terminal = bridge
                .bind_submitted_turn_if_current(&sid2, activation, &turn_id)
                .await;
            let delivery_acceptance =
                bridge.spawn_provisional_delivery_cleanup(sid2.clone(), activation);
            emit_chat_events(&app2, &sid2, terminal);
            let _ = delivery_acceptance.send(());
            Ok::<(), String>(())
        }
        .await;

        if let Err(err) = result {
            let is_current = bridge.fail_activation(&sid2, activation).await;
            if is_current {
                emit_session_status(
                    &app2,
                    &sid2,
                    "systemError",
                    Vec::new(),
                    Some(friendly_error(&err)),
                );
            }
            for event in submission_failure_events(is_current, friendly_error(&err)) {
                let _ = app2.emit(&event_name2, event);
            }
        }
    });

    Ok(sid)
}

/// 暂停 / 继续 / 取消进行中的聊天流。
///
/// `resume` / `stream_resume` 仅恢复流式生成，**不可**用于回答 interrupt。
fn parse_chat_control_action(action: &str) -> Result<(ChatControlAction, bool), String> {
    let parsed = match action.trim().to_ascii_lowercase().as_str() {
        "pause" => (ChatControlAction::ChatControlPause, false),
        "resume" | "stream_resume" => (ChatControlAction::ChatControlStreamResume, false),
        "cancel" | "stop" => (ChatControlAction::ChatControlCancel, false),
        "new_chat" | "new-chat" => (ChatControlAction::ChatControlNewChat, true),
        "refresh_memory" | "refresh-memory" => {
            (ChatControlAction::ChatControlRefreshMemory, false)
        }
        "release_session" | "release-session" => (ChatControlAction::ReleaseSession, true),
        other => {
            return Err(format!(
                "未知控制动作: {other}（pause|resume|stream_resume|cancel|new_chat|refresh_memory|release_session）"
            ))
        }
    };
    Ok(parsed)
}

pub(crate) async fn chat_control_rpc(
    session_id: String,
    action: ChatControlAction,
) -> Result<(), String> {
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

async fn chat_control_with_lifecycle<F, Fut>(
    bridge: &ThreadEventsBridge,
    session_id: &str,
    forget_thread: bool,
    rpc: F,
) -> Result<(), String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    if forget_thread {
        // The backend RPC is the release boundary. The bridge holds the same per-thread gate used
        // by activation across this await, preventing either a pre-RPC or post-RPC ABA window.
        bridge.forget_thread_through(session_id, rpc).await
    } else {
        rpc().await
    }
}

#[tauri::command]
pub async fn chat_control(
    app: AppHandle,
    session_id: String,
    action: String,
) -> Result<(), String> {
    let (action, forget_thread) = parse_chat_control_action(&action)?;
    let bridge = managed_bridge(&app);
    let rpc_session_id = session_id.clone();
    chat_control_with_lifecycle(&bridge, &session_id, forget_thread, move || {
        chat_control_rpc(rpc_session_id, action)
    })
    .await
}

/// 提交 interrupt resume（HITL 阻塞闸门）；同回合续跑，无需再调 start_chat。
#[tauri::command]
pub async fn interrupt_resume(
    app: AppHandle,
    session_id: String,
    resume_json: String,
) -> Result<(), String> {
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
        .interrupt_resume(proto::InterruptResumeRequest {
            session_id: session_id.clone(),
            resume,
        })
        .await
        .map_err(|e| e.to_string())?;
    emit_session_status(&app, session_id, "active", Vec::new(), None);
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

#[cfg(test)]
mod tests {
    use super::{chat_control_with_lifecycle, parse_chat_control_action};
    use crate::infra::thread_events::ThreadEventsBridge;
    use proto::ChatControlAction;

    #[test]
    fn explicit_session_lifecycle_actions_forget_thread_recovery_targets() {
        assert_eq!(
            parse_chat_control_action("new_chat").unwrap(),
            (ChatControlAction::ChatControlNewChat, true)
        );
        assert_eq!(
            parse_chat_control_action("release_session").unwrap(),
            (ChatControlAction::ReleaseSession, true)
        );
        assert_eq!(
            parse_chat_control_action("refresh_memory").unwrap(),
            (ChatControlAction::ChatControlRefreshMemory, false)
        );
    }

    #[tokio::test]
    async fn lifecycle_control_fences_replacement_until_rpc_boundary_even_on_error() {
        for rpc_result in [Ok(()), Err("rpc failed".to_string())] {
            let expect_ok = rpc_result.is_ok();
            let bridge = std::sync::Arc::new(ThreadEventsBridge::new());
            bridge.activate("session-aba").await;
            let rpc_entered = std::sync::Arc::new(tokio::sync::Notify::new());
            let release_rpc = std::sync::Arc::new(tokio::sync::Notify::new());

            let lifecycle_bridge = std::sync::Arc::clone(&bridge);
            let entered = std::sync::Arc::clone(&rpc_entered);
            let release = std::sync::Arc::clone(&release_rpc);
            let lifecycle = tokio::spawn(async move {
                chat_control_with_lifecycle(
                    &lifecycle_bridge,
                    "session-aba",
                    true,
                    move || async move {
                        entered.notify_one();
                        release.notified().await;
                        rpc_result
                    },
                )
                .await
            });
            rpc_entered.notified().await;

            let replacement_bridge = std::sync::Arc::clone(&bridge);
            let mut replacement =
                tokio::spawn(async move { replacement_bridge.activate("session-aba").await });
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(20), &mut replacement,)
                    .await
                    .is_err(),
                "replacement activation must wait until the lifecycle RPC linearization boundary"
            );

            release_rpc.notify_one();
            let result = lifecycle.await.expect("lifecycle task");
            assert_eq!(result.is_ok(), expect_ok);
            assert!(
                bridge
                    .fail_activation(
                        "session-aba",
                        replacement.await.expect("replacement activation"),
                    )
                    .await
            );
        }
    }
}
