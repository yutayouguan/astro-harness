//! OpenAI Chat Completions 兼容层 — 一行接入 OpenAI 兼容厂商。

pub mod chat_completions;
pub mod completion;
pub mod media;
pub mod responses;
pub mod sse;
pub mod think_tag;

pub use completion::{
    apply_thinking_compat, OpenAICompatible, OpenAICompletionModel, ThinkingFormat,
};
pub use responses::{OpenAIResponsesCompatible, OpenAIResponsesModel};

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
    let details = u
        .get("prompt_tokens_details")
        .or_else(|| u.get("input_tokens_details"));
    let cache_read_value = details
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|x| x.as_u64())
        .or_else(|| u.get("cache_read_input_tokens").and_then(|x| x.as_u64()));
    let cache_read = cache_read_value.unwrap_or(0) as u32;
    let cache_write_value = details
        .and_then(|d| d.get("cache_write_tokens"))
        .and_then(|x| x.as_u64())
        .or_else(|| {
            u.get("cache_creation_input_tokens")
                .and_then(|x| x.as_u64())
        });
    let cache_write = cache_write_value.unwrap_or(0) as u32;
    let input = prompt_total
        .saturating_sub(cache_read)
        .saturating_sub(cache_write);
    let reasoning_value = u
        .get("completion_tokens_details")
        .or_else(|| u.get("output_tokens_details"))
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(|x| x.as_u64());
    let reasoning = reasoning_value.unwrap_or(0) as u32;
    let reported_total_tokens = u
        .get("total_tokens")
        .and_then(|x| x.as_u64())
        .map(|x| x.min(u64::from(u32::MAX)) as u32);
    if input == 0
        && output == 0
        && cache_read == 0
        && cache_write == 0
        && reasoning == 0
        && reported_total_tokens.unwrap_or(0) == 0
    {
        return None;
    }
    Some(crate::types::stream::Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: reasoning,
        request_count: 1,
        reported_total_tokens,
        cache_read_reported: cache_read_value.is_some(),
        cache_write_reported: cache_write_value.is_some(),
        reasoning_reported: reasoning_value.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::parse_openai_usage;

    #[test]
    fn responses_usage_preserves_wire_total_and_reporting_status() {
        let usage = parse_openai_usage(&serde_json::json!({
            "usage": {
                "input_tokens": 100,
                "input_tokens_details": {"cached_tokens": 40, "cache_write_tokens": 10},
                "output_tokens": 25,
                "output_tokens_details": {"reasoning_tokens": 20},
                "total_tokens": 125
            }
        }))
        .expect("usage");

        assert_eq!(usage.input_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 40);
        assert_eq!(usage.cache_write_tokens, 10);
        assert_eq!(usage.output_tokens, 25);
        assert_eq!(usage.reasoning_tokens, 20);
        assert_eq!(usage.reported_total_tokens, Some(125));
        assert!(usage.cache_read_reported);
        assert!(usage.cache_write_reported);
        assert!(usage.reasoning_reported);
        assert_eq!(usage.total_tokens(), 125);
    }

    #[test]
    fn zero_detail_is_reported_while_absent_detail_is_not() {
        let reported = parse_openai_usage(&serde_json::json!({
            "usage": {
                "input_tokens": 10,
                "input_tokens_details": {"cached_tokens": 0},
                "output_tokens": 1
            }
        }))
        .expect("usage");
        let absent = parse_openai_usage(&serde_json::json!({
            "usage": {"input_tokens": 10, "output_tokens": 1}
        }))
        .expect("usage");

        assert!(reported.cache_read_reported);
        assert_eq!(reported.cache_read_tokens, 0);
        assert!(!absent.cache_read_reported);
        assert!(!absent.reasoning_reported);
    }
}
