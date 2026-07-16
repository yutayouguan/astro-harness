//! Mid-run tool result compression inspired by Agno's `CompressionManager`.
//!
//! The invariant is important: `Message::content` and the DB `content` column keep
//! the original tool output, while `compressed_content` is only the provider-facing
//! view used on subsequent model calls.

use common::message::{Message, Role};

pub const DEFAULT_TOOL_RESULTS_LIMIT: usize = 3;
const DEFAULT_MAX_COMPRESSED_CHARS: usize = 1800;
const HEAD_CHARS: usize = 1100;
const TAIL_CHARS: usize = 500;

#[derive(Debug, Clone)]
pub struct ToolCompressionManager {
    pub enabled: bool,
    pub tool_results_limit: usize,
    pub max_compressed_chars: usize,
}

impl Default for ToolCompressionManager {
    fn default() -> Self {
        Self {
            enabled: true,
            tool_results_limit: DEFAULT_TOOL_RESULTS_LIMIT,
            max_compressed_chars: DEFAULT_MAX_COMPRESSED_CHARS,
        }
    }
}

impl ToolCompressionManager {
    pub fn should_compress(&self, messages: &[Message]) -> bool {
        self.enabled
            && self.tool_results_limit > 0
            && uncompressed_tool_result_count(messages) >= self.tool_results_limit
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
