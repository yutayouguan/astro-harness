//! 辅模型 Smart 审批：仅对规则 `Ask` 可选降级为 `Auto`。
//!
//! 默认关闭；`ASTRO_SMART_APPROVAL=1` 开启。失败 / 超时一律回退 `Ask`。
//! 目标链由 ChatRequest 注入的 `AuxiliaryTask::SmartApproval` 提供（preferred + 可选 fallback）。

use std::time::Duration;

use futures::StreamExt;
use providers::ProviderConfig;
use providers::types::message::Message as ProviderMessage;
use providers::types::stream::StreamChunk;
use tools::ApprovalAction;

const SMART_TIMEOUT: Duration = Duration::from_secs(8);

/// 单个智能审批调用目标（最多 preferred + fallback 两项）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalTarget {
    pub backend_id: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
}

impl From<&common::ChatTarget> for ApprovalTarget {
    fn from(t: &common::ChatTarget) -> Self {
        Self {
            backend_id: t.backend_id.clone(),
            model: t.model.clone(),
            api_key: t.api_key.clone(),
            base_url: t.base_url.clone(),
        }
    }
}

/// 是否启用辅模型审批。
pub fn smart_approval_enabled() -> bool {
    matches!(
        std::env::var("ASTRO_SMART_APPROVAL").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes")
    )
}

/// 解析辅模型回复：仅当明确 `AUTO` 时放行。
pub fn parse_smart_verdict(text: &str) -> ApprovalAction {
    let upper = text.to_uppercase();
    // 取首个非空行或整段中的独立词
    for token in upper.split_whitespace() {
        let t = token.trim_matches(|c: char| !c.is_ascii_alphabetic());
        if t == "AUTO" {
            return ApprovalAction::Auto;
        }
        if t == "ASK" {
            return ApprovalAction::Ask;
        }
    }
    if upper.contains("AUTO") && !upper.contains("ASK") {
        return ApprovalAction::Auto;
    }
    ApprovalAction::Ask
}

fn build_prompt(command: &str, description: &str) -> String {
    format!(
        "You are a security reviewer for shell commands in a developer agent.\n\
         The command already matched a \"needs approval\" rule: {description}.\n\
         Command:\n```\n{command}\n```\n\
         Reply with exactly one word: AUTO (safe to auto-run in a project workspace) or ASK (need human approval).\n\
         Prefer ASK when unsure. Never invent other words."
    )
}

/// 按 preferred→fallback 顺序完成；返回首个可解析 verdict。
///
/// 全部失败返回 `Err`，由调用方保留原 `Ask`（不自动 allow）。
pub async fn evaluate_smart_approval_with_completion<F, Fut>(
    targets: &[ApprovalTarget],
    mut complete: F,
) -> Result<ApprovalAction, String>
where
    F: FnMut(&ApprovalTarget) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let mut last_err = "no approval targets".to_string();
    for target in targets.iter().take(2) {
        match complete(target).await {
            Ok(text) if !text.trim().is_empty() => return Ok(parse_smart_verdict(&text)),
            Ok(_) => {
                last_err = format!("empty smart-approval response from {}", target.backend_id);
            }
            Err(err) => {
                tracing::warn!(
                    backend = %target.backend_id,
                    model = %target.model,
                    error = %err,
                    "smart approval target failed; trying next"
                );
                last_err = err;
            }
        }
    }
    Err(last_err)
}

/// 对 `Ask` 命令尝试辅模型降级；未开启或失败时返回 `Ask`。
pub async fn maybe_smart_downgrade_ask(
    command: &str,
    description: &str,
    targets: &[ApprovalTarget],
) -> ApprovalAction {
    if !smart_approval_enabled() {
        return ApprovalAction::Ask;
    }
    if targets.is_empty() {
        return ApprovalAction::Ask;
    }

    let prompt = build_prompt(command, description);
    match tokio::time::timeout(SMART_TIMEOUT, async {
        evaluate_smart_approval_with_completion(targets, |target| {
            let prompt = prompt.clone();
            let target = target.clone();
            async move { ask_model(&target, &prompt).await }
        })
        .await
    })
    .await
    {
        Ok(Ok(action)) => action,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "smart approval LLM failed; falling back to Ask");
            ApprovalAction::Ask
        }
        Err(_) => {
            tracing::warn!("smart approval timed out; falling back to Ask");
            ApprovalAction::Ask
        }
    }
}

async fn ask_model(
    target: &ApprovalTarget,
    prompt: &str,
) -> Result<String, String> {
    let config = ProviderConfig {
        model: if target.model.trim().is_empty() {
            providers::default_model(&target.backend_id)
        } else {
            target.model.clone()
        },
        api_key: target.api_key.clone(),
        base_url: if target.base_url.trim().is_empty() {
            None
        } else {
            Some(target.base_url.clone())
        },
        ..ProviderConfig::default()
    };
    let messages = vec![ProviderMessage::user_text(prompt)];
    let mut stream = providers::dispatch::chat_stream(
        &target.backend_id,
        messages,
        vec![],
        &config,
    )
    .await
    .map_err(|e| e.to_string())?;

    let mut full = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        if let StreamChunk::Text(token) = chunk {
            full.push_str(&token);
        }
    }
    if full.trim().is_empty() {
        return Err("empty smart-approval response".into());
    }
    Ok(full)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(backend: &str, model: &str) -> ApprovalTarget {
        ApprovalTarget {
            backend_id: backend.into(),
            model: model.into(),
            api_key: "k".into(),
            base_url: "https://example".into(),
        }
    }

    #[test]
    fn parse_auto_and_ask() {
        assert_eq!(parse_smart_verdict("AUTO"), ApprovalAction::Auto);
        assert_eq!(parse_smart_verdict("auto\n"), ApprovalAction::Auto);
        assert_eq!(parse_smart_verdict("ASK"), ApprovalAction::Ask);
        assert_eq!(parse_smart_verdict("I think ASK"), ApprovalAction::Ask);
        assert_eq!(parse_smart_verdict("maybe"), ApprovalAction::Ask);
        assert_eq!(parse_smart_verdict("AUTO please"), ApprovalAction::Auto);
    }

    #[test]
    fn prefer_ask_when_both_mentioned() {
        // 含 ASK 时第一轮 token 扫描会先命中 ASK（若 AUTO 在前则 Auto）
        assert_eq!(parse_smart_verdict("ASK not AUTO"), ApprovalAction::Ask);
    }

    #[tokio::test]
    async fn disabled_returns_ask_without_llm() {
        std::env::remove_var("ASTRO_SMART_APPROVAL");
        // 无可用 provider 调用路径：直接查 enabled
        assert!(!smart_approval_enabled());
    }

    #[tokio::test]
    async fn preferred_error_falls_back_to_second_target_ask() {
        let targets = vec![target("pref", "mini"), target("fallback", "main")];
        let mut calls = 0usize;
        let verdict = evaluate_smart_approval_with_completion(&targets, |_t| {
            calls += 1;
            async move {
                if calls == 1 {
                    Err("provider error".into())
                } else {
                    Ok("ASK".into())
                }
            }
        })
        .await
        .expect("fallback should succeed");
        assert_eq!(verdict, ApprovalAction::Ask);
        assert_eq!(calls, 2);
    }

    #[tokio::test]
    async fn both_targets_fail_returns_error_not_auto() {
        let targets = vec![target("pref", "mini"), target("fallback", "main")];
        let err = evaluate_smart_approval_with_completion(&targets, |_t| async {
            Err("provider error".into())
        })
        .await
        .expect_err("both failures should surface");
        assert!(err.contains("provider error"));
    }

    #[tokio::test]
    async fn empty_targets_returns_error() {
        let err = evaluate_smart_approval_with_completion(&[], |_t| async { Ok("AUTO".into()) })
            .await
            .expect_err("empty targets");
        assert_eq!(err, "no approval targets");
    }
}
