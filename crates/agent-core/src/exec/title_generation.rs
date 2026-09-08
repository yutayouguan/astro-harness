//! 首轮助手回复后的异步会话标题生成。
//!
//! 由 backend 在成功的 Thread terminal 后 fire-and-forget。仅在标题仍为空时写入
//! （[`session::SessionStore::set_session_title_if_empty`]），迟到任务不会覆盖手动标题。

use futures::StreamExt;
use providers::types::stream::StreamChunk;
use providers::ProviderConfig;
use tracing::{info, warn};

use crate::runtime::AgentLoop;

const TITLE_MAX_CHARS: usize = 40;

/// 标题写入成功后的轻量通知。
#[derive(Debug, Clone)]
pub struct TitleChangedNotify {
    pub session_id: String,
    pub title: String,
}

/// 一次标题生成所需的快照。
#[derive(Debug, Clone)]
pub struct TitleGenerationJob {
    pub memory_dir: std::path::PathBuf,
    pub session_id: String,
    pub targets: Vec<types::ModelTarget>,
}

/// 从当前 [`AgentLoop`] 构造标题任务。
pub fn job_from_agent(agent: &AgentLoop) -> TitleGenerationJob {
    TitleGenerationJob {
        memory_dir: agent.memory_dir().to_path_buf(),
        session_id: agent.session_id().to_string(),
        targets: agent.auxiliary_targets(types::AuxiliaryTask::TitleGeneration),
    }
}

/// 成功的 Thread terminal 后异步生成标题；成功时可选推送 [`TitleChangedNotify`]。
pub fn spawn_title_generation_after_turn(
    agent: &AgentLoop,
    notify: Option<tokio::sync::mpsc::UnboundedSender<TitleChangedNotify>>,
) {
    let job = job_from_agent(agent);
    tokio::spawn(async move {
        match maybe_generate_session_title(job).await {
            Ok(Some(n)) => {
                if let Some(tx) = notify {
                    let _ = tx.send(n);
                }
            }
            Ok(None) => {}
            Err(e) => warn!(error = %e, "session title generation failed"),
        }
    });
}

fn build_title_prompt(user: &str, assistant: &str) -> String {
    format!(
        "Generate a short chat session title for the conversation below.\n\
         Rules:\n\
         - Reply with ONLY the title text\n\
         - No quotes, markdown, or explanation\n\
         - Prefer the same language as the user message\n\
         - At most {TITLE_MAX_CHARS} characters\n\n\
         User:\n{user}\n\n\
         Assistant:\n{assistant}"
    )
}

/// 按 preferred→fallback 完成标题文本。
pub async fn complete_title_with_targets<F, Fut>(
    targets: &[types::ModelTarget],
    mut complete: F,
) -> Result<String, String>
where
    F: FnMut(&types::ModelTarget) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let mut last_err = "no title targets".to_string();
    for target in targets.iter().take(2) {
        match complete(target).await {
            Ok(text) if !text.trim().is_empty() => return Ok(text),
            Ok(_) => {
                last_err = format!("empty title response from {}", target.backend_id);
            }
            Err(err) => {
                warn!(
                    backend = %target.backend_id,
                    model = %target.model,
                    error = %err,
                    "title generation target failed; trying next"
                );
                last_err = err;
            }
        }
    }
    Err(last_err)
}

/// 若标题为空且存在完整首轮，则生成并条件写入。
pub async fn maybe_generate_session_title(
    job: TitleGenerationJob,
) -> anyhow::Result<Option<TitleChangedNotify>> {
    let store = open_store(&job.memory_dir).await?;
    let meta = store
        .get_session(&job.session_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("session not found"))?;
    if meta
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some()
    {
        return Ok(None);
    }

    let Some((user, assistant)) = store.first_turn_text(&job.session_id).await? else {
        return Ok(None);
    };
    drop(store);

    if job.targets.is_empty() {
        warn!("title generation skipped: empty targets");
        return Ok(None);
    }

    let prompt = build_title_prompt(&user, &assistant);
    let raw = complete_title_with_targets(&job.targets, |target| {
        let prompt = prompt.clone();
        let target = target.clone();
        async move {
            complete_title_response(&target, &prompt)
                .await
                .map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?;

    let title = types::sanitize_title(&raw, TITLE_MAX_CHARS);
    if title.is_empty() {
        warn!("title generation produced empty sanitized title");
        return Ok(None);
    }

    let store = open_store(&job.memory_dir).await?;
    let wrote = store
        .set_session_title_if_empty(&job.session_id, &title)
        .await?;
    if !wrote {
        info!(
            session = %job.session_id,
            "title generation skipped: title already set"
        );
        return Ok(None);
    }

    info!(session = %job.session_id, title = %title, "session title generated");
    Ok(Some(TitleChangedNotify {
        session_id: job.session_id,
        title,
    }))
}

async fn open_store(memory_dir: &std::path::Path) -> anyhow::Result<session::SessionStore> {
    memory::ensure_workspace(memory_dir)?;
    session::SessionStore::open_sessions_dir(&home::sessions_dir(memory_dir)).await
}

async fn complete_title_response(
    target: &types::ModelTarget,
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
        temperature: 0.3,
        max_tokens: 64,
        thinking_enabled: false,
        reasoning_effort: "high".to_string(),
        additional_params: serde_json::Value::Null,
        ..ProviderConfig::default()
    };
    let mut stream = providers::dispatch::agent_responses_prompt(
        &target.backend_id,
        "Generate a concise title for this agent task.",
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
    if out.trim().is_empty() {
        anyhow::bail!("title generation model returned empty content");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn preferred_failure_falls_back() {
        let targets = vec![
            types::ModelTarget {
                provider_id: "a".into(),
                backend_id: "deepseek".into(),
                model: "mini".into(),
                api_key: "k1".into(),
                base_url: "https://a".into(),
            },
            types::ModelTarget {
                provider_id: "b".into(),
                backend_id: "openai".into(),
                model: "gpt".into(),
                api_key: "k2".into(),
                base_url: "https://b".into(),
            },
        ];
        let mut calls = 0usize;
        let text = complete_title_with_targets(&targets, |_t| {
            calls += 1;
            let n = calls;
            async move {
                if n == 1 {
                    Err("boom".into())
                } else {
                    Ok(" **「Rust 会话管理」** ".into())
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(types::sanitize_title(&text, 20), "Rust 会话管理");
        assert_eq!(calls, 2);
    }
}
