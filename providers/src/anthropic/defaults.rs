//! Anthropic Messages API 常量。

pub const DEFAULT_API_BASE: &str = "https://api.anthropic.com";

pub const ANTHROPIC_VERSION: &str = "2024-10-22";

pub const ANTHROPIC_BETA: &str = "prompt-caching-2024-07-31,pdfs-2024-09-25,token-counting-2024-11-01,interleaved-thinking-2025-05-14";

pub const THINKING_BUDGET_HIGH: u32 = 10_240;
pub const THINKING_BUDGET_MAX: u32 = 32_768;
