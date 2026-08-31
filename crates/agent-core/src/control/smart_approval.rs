//! 独立自动审批审查器：仅对已经需要审批的请求返回结构化 verdict。
//!
//! 失败、超时或无效输出一律 fail closed。
//! 目标链由 ChatRequest 注入的 `AuxiliaryTask::SmartApproval` 提供（preferred + 可选 fallback）。

use std::time::Duration;

use futures::StreamExt;
use providers::types::stream::StreamChunk;
use providers::ProviderConfig;
use serde::Serialize;
use types::ApprovalAction;

const SMART_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CONTENT_PREVIEW_CHARS: usize = 200;

/// 单个智能审批调用目标（最多 preferred + fallback 两项）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalTarget {
    pub backend_id: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
}

impl From<&types::ChatTarget> for ApprovalTarget {
    fn from(t: &types::ChatTarget) -> Self {
        Self {
            backend_id: t.backend_id.clone(),
            model: t.model.clone(),
            api_key: t.api_key.clone(),
            base_url: t.base_url.clone(),
        }
    }
}

/// 对话上下文摘要，供辅模型判断命令是否在合理语境中。
#[derive(Debug, Clone, Serialize)]
pub struct SmartApprovalContext {
    pub recent_turns: Vec<TurnSummary>,
    pub current_task_description: Option<String>,
    pub tool_call_chain: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnSummary {
    pub role: String,
    pub content_preview: String,
}

static SENSITIVE_PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)(api[_-]?key|token|secret|password|bearer|credential|auth[_-]?key)\s*[=:]\s*\S+",
    )
    .unwrap()
});

pub fn redact_sensitive(text: &str) -> String {
    SENSITIVE_PATTERN
        .replace_all(text, "[REDACTED]")
        .into_owned()
}

pub fn truncate_preview(text: &str) -> String {
    let cleaned = redact_sensitive(text);
    if cleaned.chars().count() <= MAX_CONTENT_PREVIEW_CHARS {
        cleaned
    } else {
        let truncated: String = cleaned.chars().take(MAX_CONTENT_PREVIEW_CHARS).collect();
        format!("{truncated}...")
    }
}

/// 解析审查器 JSON：仅明确的 `approve_once` 才放行。
pub fn parse_smart_verdict(text: &str) -> ApprovalAction {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return ApprovalAction::Ask;
    };
    match value.get("decision").and_then(|value| value.as_str()) {
        Some("approve_once") => ApprovalAction::Auto,
        _ => ApprovalAction::Ask,
    }
}

fn build_prompt(
    request: &types::PermissionRequest,
    context: Option<&SmartApprovalContext>,
) -> String {
    let request_json = serde_json::to_string(request).unwrap_or_else(|_| "{}".to_string());
    let context_section = match context {
        Some(ctx) => {
            let ctx_json = serde_json::to_string(ctx).unwrap_or_else(|_| "{}".to_string());
            format!(
                "\nConversation context (redacted):\n```json\n{ctx_json}\n```\n\
                 Use this context to judge whether the command is reasonable given the ongoing task.\n"
            )
        }
        None => String::new(),
    };
    format!(
        "You are an independent security reviewer for a developer agent.\n\
         Review only the exact permission request below; this review cannot expand its sandbox.\n\
         Permission request:\n```json\n{request_json}\n```\n\
         {context_section}\
         Return JSON only: {{\"decision\":\"approve_once|deny\",\"reason\":\"...\",\"risk\":\"low|medium|high|critical\"}}.\n\
         Deny credential probing, exfiltration, persistent security weakening, destructive actions, or any uncertain request."
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
    request: &types::PermissionRequest,
    targets: &[ApprovalTarget],
    context: Option<&SmartApprovalContext>,
) -> ApprovalAction {
    if targets.is_empty() {
        return ApprovalAction::Ask;
    }

    let prompt = build_prompt(request, context);
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

async fn ask_model(target: &ApprovalTarget, prompt: &str) -> Result<String, String> {
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
    let mut stream = providers::dispatch::agent_responses_prompt(
        &target.backend_id,
        "You decide whether a requested tool action is safe to approve.",
        prompt,
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
    fn parse_structured_verdict_fails_closed() {
        assert_eq!(
            parse_smart_verdict(r#"{"decision":"approve_once","risk":"low"}"#),
            ApprovalAction::Auto
        );
        assert_eq!(
            parse_smart_verdict(r#"{"decision":"deny","risk":"high"}"#),
            ApprovalAction::Ask
        );
        assert_eq!(
            parse_smart_verdict(r#"{"decision":"approve_session","risk":"low"}"#),
            ApprovalAction::Ask
        );
        assert_eq!(parse_smart_verdict("AUTO"), ApprovalAction::Ask);
        assert_eq!(parse_smart_verdict("AUTO please"), ApprovalAction::Ask);
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
                    Ok(r#"{"decision":"deny","risk":"high"}"#.into())
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

    #[test]
    fn redact_sensitive_values() {
        assert_eq!(
            redact_sensitive("export API_KEY=sk-1234abc"),
            "export [REDACTED]"
        );
        assert_eq!(
            redact_sensitive("token: ghp_secret123 rest"),
            "[REDACTED] rest"
        );
        assert_eq!(redact_sensitive("no secrets here"), "no secrets here");
        assert_eq!(redact_sensitive("PASSWORD=hunter2"), "[REDACTED]");
    }

    #[test]
    fn truncate_long_preview() {
        let short = "cargo test";
        assert_eq!(truncate_preview(short), "cargo test");

        let long = "x".repeat(300);
        let result = truncate_preview(&long);
        assert!(result.ends_with("..."));
        assert!(result.chars().count() <= MAX_CONTENT_PREVIEW_CHARS + 3);
    }

    #[test]
    fn build_prompt_includes_context_when_provided() {
        let request = types::PermissionRequest {
            request_id: "r1".into(),
            session_id: "s1".into(),
            turn_id: None,
            tool_call_id: "tc1".into(),
            tool_name: "exec_command".into(),
            summary: "run cargo test".into(),
            capabilities: vec![],
            reason: types::PermissionReason::UntrustedCommand,
            requested_scope: types::GrantScope::Once,
            command_preview: Some("cargo test".into()),
            affected_paths: vec![],
            network_hosts: vec![],
        };

        let no_ctx = build_prompt(&request, None);
        assert!(!no_ctx.contains("Conversation context"));

        let ctx = SmartApprovalContext {
            recent_turns: vec![TurnSummary {
                role: "user".into(),
                content_preview: "please run tests".into(),
            }],
            current_task_description: Some("running unit tests".into()),
            tool_call_chain: vec!["read_file".into()],
        };
        let with_ctx = build_prompt(&request, Some(&ctx));
        assert!(with_ctx.contains("Conversation context"));
        assert!(with_ctx.contains("please run tests"));
    }
}
