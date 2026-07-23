//! 会话压实：LLM/启发式摘要 + SessionStore 拆分。

use futures::StreamExt;
use serde::Serialize;
use uuid::Uuid;

use providers::registry::ProviderRegistry;
use providers::trait_::{ChatMessage, ProviderConfig};
use session::StoredMessage;

use crate::auxiliary_resolver::{
    primary_chat_target_for_session, resolve_auxiliary_targets, AuxiliaryTargets, ResolvedTarget,
};

const SUMMARY_PREFIX: &str = "[CONTEXT COMPACTION]";
const FALLBACK_PREFIX: &str = "[CONTEXT COMPACTION — fallback summary]";

fn keep_tail_default() -> usize {
    memory::load_compression_config(&home::default_memory_dir()).keep_tail_bubbles
}

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

fn next_compaction_target(
    targets: &AuxiliaryTargets,
    failed_index: Option<usize>,
) -> Option<(usize, &ResolvedTarget)> {
    match failed_index {
        None => Some((0, &targets.preferred)),
        Some(0) => targets.fallback.as_ref().map(|target| (1, target)),
        Some(_) => None,
    }
}

async fn summarize_with_target(
    target: &ResolvedTarget,
    transcript: &str,
) -> Result<String, String> {
    let registry = ProviderRegistry::default();
    let provider = registry.get(&target.backend_id).ok_or_else(|| {
        format!(
            "不支持的提供商后端: {}（请换用 OpenAI / DeepSeek / Google / Claude 等）",
            target.backend_id
        )
    })?;
    let config = ProviderConfig {
        api_key: target.api_key.clone(),
        base_url: if target.provider.endpoint.trim().is_empty() {
            None
        } else {
            Some(target.provider.endpoint.trim_end_matches('/').to_string())
        },
        model: target.model.clone(),
        temperature: 0.2,
        max_tokens: 2048,
        thinking_enabled: false,
        reasoning_effort: "high".to_string(),
        additional_params: serde_json::Value::Null,
        previous_interaction_id: None,
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

async fn summarize_with_targets(
    targets: AuxiliaryTargets,
    transcript: &str,
) -> Result<String, String> {
    let mut failed_index = None;
    while let Some((idx, target)) = next_compaction_target(&targets, failed_index) {
        match summarize_with_target(target, transcript).await {
            Ok(text) if !text.trim().is_empty() => return Ok(text),
            Ok(_) => {
                tracing::warn!(target = %target.model, "压实辅助模型返回空摘要，尝试下一个目标");
            }
            Err(err) => {
                tracing::warn!(target = %target.model, error = %err, "压实辅助模型失败，尝试下一个目标");
            }
        }
        failed_index = Some(idx);
    }
    Err("auxiliary compaction returned no summary".into())
}

/// 用辅助模型路由生成压实交接摘要（primary 取自会话账单/模型）。
async fn summarize_with_llm(session_id: &str, transcript: &str) -> Result<String, String> {
    let primary = primary_chat_target_for_session(session_id)?;
    let targets = resolve_auxiliary_targets(memory::AuxiliaryKind::Compaction, &primary)?;
    summarize_with_targets(targets, transcript).await
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
        .unwrap_or_else(keep_tail_default);

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

    let (summary, degraded) = match summarize_with_llm(sid, &transcript).await {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers_commands::{ProviderConfig as UiProvider, ProviderKind};

    fn ui_provider(id: &str) -> UiProvider {
        UiProvider {
            id: id.into(),
            kind: ProviderKind::Openai,
            display_name: id.into(),
            endpoint: format!("https://{id}.example"),
            model: "gpt-5.6".into(),
            enabled: true,
            fallback: Vec::new(),
            image_model: String::new(),
            video_model: String::new(),
            tts_model: String::new(),
            vision_model: String::new(),
            music_model: String::new(),
            api_mode: String::new(),
        }
    }

    fn resolved(id: &str, model: &str) -> ResolvedTarget {
        ResolvedTarget {
            provider: ui_provider(id),
            backend_id: "openai".into(),
            model: model.into(),
            api_key: format!("{id}-key"),
        }
    }

    #[test]
    fn next_compaction_target_walks_preferred_then_fallback() {
        let targets = AuxiliaryTargets {
            preferred: resolved("preferred", "gpt-mini"),
            fallback: Some(resolved("fallback", "gpt-main")),
        };

        let (idx, first) = next_compaction_target(&targets, None).expect("preferred");
        assert_eq!(idx, 0);
        assert_eq!(first.provider.id, "preferred");

        let (idx, second) = next_compaction_target(&targets, Some(0)).expect("fallback");
        assert_eq!(idx, 1);
        assert_eq!(second.provider.id, "fallback");

        assert!(next_compaction_target(&targets, Some(1)).is_none());
    }

    #[test]
    fn next_compaction_target_stops_without_fallback() {
        let targets = AuxiliaryTargets {
            preferred: resolved("preferred", "gpt-mini"),
            fallback: None,
        };

        assert!(next_compaction_target(&targets, None).is_some());
        assert!(next_compaction_target(&targets, Some(0)).is_none());
    }
}
