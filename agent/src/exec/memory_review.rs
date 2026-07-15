//! 回合后记忆 background review：调用辅助模型并应用建议。
//!
//! 由 backend 在 Chat 流 `Done` 后 fire-and-forget；完成后可选通过
//! [`MemoryReviewNotify`] 通知调用方（再由 backend 发布到 SessionEventHub）。
//! `auxiliary.background_review_enabled` 为 false 时直接跳过。

use std::path::PathBuf;

use futures::StreamExt;
use memory::{
    apply_review_suggestions, build_review_digest, load_auxiliary_config, parse_review_llm_output,
    resolve_auxiliary, AuxiliaryKind, MemoryManager, REVIEW_SYSTEM_PROMPT,
};
use providers::registry::ProviderRegistry;
use providers::trait_::{ChatMessage, ProviderConfig};
use tracing::{info, warn};

use crate::loop_::AgentLoop;

/// 一次 background review 所需的快照凭据与对话。
#[derive(Debug, Clone)]
pub struct BackgroundReviewJob {
    pub memory_dir: PathBuf,
    pub agent_id: String,
    /// `(role, content)` 升序。
    pub messages: Vec<(String, String)>,
    pub session_provider: String,
    pub session_model: String,
    pub api_key: String,
    pub base_url: String,
}

/// review 写盘后的轻量通知（`op` + `content`）。
#[derive(Debug, Clone)]
pub struct MemoryReviewNotify {
    pub op: String,
    pub content: String,
}

/// 从当前 [`AgentLoop`] 快照构造 review job（不含 system 消息）。
pub fn job_from_agent(agent: &AgentLoop) -> BackgroundReviewJob {
    let messages = agent
        .session_messages
        .iter()
        .filter_map(|m| {
            let role = match m.role {
                common::message::Role::User => "user",
                common::message::Role::Assistant => "assistant",
                common::message::Role::Tool => "tool",
                common::message::Role::System => return None,
            };
            let content = m.content_str().trim();
            if content.is_empty() {
                return None;
            }
            // 截断单条，避免 review 上下文爆炸
            let clipped: String = content.chars().take(4_000).collect();
            Some((role.to_string(), clipped))
        })
        .collect();
    BackgroundReviewJob {
        memory_dir: agent.memory_dir().to_path_buf(),
        agent_id: agent.agent_id().to_string(),
        messages,
        session_provider: agent.chat_provider().to_string(),
        session_model: agent.chat_model().to_string(),
        api_key: agent.chat_api_key().to_string(),
        base_url: agent.chat_base_url().to_string(),
    }
}

/// 生成面向 UI 的通知文案；无有效写入时返回 `None`。
pub fn review_notify_from_applied(applied: &[String]) -> Option<MemoryReviewNotify> {
    if applied.is_empty() {
        return None;
    }
    let preview = applied
        .iter()
        .take(2)
        .map(|s| {
            let t = s.trim();
            if t.chars().count() > 80 {
                format!("{}…", t.chars().take(80).collect::<String>())
            } else {
                t.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("；");
    Some(MemoryReviewNotify {
        op: "background_review".into(),
        content: format!("记忆已更新（{}）· {preview}", applied.len()),
    })
}

/// 若配置开启则异步跑 review；完成后可选向 `notify` 推一条摘要。
pub fn spawn_background_review_after_turn(
    agent: &AgentLoop,
    notify: Option<tokio::sync::mpsc::UnboundedSender<MemoryReviewNotify>>,
) {
    let job = job_from_agent(agent);
    tokio::spawn(async move {
        match maybe_run_background_review(job).await {
            Ok(applied) => {
                if let (Some(tx), Some(n)) = (notify, review_notify_from_applied(&applied)) {
                    let _ = tx.send(n);
                }
            }
            Err(e) => warn!(error = %e, "memory background review failed"),
        }
    });
}

/// 若配置开启则跑 review；关闭则 Ok(空)。
///
/// 返回值：实际应用的写入摘要行（含 pending 入队提示）。
pub async fn maybe_run_background_review(job: BackgroundReviewJob) -> anyhow::Result<Vec<String>> {
    let aux = load_auxiliary_config(&job.memory_dir);
    if !aux.background_review_enabled {
        return Ok(vec![]);
    }
    if job.api_key.is_empty() && job.session_provider != "ollama" {
        warn!("background review skipped: empty api_key");
        return Ok(vec![]);
    }

    let (provider, model) = resolve_auxiliary(
        AuxiliaryKind::BackgroundReview,
        &aux,
        &job.session_provider,
        &job.session_model,
    );
    if model.trim().is_empty() {
        warn!("background review skipped: empty model");
        return Ok(vec![]);
    }

    let digest = build_review_digest(&job.messages, 6);
    if digest.trim().is_empty() {
        return Ok(vec![]);
    }

    info!(
        agent = %job.agent_id,
        provider = %provider,
        model = %model,
        "running memory background review"
    );

    let raw = complete_review_chat(
        &provider,
        &model,
        &job.api_key,
        &job.base_url,
        REVIEW_SYSTEM_PROMPT,
        &digest,
    )
    .await?;

    let output = parse_review_llm_output(&raw)?;
    if output.suggestions.is_empty()
        && output
            .daily_note
            .as_ref()
            .map_or(true, |s| s.trim().is_empty())
    {
        return Ok(vec![]);
    }

    let mut mgr = MemoryManager::for_agent(job.memory_dir, &job.agent_id)?;
    let applied = apply_review_suggestions(&mut mgr, &output)?;
    info!(
        agent = %job.agent_id,
        n = applied.len(),
        "memory background review applied"
    );
    Ok(applied)
}

async fn complete_review_chat(
    backend_id: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
    system: &str,
    user: &str,
) -> anyhow::Result<String> {
    let registry = ProviderRegistry::default();
    let provider = registry
        .get(backend_id)
        .ok_or_else(|| anyhow::anyhow!("不支持的 review 提供商后端: {backend_id}"))?;
    let config = ProviderConfig {
        api_key: api_key.to_string(),
        base_url: if base_url.trim().is_empty() {
            None
        } else {
            Some(base_url.trim_end_matches('/').to_string())
        },
        model: model.to_string(),
        temperature: 0.2,
        max_tokens: 2048,
        thinking_enabled: false,
        reasoning_effort: "high".to_string(),
        additional_params: serde_json::Value::Null,
    };
    let messages = vec![
        ChatMessage::text("system", system),
        ChatMessage::text("user", user),
    ];
    let mut stream = provider.chat_stream(messages, vec![], &config).await?;
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item?;
        if let Some(token) = chunk.token {
            out.push_str(&token);
        }
    }
    if out.trim().is_empty() {
        anyhow::bail!("background review 模型未返回内容");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_none_when_empty() {
        assert!(review_notify_from_applied(&[]).is_none());
    }

    #[test]
    fn notify_summarizes_count() {
        let n = review_notify_from_applied(&[
            "added memory entry".into(),
            "added user profile".into(),
        ])
        .unwrap();
        assert_eq!(n.op, "background_review");
        assert!(n.content.contains("记忆已更新（2）"));
        assert!(n.content.contains("added memory"));
    }
}
