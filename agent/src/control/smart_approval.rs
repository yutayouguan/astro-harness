//! 辅模型 Smart 审批：仅对规则 `Ask` 可选降级为 `Auto`。
//!
//! 默认关闭；`ASTRO_SMART_APPROVAL=1` 开启。失败 / 超时一律回退 `Ask`。

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use providers::trait_::{AiProvider, ChatMessage, ProviderConfig};
use tools::ApprovalAction;

const SMART_TIMEOUT: Duration = Duration::from_secs(8);

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

/// 对 `Ask` 命令尝试辅模型降级；未开启或失败时返回 `Ask`。
pub async fn maybe_smart_downgrade_ask(
    command: &str,
    description: &str,
    provider: Arc<dyn AiProvider>,
    config: ProviderConfig,
) -> ApprovalAction {
    if !smart_approval_enabled() {
        return ApprovalAction::Ask;
    }
    match tokio::time::timeout(
        SMART_TIMEOUT,
        ask_model(provider, config, command, description),
    )
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
    provider: Arc<dyn AiProvider>,
    config: ProviderConfig,
    command: &str,
    description: &str,
) -> anyhow::Result<ApprovalAction> {
    let prompt = build_prompt(command, description);
    let messages = vec![ChatMessage::text("user", prompt)];
    let mut stream = provider
        .chat_stream(messages, vec![], &config)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut full = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| anyhow::anyhow!("{e}"))?;
        if let Some(token) = chunk.token {
            full.push_str(&token);
        }
    }
    if full.trim().is_empty() {
        anyhow::bail!("empty smart-approval response");
    }
    Ok(parse_smart_verdict(&full))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
