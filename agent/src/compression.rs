//! Mid-run tool result compression inspired by Agno's `CompressionManager`.
//!
//! The invariant is important: `Message::content` and the DB `content` column keep
//! the original tool output, while `compressed_content` is only the provider-facing
//! view used on subsequent model calls.
//!
//! Triggers (either fires):
//! - count: uncompressed tool results ≥ [`ToolCompressionManager::tool_results_limit`]
//! - token: estimated session tokens ≥ [`ToolCompressionManager::compress_token_limit`]

use common::message::{Message, Role};

use crate::prompt::context_usage::estimate_tokens;

pub const DEFAULT_TOOL_RESULTS_LIMIT: usize = 3;
/// 默认 token 阈值（ceil(chars/4) 估算）；与 Agno `compress_token_limit` 对齐为可选第二触发器。
pub const DEFAULT_COMPRESS_TOKEN_LIMIT: usize = 16_000;
const DEFAULT_MAX_COMPRESSED_CHARS: usize = 1800;
const HEAD_CHARS: usize = 1100;
const TAIL_CHARS: usize = 500;

#[derive(Debug, Clone)]
pub struct ToolCompressionManager {
    pub enabled: bool,
    /// 未压缩 tool 结果条数阈值；`0` 表示关闭条数触发。
    pub tool_results_limit: usize,
    /// 会话消息估算 token 阈值；`None` 表示关闭 token 触发。
    pub compress_token_limit: Option<usize>,
    pub max_compressed_chars: usize,
}

impl Default for ToolCompressionManager {
    fn default() -> Self {
        Self {
            enabled: true,
            tool_results_limit: DEFAULT_TOOL_RESULTS_LIMIT,
            compress_token_limit: Some(DEFAULT_COMPRESS_TOKEN_LIMIT),
            max_compressed_chars: DEFAULT_MAX_COMPRESSED_CHARS,
        }
    }
}

impl ToolCompressionManager {
    /// 是否应压缩：有未压缩 tool 结果，且（条数超限 **或** token 超限）。
    pub fn should_compress(&self, messages: &[Message]) -> bool {
        if !self.enabled {
            return false;
        }
        let uncompressed = uncompressed_tool_result_count(messages);
        if uncompressed == 0 {
            return false;
        }
        if self.tool_results_limit > 0 && uncompressed >= self.tool_results_limit {
            return true;
        }
        if let Some(limit) = self.compress_token_limit {
            if limit > 0 && estimate_messages_tokens(messages) as usize >= limit {
                return true;
            }
        }
        false
    }

    pub fn compress_content(&self, tool_name: Option<&str>, content: &str) -> Option<String> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return None;
        }

        let char_count = trimmed.chars().count();
        if char_count <= self.max_compressed_chars {
            // Still mark it as compressed when the threshold fires, so the same tool
            // row is not repeatedly reconsidered in later rounds.
            return Some(trimmed.to_string());
        }

        let head: String = trimmed.chars().take(HEAD_CHARS).collect();
        let tail_vec: Vec<char> = trimmed.chars().rev().take(TAIL_CHARS).collect();
        let tail: String = tail_vec.into_iter().rev().collect();
        let removed = char_count.saturating_sub(HEAD_CHARS + TAIL_CHARS);
        let name = tool_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown");

        Some(format!(
            "[astro:compressed-tool-result]\nTool: {name}\nOriginal chars: {char_count}; removed middle chars: {removed}.\nPreserved head/tail because the full result is stored in session history.\n\n{head}\n\n...[compressed middle omitted]...\n\n{tail}"
        ))
    }
}

pub fn uncompressed_tool_result_count(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| m.role == Role::Tool && m.compressed_content.is_none())
        .count()
}

/// 估算会话消息发给模型时的 token 量（ceil(chars/4)）。
///
/// tool 角色优先计 `compressed_content`；assistant 的 tool_calls JSON 一并计入。
pub fn estimate_messages_tokens(messages: &[Message]) -> u32 {
    let mut total_chars = 0usize;
    for m in messages {
        total_chars = total_chars.saturating_add(provider_facing_chars(m));
    }
    estimate_tokens(total_chars)
}

fn provider_facing_chars(m: &Message) -> usize {
    let body = m
        .compressed_content
        .as_deref()
        .map(|s| s.chars().count())
        .unwrap_or_else(|| m.content_str().chars().count());
    let tool_calls = m
        .tool_calls
        .as_ref()
        .and_then(|tc| serde_json::to_string(tc).ok())
        .map(|s| s.chars().count())
        .unwrap_or(0);
    body.saturating_add(tool_calls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_counts_uncompressed_tool_results() {
        let mut messages = vec![
            Message::tool("a"),
            Message::tool("b"),
            Message::tool("c"),
        ];
        let mgr = ToolCompressionManager::default();
        assert!(mgr.should_compress(&messages));
        messages[0].compressed_content = Some("a".into());
        assert!(!mgr.should_compress(&messages));
    }

    #[test]
    fn token_threshold_triggers_with_few_tools() {
        let mgr = ToolCompressionManager {
            enabled: true,
            tool_results_limit: 99, // count alone won't fire
            compress_token_limit: Some(100),
            max_compressed_chars: DEFAULT_MAX_COMPRESSED_CHARS,
        };
        // ~800 chars → ~200 tokens
        let big = "x".repeat(800);
        let messages = vec![Message::tool(&big), Message::user("hi")];
        assert!(mgr.should_compress(&messages));
    }

    #[test]
    fn token_threshold_disabled_skips_token_path() {
        let mgr = ToolCompressionManager {
            enabled: true,
            tool_results_limit: 99,
            compress_token_limit: None,
            max_compressed_chars: DEFAULT_MAX_COMPRESSED_CHARS,
        };
        let big = "x".repeat(80_000);
        let messages = vec![Message::tool(&big)];
        assert!(!mgr.should_compress(&messages));
    }

    #[test]
    fn no_uncompressed_tools_never_triggers() {
        let mut messages = vec![Message::tool("a"), Message::tool("b"), Message::tool("c")];
        for m in &mut messages {
            m.compressed_content = Some("done".into());
        }
        let mgr = ToolCompressionManager {
            compress_token_limit: Some(1),
            ..ToolCompressionManager::default()
        };
        assert!(!mgr.should_compress(&messages));
    }

    #[test]
    fn estimate_prefers_compressed_view() {
        let big = "x".repeat(400);
        let mut m = Message::tool(&big);
        let before = estimate_messages_tokens(std::slice::from_ref(&m));
        m.compressed_content = Some("short".into());
        let after = estimate_messages_tokens(std::slice::from_ref(&m));
        assert!(after < before);
    }

    #[test]
    fn heuristic_keeps_tool_facts_and_shortens_long_output() {
        let mgr = ToolCompressionManager::default();
        let content = format!(
            "id=abc123 price=42\n{}\nfinal_url=https://example.com/end",
            "middle filler ".repeat(400)
        );
        let compressed = mgr
            .compress_content(Some("search"), &content)
            .expect("compressed");
        assert!(compressed.len() < content.len());
        assert!(compressed.contains("id=abc123"));
        assert!(compressed.contains("final_url=https://example.com/end"));
    }
}
