//! 会话压实：LLM/启发式摘要 + SessionStore 拆分。

use futures::StreamExt;
use serde::Serialize;
use uuid::Uuid;

use providers::types::stream::StreamChunk;
use providers::ProviderConfig;
use session::StoredResponseItem;

use crate::meta::auxiliary_resolver::{
    primary_model_target_for_session, resolve_auxiliary_targets, AuxiliaryTargets, ResolvedTarget,
};

const SUMMARY_PREFIX: &str = "[CONTEXT COMPACTION]";
const FALLBACK_PREFIX: &str = "[CONTEXT COMPACTION — fallback summary]";

fn keep_tail_default() -> usize {
    memory::load_compression_config(&home::default_memory_dir()).keep_tail_bubbles
}

async fn open_sessions() -> Result<session::SessionStore, String> {
    let root = home::default_memory_dir();
    memory::ensure_workspace(&root).map_err(|e| e.to_string())?;
    session::SessionStore::open_sessions_dir(&home::data_dir(&root))
        .await
        .map_err(|e| e.to_string())
}

fn fire_manual_pre_compact(
    session: Option<&agent::Session>,
    session_id: &str,
) -> Result<(), String> {
    let Some(session) = session else {
        return Ok(());
    };
    let outcome = session.fire_hook(
        hooks::PRE_COMPACT,
        hooks::HookPayload {
            trigger: Some("manual".into()),
            detail: format!("session={session_id}"),
            ..Default::default()
        },
    );
    if let hooks::HookOutcome::Block(reason) | hooks::HookOutcome::Skip(reason) = outcome {
        return Err(format!("manual compaction stopped by hook: {reason}"));
    }
    Ok(())
}

fn fire_manual_post_compact(
    session: Option<&agent::Session>,
    session_id: &str,
    new_session_id: &str,
) {
    let Some(session) = session else {
        return;
    };
    let _ = session.fire_hook(
        hooks::POST_COMPACT,
        hooks::HookPayload {
            trigger: Some("manual".into()),
            detail: format!("session={session_id} new_session={new_session_id}"),
            ..Default::default()
        },
    );
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
fn heuristic_summary(messages: &[StoredResponseItem], max_chars: usize) -> String {
    let mut parts = Vec::new();
    for m in messages.iter().rev() {
        if !matches!(m.role(), Some("user" | "assistant")) {
            continue;
        }
        let text = m.text();
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        parts.push(format!(
            "{}: {}",
            m.role().unwrap_or("unknown"),
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
        api_mode: String::new(),
    };
    let system = "You compress a chat transcript into a compact handoff note. \
Cover: goals, constraints, done, in-progress, key paths/decisions, next steps. \
Reply in the same language as the transcript. No preamble.";
    let user = format!("Transcript:\n\n{transcript}");
    let mut stream =
        providers::dispatch::agent_responses_prompt(&target.backend_id, system, user, &config)
            .await
            .map_err(|e| format!("压实调用模型失败: {e}"))?;
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| format!("压实流式读取失败: {e}"))?;
        if let StreamChunk::Text(token) = chunk {
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
    let primary = primary_model_target_for_session(session_id).await?;
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
    let memory_dir = home::default_memory_dir();
    let live_session = agent::exec::dispatch::active_root_session_for_hooks(&memory_dir, sid)
        .map_err(|error| error.to_string())?;

    // 先读出元数据/消息并 drop store，再 await LLM。
    let (messages, expected_last_message_id, transcript) = {
        let store = open_sessions().await?;
        let meta = store
            .get_session(sid)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "会话不存在".to_string())?;
        if meta.ended_at.is_some() {
            return Err("会话已结束，无法压实".into());
        }

        let messages = store
            .get_response_items(sid)
            .await
            .map_err(|e| e.to_string())?;
        let bubble_count = messages
            .iter()
            .filter(|m| matches!(m.role(), Some("user" | "assistant")))
            .count();
        if bubble_count < 2 {
            return Err("消息过少，无需压实".into());
        }

        let transcript: String = messages
            .iter()
            .filter(|m| matches!(m.role(), Some("user" | "assistant")))
            .map(|m| {
                format!(
                    "{}: {}",
                    m.role().unwrap_or("unknown"),
                    m.text().chars().take(2000).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let transcript: String = transcript.chars().take(60_000).collect();
        let expected_last_message_id = messages.last().map(|message| message.id);
        (messages, expected_last_message_id, transcript)
    };

    fire_manual_pre_compact(live_session.as_deref(), sid)?;

    let (summary, degraded) = match summarize_with_llm(sid, &transcript).await {
        Ok(s) => (s, false),
        Err(err) => {
            tracing::warn!(error = %err, "压实 LLM 摘要失败，回退启发式");
            (heuristic_summary(&messages, 6_000), true)
        }
    };

    let new_id = Uuid::new_v4().to_string();
    {
        let store = open_sessions().await?;
        store
            .compact_and_split_if_unchanged(sid, &new_id, &summary, keep, expected_last_message_id)
            .await
            .map_err(|e| e.to_string())?;
    }

    fire_manual_post_compact(live_session.as_deref(), sid, &new_id);

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
    use crate::commands::providers::core::{ProviderConfig as UiProvider, ProviderKind};
    use std::sync::{Arc, Mutex};

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
            embedding_model: String::new(),
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

    #[tokio::test]
    async fn manual_compaction_fires_canonical_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let session = agent::Session::new(agent::Config::with_defaults(dir.path().to_path_buf()))
            .await
            .unwrap();
        let observed = Arc::new(Mutex::new(Vec::new()));
        for event in [hooks::PRE_COMPACT, hooks::POST_COMPACT] {
            let captured = Arc::clone(&observed);
            session.hook_bus().register(event, move |payload| {
                captured
                    .lock()
                    .unwrap()
                    .push((payload.hook_event_name.clone(), payload.trigger.clone()));
                hooks::HookOutcome::Continue
            });
        }

        fire_manual_pre_compact(Some(&session), "session-1").unwrap();
        fire_manual_post_compact(Some(&session), "session-1", "session-2");

        assert_eq!(
            *observed.lock().unwrap(),
            vec![
                (hooks::PRE_COMPACT.into(), Some("manual".into())),
                (hooks::POST_COMPACT.into(), Some("manual".into())),
            ]
        );
    }

    #[tokio::test]
    async fn manual_pre_compact_can_stop_before_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let session = agent::Session::new(agent::Config::with_defaults(dir.path().to_path_buf()))
            .await
            .unwrap();
        session.hook_bus().register(hooks::PRE_COMPACT, |payload| {
            assert_eq!(payload.trigger.as_deref(), Some("manual"));
            hooks::HookOutcome::Block("keep current transcript".into())
        });

        let error = fire_manual_pre_compact(Some(&session), "session-1").unwrap_err();
        assert!(error.contains("keep current transcript"));
    }
}
