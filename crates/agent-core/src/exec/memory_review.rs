//! 回合后记忆 background review：调用辅助模型并应用建议。
//!
//! 由 backend 在成功的 Thread terminal 后 fire-and-forget；完成后可选通过
//! [`MemoryReviewNotify`] 通知调用方（再由 backend 提交 durable Thread Extension）。
//! `auxiliary.background_review_enabled` 为 false 时直接跳过。
//!
//! 目标链来自 ChatRequest 注入的 `AuxiliaryTask::BackgroundReview`
//! （preferred + 可选 fallback）；每项携带完整 endpoint/key/model，不再把
//! session 的 api_key 硬套到显式 backend 上。

use std::path::PathBuf;

use futures::StreamExt;
use memory::{
    apply_review_suggestions, build_review_digest, load_auxiliary_config, parse_review_llm_output,
    MemoryManager, REVIEW_SYSTEM_PROMPT,
};
use providers::types::stream::StreamChunk;
use providers::ProviderConfig;
use tracing::{info, warn};

use crate::runtime::AgentLoop;

/// 一次 background review 所需的快照凭据与对话。
#[derive(Debug, Clone)]
pub struct BackgroundReviewJob {
    pub memory_dir: PathBuf,
    pub agent_id: String,
    /// `(role, content)` 升序。
    pub messages: Vec<(String, String)>,
    /// preferred + 可选 fallback；每项是完整 ChatTarget。
    pub targets: Vec<types::ChatTarget>,
}

/// review 写盘后的轻量通知（`op` + `content`）。
#[derive(Debug, Clone)]
pub struct MemoryReviewNotify {
    pub op: String,
    pub content: String,
}

/// 从当前 [`AgentLoop`] 快照构造 review job（不含 system 消息）。
pub async fn job_from_agent(agent: &AgentLoop) -> BackgroundReviewJob {
    let history = agent.clone_history().await;
    let messages = history
        .iter()
        .filter_map(|m| {
            let role = match m.role {
                types::message::Role::User => "user",
                types::message::Role::Assistant => "assistant",
                types::message::Role::Tool => "tool",
                types::message::Role::System => return None,
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
        targets: agent.auxiliary_targets(types::AuxiliaryTask::BackgroundReview),
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
pub async fn spawn_background_review_after_turn(
    agent: &AgentLoop,
    notify: Option<tokio::sync::mpsc::UnboundedSender<MemoryReviewNotify>>,
) {
    let job = job_from_agent(agent).await;
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

/// 按 preferred→fallback 完成；返回首个非空响应。全部失败返回 Err。
pub async fn complete_review_with_targets<F, Fut>(
    targets: &[types::ChatTarget],
    mut complete: F,
) -> Result<String, String>
where
    F: FnMut(&types::ChatTarget) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let mut last_err = "no review targets".to_string();
    for target in targets.iter().take(2) {
        match complete(target).await {
            Ok(text) if !text.trim().is_empty() => return Ok(text),
            Ok(_) => {
                last_err = format!(
                    "empty background-review response from {}",
                    target.backend_id
                );
            }
            Err(err) => {
                warn!(
                    backend = %target.backend_id,
                    model = %target.model,
                    error = %err,
                    "background review target failed; trying next"
                );
                last_err = err;
            }
        }
    }
    Err(last_err)
}

/// 若配置开启则跑 review；关闭则 Ok(空)。
///
/// 返回值：实际应用的写入摘要行（含 pending 入队提示）。
pub async fn maybe_run_background_review(job: BackgroundReviewJob) -> anyhow::Result<Vec<String>> {
    let aux = load_auxiliary_config(&job.memory_dir);
    if !aux.background_review_enabled {
        return Ok(vec![]);
    }
    if job.targets.is_empty() {
        warn!("background review skipped: empty targets");
        return Ok(vec![]);
    }

    let digest = build_review_digest(&job.messages, 6);
    if digest.trim().is_empty() {
        return Ok(vec![]);
    }

    let preferred = &job.targets[0];
    info!(
        agent = %job.agent_id,
        provider = %preferred.backend_id,
        model = %preferred.model,
        targets = job.targets.len(),
        "running memory background review"
    );

    let raw = match complete_review_with_targets(&job.targets, |target| {
        let system = REVIEW_SYSTEM_PROMPT.to_string();
        let digest = digest.clone();
        let target = target.clone();
        async move {
            complete_review_chat(
                &target.backend_id,
                &target.model,
                &target.api_key,
                &target.base_url,
                &system,
                &digest,
            )
            .await
            .map_err(|e| e.to_string())
        }
    })
    .await
    {
        Ok(text) => text,
        Err(err) => {
            // 两次失败只记日志，不抛到 Done 路径；此处仍以 Result 上抛给 spawn 的 warn。
            return Err(anyhow::anyhow!(err));
        }
    };

    let output = parse_review_llm_output(&raw)?;
    if output.suggestions.is_empty()
        && output
            .daily_note
            .as_ref()
            .is_none_or(|s| s.trim().is_empty())
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
        previous_interaction_id: None,
        api_mode: String::new(),
    };
    let mut stream =
        providers::dispatch::agent_responses_prompt(backend_id, system, user, &config).await?;
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item?;
        if let StreamChunk::Text(token) = chunk {
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

    fn target(id: &str, backend: &str, model: &str, key: &str) -> types::ChatTarget {
        types::ChatTarget {
            provider_id: id.into(),
            backend_id: backend.into(),
            model: model.into(),
            api_key: key.into(),
            base_url: format!("https://{id}.example"),
            api_mode: String::new(),
        }
    }

    #[test]
    fn notify_none_when_empty() {
        assert!(review_notify_from_applied(&[]).is_none());
    }

    #[test]
    fn notify_summarizes_count() {
        let n =
            review_notify_from_applied(&["added memory entry".into(), "added user profile".into()])
                .unwrap();
        assert_eq!(n.op, "background_review");
        assert!(n.content.contains("记忆已更新（2）"));
        assert!(n.content.contains("added memory"));
    }

    #[tokio::test]
    async fn explicit_target_uses_its_own_credentials() {
        let targets = vec![
            target("cheap", "deepseek", "mini", "cheap-key"),
            target("main", "openai", "gpt", "main-key"),
        ];
        let mut seen = Vec::new();
        let text = complete_review_with_targets(&targets, |t| {
            seen.push((t.backend_id.clone(), t.api_key.clone(), t.base_url.clone()));
            let key = t.api_key.clone();
            async move {
                assert_eq!(key, "cheap-key");
                Ok("{\"suggestions\":[]}".into())
            }
        })
        .await
        .unwrap();
        assert!(text.contains("suggestions"));
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "deepseek");
        assert_eq!(seen[0].1, "cheap-key");
        assert_eq!(seen[0].2, "https://cheap.example");
    }

    #[tokio::test]
    async fn preferred_failure_falls_back_to_session_target() {
        let targets = vec![
            target("cheap", "deepseek", "mini", "cheap-key"),
            target("main", "openai", "gpt", "main-key"),
        ];
        let mut calls = 0usize;
        let text = complete_review_with_targets(&targets, |t| {
            calls += 1;
            let key = t.api_key.clone();
            let n = calls;
            async move {
                if n == 1 {
                    Err("provider error".into())
                } else {
                    assert_eq!(key, "main-key");
                    Ok("ok-from-fallback".into())
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(text, "ok-from-fallback");
        assert_eq!(calls, 2);
    }

    #[tokio::test]
    async fn both_failures_return_error_without_panic() {
        let targets = vec![
            target("cheap", "deepseek", "mini", "cheap-key"),
            target("main", "openai", "gpt", "main-key"),
        ];
        let err =
            complete_review_with_targets(&targets, |_t| async { Err("provider error".into()) })
                .await
                .expect_err("both failures");
        assert!(err.contains("provider error"));
    }
}
