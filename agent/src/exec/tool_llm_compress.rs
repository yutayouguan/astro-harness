//! Agno 式逐条 LLM 摘要 tool 结果（替换 / 回退 head-tail）。
//!
//! 不变量：只改 `compressed_content`；DB `content` 全文保留。
//! 辅模型：`AuxiliaryTask::Compaction`；失败或无目标时回退 head/tail。

use futures::StreamExt;
use providers::registry::ProviderRegistry;
use providers::trait_::{ChatMessage, ProviderConfig};
use tracing::warn;

use common::TOOL_LLM_COMPRESS_MARK;

/// 单次维护中最多对多少条 tool 做 LLM 摘要（其余走 head/tail）。
pub const MAX_LLM_TOOL_COMPRESS_PER_PASS: usize = 6;
/// 送给辅模型的原文上限（字符），超出先截断再摘要。
pub const MAX_LLM_INPUT_CHARS: usize = 24_000;

pub fn tool_summary_prompt(tool_name: &str, content: &str, max_chars: usize) -> String {
    let clipped: String = content.chars().take(MAX_LLM_INPUT_CHARS).collect();
    let truncated = content.chars().count() > MAX_LLM_INPUT_CHARS;
    format!(
        "Compress ONE tool result for a coding agent context window.\n\
         Tool: {tool_name}\n\
         Target length: about {max_chars} characters or less.\n\
         Keep: numbers, dates, paths, IDs, URLs, errors, key facts, decisions.\n\
         Drop: boilerplate, banners, repeated whitespace, decorative formatting.\n\
         Reply with ONLY the compressed text (no preamble).\n\
         Same language as the tool output.\n\n\
         Tool output{trunc}:\n\n{clipped}",
        trunc = if truncated {
            " (input truncated for summarizer)"
        } else {
            ""
        },
    )
}

pub fn make_llm_compress_view(
    tool_name: Option<&str>,
    summary: &str,
    original_chars: usize,
) -> String {
    let name = tool_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown");
    let body = summary.trim();
    format!(
        "{TOOL_LLM_COMPRESS_MARK}\n\
         Tool: {name}\n\
         Original chars: {original_chars}. Full output remains in session DB.\n\
         Recovery: `search` (scope=session) or `file_ops` read on spill path if present.\n\n\
         {body}"
    )
}

async fn complete_compaction_chat(
    target: &common::ChatTarget,
    prompt: &str,
    max_tokens: u32,
) -> anyhow::Result<String> {
    let registry = ProviderRegistry::default();
    let provider = registry
        .get(&target.backend_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported compaction backend: {}", target.backend_id))?;
    let config = ProviderConfig {
        api_key: target.api_key.clone(),
        base_url: if target.base_url.trim().is_empty() {
            None
        } else {
            Some(target.base_url.trim_end_matches('/').to_string())
        },
        model: target.model.clone(),
        temperature: 0.2,
        max_tokens,
        thinking_enabled: false,
        reasoning_effort: "high".to_string(),
        additional_params: serde_json::Value::Null,
        ..ProviderConfig::default()
    };
    let messages = vec![
        ChatMessage::text(
            "system",
            "You compress tool outputs for coding agents. Preserve critical facts.",
        ),
        ChatMessage::text("user", prompt),
    ];
    let mut stream = provider.chat_stream(messages, vec![], &config).await?;
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item?;
        if let Some(token) = chunk.token {
            out.push_str(&token);
        }
    }
    let trimmed = out.trim().to_string();
    if trimmed.is_empty() {
        anyhow::bail!("empty tool LLM compress");
    }
    Ok(trimmed)
}

/// 对单条 tool 结果做 LLM 摘要；失败返回 Err（调用方回退 head/tail）。
pub async fn summarize_tool_result(
    targets: &[common::ChatTarget],
    tool_name: Option<&str>,
    content: &str,
    max_chars: usize,
) -> anyhow::Result<String> {
    if targets.is_empty() {
        anyhow::bail!("no compaction targets");
    }
    let name = tool_name.unwrap_or("unknown");
    let prompt = tool_summary_prompt(name, content, max_chars.max(200));
    let max_tokens = u32::try_from((max_chars / 3).clamp(256, 1_024)).unwrap_or(512);
    let mut last_err = None;
    for target in targets.iter().take(2) {
        match complete_compaction_chat(target, &prompt, max_tokens).await {
            Ok(text) => {
                return Ok(make_llm_compress_view(
                    tool_name,
                    &text,
                    content.chars().count(),
                ));
            }
            Err(e) => {
                warn!(
                    backend = %target.backend_id,
                    model = %target.model,
                    tool = %name,
                    error = %e,
                    "tool LLM compress target failed"
                );
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("tool LLM compress failed")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_mentions_tool_and_budget() {
        let p = tool_summary_prompt("file_ops", "hello world", 900);
        assert!(p.contains("file_ops"));
        assert!(p.contains("900"));
        assert!(p.contains("hello world"));
    }

    #[test]
    fn llm_view_uses_mark() {
        let v = make_llm_compress_view(Some("terminal"), "ls ok", 1200);
        assert!(v.starts_with(TOOL_LLM_COMPRESS_MARK));
        assert!(v.contains("terminal"));
        assert!(v.contains("ls ok"));
        assert!(common::is_externalized_view(&v));
    }
}
