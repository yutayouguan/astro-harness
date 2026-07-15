//! 会话压实：LLM/启发式摘要 + SessionStore 拆分。

use futures::StreamExt;
use serde::Serialize;
use uuid::Uuid;

use session::StoredMessage;
use providers::registry::ProviderRegistry;
use providers::trait_::{ChatMessage, ProviderConfig};

use crate::providers_commands::{self, resolve_api_key, ProviderConfig as UiProvider};

const KEEP_TAIL_DEFAULT: usize = 3;
const SUMMARY_PREFIX: &str = "[CONTEXT COMPACTION]";
const FALLBACK_PREFIX: &str = "[CONTEXT COMPACTION — fallback summary]";

fn open_sessions() -> Result<session::SessionStore, String> {
    let root = home::default_memory_dir();
    memory::ensure_workspace(&root).map_err(|e| e.to_string())?;
    session::SessionStore::open_sessions_dir(&root.join("sessions")).map_err(|e| e.to_string())
}

/// 压实结果：新会话 id、摘要预览、是否降级为启发式。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactChatResultDto {
    pub new_session_id: String,
    pub summary_preview: String,
    pub degraded: bool,
}

/// 取 UI 当前激活（或列表首个）供应商配置。
fn active_ui_provider() -> Result<UiProvider, String> {
    let state = providers_commands::get_providers_state()?;
    let id = state
        .active_provider_id
        .or_else(|| state.providers.first().map(|p| p.id.clone()))
        .ok_or_else(|| "请先在「模型提供商」中配置并启用至少一个提供商".to_string())?;
    providers_commands::find_provider(&id)
}

/// 无模型时的启发式摘要（最近若干条 user/assistant 截断拼接）。
fn heuristic_summary(messages: &[StoredMessage], max_chars: usize) -> String {
    let mut parts = Vec::new();
    for m in messages.iter().rev() {
        if m.role != "user" && m.role != "assistant" {
            continue;
        }
        let text = m.content.as_deref().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        parts.push(format!(
            "{}: {}",
            m.role,
            text.chars().take(400).collect::<String>()
        ));
        if parts.len() >= 8 {
            break;
        }
    }
    parts.reverse();
    let body = parts.join("\n");
    let clipped: String = body.chars().take(max_chars).collect();
    format!("{FALLBACK_PREFIX}\n{clipped}")
}

/// 用当前激活提供商生成压实交接摘要。
async fn summarize_with_llm(transcript: &str) -> Result<String, String> {
    let ui = active_ui_provider()?;
    let (has, _src, _env, key) = resolve_api_key(&ui);
    if ui.kind.requires_api_key() && !has {
        return Err(format!(
            "未配置 API Key。请在「模型提供商」中为 {} 保存密钥。",
            ui.display_name
        ));
    }
    if ui.model.trim().is_empty() {
        return Err("激活提供商未配置模型".into());
    }
    let api_key = key.unwrap_or_default();
    let backend_id = ui.kind.backend_id();
    let base_url = ui.endpoint.clone();

    let registry = ProviderRegistry::default();
    let provider = registry.get(backend_id).ok_or_else(|| {
        format!("不支持的提供商后端: {backend_id}（请换用 OpenAI / DeepSeek / Google / Claude 等）")
    })?;
    let config = ProviderConfig {
        api_key,
        base_url: if base_url.trim().is_empty() {
            None
        } else {
            Some(base_url.trim_end_matches('/').to_string())
        },
        model: ui.model.clone(),
        temperature: 0.2,
        max_tokens: 2048,
        thinking_enabled: false,
        reasoning_effort: "high".to_string(),
        additional_params: serde_json::Value::Null,
    };
    let system = "You compress a chat transcript into a compact handoff note. \
Cover: goals, constraints, done, in-progress, key paths/decisions, next steps. \
Reply in the same language as the transcript. No preamble.";
    let user = format!("Transcript:\n\n{transcript}");
    let messages = vec![
        ChatMessage::text("system", system),
        ChatMessage::text("user", user),
    ];
    let mut stream = provider
        .chat_stream(messages, vec![], &config)
        .await
        .map_err(|e| format!("压实调用模型失败: {e}"))?;
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| format!("压实流式读取失败: {e}"))?;
        if let Some(token) = chunk.token {
            out.push_str(&token);
        }
    }
    let trimmed = out.trim().to_string();
    if trimmed.is_empty() {
        return Err("模型未返回任何内容".into());
    }
    Ok(format!("{SUMMARY_PREFIX}\n{trimmed}"))
}

/// Tauri 命令：压实当前会话（摘要 + 拆出新会话）。
#[tauri::command]
pub async fn compact_chat_session(
    session_id: String,
    keep_tail_bubbles: Option<i32>,
    _focus: Option<String>,
) -> Result<CompactChatResultDto, String> {
    let sid = session_id.trim();
    if sid.is_empty() {
        return Err("session_id 不能为空".into());
    }
    let keep = keep_tail_bubbles
        .map(|k| k.max(0) as usize)
        .unwrap_or(KEEP_TAIL_DEFAULT);

    // SessionStore（rusqlite）非 Send：先读出元数据/消息并 drop，再 await LLM。
    let (messages, transcript) = {
        let store = open_sessions()?;
        let meta = store
            .get_session(sid)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "会话不存在".to_string())?;
        if meta.ended_at.is_some() {
            return Err("会话已结束，无法压实".into());
        }

        let messages = store.get_messages(sid).map_err(|e| e.to_string())?;
        let bubble_count = messages
            .iter()
            .filter(|m| m.role == "user" || m.role == "assistant")
            .count();
        if bubble_count < 2 {
            return Err("消息过少，无需压实".into());
        }

        let transcript: String = messages
            .iter()
            .filter(|m| m.role == "user" || m.role == "assistant")
            .map(|m| {
                format!(
                    "{}: {}",
                    m.role,
                    m.content
                        .as_deref()
                        .unwrap_or("")
                        .chars()
                        .take(2000)
                        .collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let transcript: String = transcript.chars().take(60_000).collect();
        (messages, transcript)
    };

    let (summary, degraded) = match summarize_with_llm(&transcript).await {
        Ok(s) => (s, false),
        Err(err) => {
            tracing::warn!(error = %err, "压实 LLM 摘要失败，回退启发式");
            (heuristic_summary(&messages, 6_000), true)
        }
    };

    let new_id = Uuid::new_v4().to_string();
    {
        let store = open_sessions()?;
        store
            .compact_and_split(sid, &new_id, &summary, keep)
            .map_err(|e| e.to_string())?;
    }

    let preview: String = summary.chars().take(160).collect();
    Ok(CompactChatResultDto {
        new_session_id: new_id,
        summary_preview: preview,
        degraded,
    })
}
