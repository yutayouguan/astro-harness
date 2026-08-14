//! Hard 阶段 mid-run 中间轮次摘要（Hermes 阶段 3）：不拆 session。
//!
//! 在 prune / head-tail 之后若窗口占用仍高，用 `AuxiliaryTask::Compaction`
//! 生成结构化交接摘要，注入 [`AgentLoop`] 供后续 Provider 历史折叠。

use futures::StreamExt;
use providers::types::message::Message as ProviderMessage;
use providers::types::stream::StreamChunk;
use providers::ProviderConfig;
use tracing::{info, warn};

use crate::compression::{estimate_messages_tokens, ToolCompressionManager};
use crate::runtime::AgentLoop;
use types::message::{Message, Role};

pub const MID_RUN_SUMMARY_MARK: &str = "[astro:mid-run-summary]";
/// Hard 阶段占比默认；运行时优先读 `compression.mid_run_summary_ratio`。
pub const MID_RUN_SUMMARY_RATIO: f32 = 0.80;
/// 默认头保护条数；运行时优先读 `compression.protect_first_messages`。
pub const PROTECT_FIRST_MESSAGES: usize = 4;

/// 是否应尝试 mid-run 摘要。
pub fn should_attempt(agent: &AgentLoop) -> bool {
    if agent.mid_run_summary_done() {
        return false;
    }
    let cfg = agent.compression_config();
    if !cfg.enabled {
        return false;
    }
    let protect_first = cfg.protect_first_messages.max(1);
    if agent.session_messages.len() < protect_first + agent.config_protect_last_n() {
        return false;
    }
    let mgr = ToolCompressionManager::from_config(&cfg).with_context_window(agent.context_window());
    mgr.occupancy_ratio(&agent.session_messages) >= cfg.mid_run_summary_ratio
}

/// 折叠 Provider 历史：头 + 摘要 + 尾（不改 DB 原文）。
pub fn collapse_history_with_handoff(
    messages: &[Message],
    handoff: &str,
    protect_first: usize,
    protect_last: usize,
) -> Vec<Message> {
    let n = messages.len();
    let first = protect_first.min(n);
    let last = protect_last.min(n.saturating_sub(first));
    if n <= first + last {
        return messages.to_vec();
    }
    let mut out = Vec::with_capacity(first + 1 + last);
    out.extend_from_slice(&messages[..first]);
    out.push(Message::user(&format!(
        "{MID_RUN_SUMMARY_MARK}\n{}",
        handoff.trim()
    )));
    out.extend_from_slice(&messages[n - last..]);
    out
}

fn build_transcript(messages: &[Message], protect_first: usize, protect_last: usize) -> String {
    let n = messages.len();
    let first = protect_first.min(n);
    let last = protect_last.min(n.saturating_sub(first));
    if n <= first + last {
        return String::new();
    }
    let mut parts = Vec::new();
    for m in &messages[first..n - last] {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
        };
        let body = m
            .compressed_content
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| m.content_str());
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

async fn complete_summary_chat(
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
    let messages = vec![
        ProviderMessage::system("You write compact structured handoff notes for coding agents."),
        ProviderMessage::user_text(prompt),
    ];
    let mut stream =
        providers::dispatch::chat_stream(&target.backend_id, messages, vec![], &config).await?;
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

/// 尝试 mid-run 摘要；成功则写入 AgentLoop handoff，返回 true。
pub async fn maybe_apply_mid_run_summary(agent: &mut AgentLoop) -> anyhow::Result<bool> {
    if !should_attempt(agent) {
        return Ok(false);
    }
    let protect_first = agent.config_protect_first_n();
    let protect_last = agent.config_protect_last_n();
    let transcript = build_transcript(&agent.session_messages, protect_first, protect_last);
    if transcript.chars().count() < 400 {
        agent.mark_mid_run_summary_skipped();
        return Ok(false);
    }

    let targets = agent.auxiliary_targets(types::AuxiliaryTask::Compaction);
    if targets.is_empty() {
        warn!("mid-run summary skipped: no compaction targets");
        agent.mark_mid_run_summary_skipped();
        return Ok(false);
    }

    let prompt = summary_prompt(&transcript);
    let mut last_err = None;
    let mut summary = None;
    for target in targets.iter().take(2) {
        match complete_summary_chat(target, &prompt).await {
            Ok(text) => {
                summary = Some(text);
                break;
            }
            Err(e) => {
                warn!(
                    backend = %target.backend_id,
                    model = %target.model,
                    error = %e,
                    "mid-run summary target failed"
                );
                last_err = Some(e);
            }
        }
    }
    let Some(text) = summary else {
        agent.mark_mid_run_summary_skipped();
        if let Some(e) = last_err {
            return Err(e);
        }
        return Ok(false);
    };

    let before = estimate_messages_tokens(&agent.session_messages);
    agent.set_mid_run_handoff(text.clone());
    let collapsed =
        collapse_history_with_handoff(&agent.session_messages, &text, protect_first, protect_last);
    let after = estimate_messages_tokens(&collapsed);
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
        let msgs: Vec<_> = (0..10).map(|i| Message::user(&format!("m{i}"))).collect();
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
        let msgs = vec![Message::user("a"), Message::user("b")];
        let out = collapse_history_with_handoff(&msgs, "x", 4, 20);
        assert_eq!(out.len(), 2);
    }
}
