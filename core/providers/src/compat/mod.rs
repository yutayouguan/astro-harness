//! OpenAI Chat Completions 兼容层 — 一行接入 OpenAI 兼容厂商。

pub mod completion;
pub mod messages;
pub mod sse;
pub mod think_tag;

pub use completion::{OpenAICompatible, OpenAICompletionModel};

/// 规范化 OpenAI 兼容 API 基址（自动补 `/v1` 等后缀）。
///
/// Gemini OpenAI 兼容基址以 `/openai` 结尾，不得再追加 `/v1`。
pub fn openai_compatible_base(endpoint: &str) -> String {
    let base = crate::http_stream::trim_slash(endpoint);
    if base.ends_with("/v1")
        || base.ends_with("/v3")
        || base.ends_with("/v4")
        || base.ends_with("/openai")
        || base.contains("/paas/v4")
        || base.contains("/v1beta/openai")
    {
        base
    } else if base.is_empty() {
        crate::openai::DEFAULT_API_BASE.to_string()
    } else {
        format!("{base}/v1")
    }
}

/// 从 OpenAI 风格 JSON 解析 usage（返回旧 `streaming::Usage` 类型）。
///
/// 供旧代码路径（responses.rs、media 模块）使用。
pub fn parse_openai_usage(v: &serde_json::Value) -> Option<crate::types::stream::Usage> {
    let u = v.get("usage")?;
    if u.is_null() {
        return None;
    }
    let prompt_total = u
        .get("prompt_tokens")
        .or_else(|| u.get("input_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let output = u
        .get("completion_tokens")
        .or_else(|| u.get("output_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let details = u.get("prompt_tokens_details");
    let mut cache_read = details
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    if cache_read == 0 {
        cache_read = u
            .get("cache_read_input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
    }
    let mut cache_write = details
        .and_then(|d| d.get("cache_write_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    if cache_write == 0 {
        cache_write = u
            .get("cache_creation_input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
    }
    let input = prompt_total
        .saturating_sub(cache_read)
        .saturating_sub(cache_write);
    let reasoning = u
        .get("completion_tokens_details")
        .or_else(|| u.get("output_tokens_details"))
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 && reasoning == 0 {
        return None;
    }
    Some(crate::types::stream::Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: reasoning,
        request_count: 1,
    })
}
