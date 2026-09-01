//! Hard 阶段 mid-run 中间轮次摘要（Hermes 阶段 3）：不拆 session。
//!
//! 在 prune / head-tail 之后若窗口占用仍高，用 `AuxiliaryTask::Compaction`
//! 生成结构化交接摘要，注入 [`AgentLoop`] 供后续 Provider 历史折叠。

use futures::StreamExt;
use providers::types::stream::StreamChunk;
use providers::ProviderConfig;
use tracing::{info, warn};

use crate::compression::{estimate_response_items_tokens, ToolCompressionManager};
use crate::runtime::AgentLoop;
use agent_protocol::ResponseItem;

pub const MID_RUN_SUMMARY_MARK: &str = "[astro:mid-run-summary]";
/// Hard 阶段占比默认；运行时优先读 `compression.mid_run_summary_ratio`。
pub const MID_RUN_SUMMARY_RATIO: f32 = 0.80;
/// 默认头保护条数；运行时优先读 `compression.protect_first_messages`。
pub const PROTECT_FIRST_MESSAGES: usize = 4;

/// 是否应尝试 mid-run 摘要。
pub async fn should_attempt(agent: &AgentLoop) -> bool {
    if agent.mid_run_summary_done().await {
        return false;
    }
    let cfg = agent.compression_config();
    if !cfg.enabled {
        return false;
    }
    let history = agent.clone_history().await;
    let protect_first = cfg.protect_first_messages.max(1);
    if history.len() < protect_first + agent.config_protect_last_n() {
        return false;
    }
    let mgr = ToolCompressionManager::from_config(&cfg).with_context_window(agent.context_window());
    mgr.occupancy_ratio(&history) >= cfg.mid_run_summary_ratio
}

/// 折叠 Provider 历史：头 + 摘要 + 尾（不改 DB 原文）。
pub fn collapse_history_with_handoff(
    items: &[ResponseItem],
    handoff: &str,
    protect_first: usize,
    protect_last: usize,
) -> Vec<ResponseItem> {
    let n = items.len();
    let first = protect_first.min(n);
    let last = protect_last.min(n.saturating_sub(first));
    if n <= first + last {
        return items.to_vec();
    }
    let mut out = Vec::with_capacity(first + 1 + last);
    out.extend_from_slice(&items[..first]);
    out.push(ResponseItem::user_text(format!(
        "{MID_RUN_SUMMARY_MARK}\n{}",
        handoff.trim()
    )));
    out.extend_from_slice(&items[n - last..]);
    out
}

fn build_transcript(items: &[ResponseItem], protect_first: usize, protect_last: usize) -> String {
    let n = items.len();
    let first = protect_first.min(n);
    let last = protect_last.min(n.saturating_sub(first));
    if n <= first + last {
        return String::new();
    }
    let mut parts = Vec::new();
    for item in &items[first..n - last] {
        let role = item.role().unwrap_or_else(|| {
            if item.is_tool_output() {
                "tool"
            } else {
                "assistant"
            }
        });
        let body = item.provider_view_text();
        let clipped: String = body.chars().take(2_000).collect();
        if clipped.trim().is_empty() {
            continue;
        }
        parts.push(format!("{role}: {clipped}"));
    }
    parts.join("\n\n")
}

fn summary_prompt(transcript: &str) -> String {
    format!(
        "You compress the MIDDLE of an ongoing agent session into a handoff note.\n\
         Keep: Goal, Constraints, Progress (Done / In Progress / Blocked), Key Decisions,\n\
         Relevant Files/paths, Critical values/errors, Next Steps.\n\
         Reply in the same language as the transcript. No preamble.\n\n\
         Transcript:\n\n{transcript}"
    )
}

async fn complete_summary_response(
    target: &types::ChatTarget,
    prompt: &str,
) -> anyhow::Result<String> {
    let config = ProviderConfig {
        api_key: target.api_key.clone(),
        base_url: if target.base_url.trim().is_empty() {
            None
        } else {
            Some(target.base_url.trim_end_matches('/').to_string())
        },
        model: target.model.clone(),
        temperature: 0.2,
        max_tokens: 2_048,
        thinking_enabled: false,
        reasoning_effort: "high".to_string(),
        additional_params: serde_json::Value::Null,
        ..ProviderConfig::default()
    };
    let mut stream = providers::dispatch::agent_responses_prompt(
        &target.backend_id,
        "You write compact structured handoff notes for coding agents.",
        prompt,
        &config,
    )
    .await?;
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item?;
        if let StreamChunk::Text(token) = chunk {
            out.push_str(&token);
        }
    }
    let trimmed = out.trim().to_string();
    if trimmed.is_empty() {
        anyhow::bail!("empty mid-run summary");
    }
    Ok(trimmed)
}

async fn complete_with_targets(
    targets: &[types::ChatTarget],
    prompt: &str,
) -> anyhow::Result<String> {
    let mut last_error = None;
    for target in targets.iter().take(2) {
        match complete_summary_response(target, prompt).await {
            Ok(text) => return Ok(text),
            Err(error) => {
                warn!(
                    backend = %target.backend_id,
                    model = %target.model,
                    %error,
                    "compaction summary target failed"
                );
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no compaction targets")))
}

/// Generate a durable full-session handoff used by the explicit `Compact` task.
pub(crate) async fn generate_manual_summary(agent: &AgentLoop) -> anyhow::Result<String> {
    let history = agent.clone_history().await;
    let transcript = build_transcript(&history, 0, 0);
    anyhow::ensure!(
        !transcript.trim().is_empty(),
        "conversation history is empty"
    );
    let targets = agent.auxiliary_targets(types::AuxiliaryTask::Compaction);
    complete_with_targets(&targets, &summary_prompt(&transcript)).await
}

/// 尝试 mid-run 摘要；成功则写入 AgentLoop handoff，返回 true。
pub async fn maybe_apply_mid_run_summary(agent: &AgentLoop) -> anyhow::Result<bool> {
    if !should_attempt(agent).await {
        return Ok(false);
    }
    let protect_first = agent.config_protect_first_n();
    let protect_last = agent.config_protect_last_n();
    let history = agent.clone_history().await;
    let transcript = build_transcript(&history, protect_first, protect_last);
    if transcript.chars().count() < 400 {
        agent.mark_mid_run_summary_skipped().await;
        return Ok(false);
    }

    let targets = agent.auxiliary_targets(types::AuxiliaryTask::Compaction);
    if targets.is_empty() {
        warn!("mid-run summary skipped: no compaction targets");
        agent.mark_mid_run_summary_skipped().await;
        return Ok(false);
    }

    let prompt = summary_prompt(&transcript);
    let text = match complete_with_targets(&targets, &prompt).await {
        Ok(text) => text,
        Err(error) => {
            agent.mark_mid_run_summary_skipped().await;
            return Err(error);
        }
    };

    let before = estimate_response_items_tokens(&history);
    agent.set_mid_run_handoff(text.clone()).await;
    agent.rebase_prompt_context_after_compaction(&text).await;
    let collapsed = collapse_history_with_handoff(&history, &text, protect_first, protect_last);
    let after = estimate_response_items_tokens(&collapsed);
    info!(
        session = %agent.session_id(),
        before_tokens = before,
        after_tokens = after,
        "mid-run summary applied (provider view collapsed; session not split)"
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_keeps_head_and_tail() {
        let msgs: Vec<_> = (0..10)
            .map(|i| ResponseItem::user_text(format!("m{i}")))
            .collect();
        let out = collapse_history_with_handoff(&msgs, "HANDOFF", 2, 3);
        assert_eq!(out.len(), 2 + 1 + 3);
        assert_eq!(out[0].content_str(), "m0");
        assert!(out[2].content_str().contains(MID_RUN_SUMMARY_MARK));
        assert!(out[2].content_str().contains("HANDOFF"));
        assert_eq!(out[3].content_str(), "m7");
        assert_eq!(out.last().unwrap().content_str(), "m9");
    }

    #[test]
    fn collapse_noop_when_short() {
        let msgs = vec![ResponseItem::user_text("a"), ResponseItem::user_text("b")];
        let out = collapse_history_with_handoff(&msgs, "x", 4, 20);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn transcript_uses_user_content_but_tool_compressed_view() {
        let mut user = ResponseItem::user_text("follow the real instruction");
        *user.metadata_mut().unwrap() = Some(serde_json::json!({
            "astro_memory_marker": "agent-mailbox-through:42"
        }));
        let mut tool = ResponseItem::FunctionCallOutput {
            id: None,
            call_id: None,
            name: Some("test".into()),
            namespace: None,
            output: agent_protocol::FunctionCallOutputPayload::from_text(
                "very long original tool result".into(),
            ),
            internal_chat_message_metadata_passthrough: None,
        };
        *tool.metadata_mut().unwrap() = Some(serde_json::json!({
            "astro_compressed_output": "short tool stub"
        }));

        let transcript = build_transcript(&[user, tool], 0, 0);

        assert!(transcript.contains("user: follow the real instruction"));
        assert!(!transcript.contains("agent-mailbox-through:42"));
        assert!(transcript.contains("tool: short tool stub"));
        assert!(!transcript.contains("very long original tool result"));
    }
}
