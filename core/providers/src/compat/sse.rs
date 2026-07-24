//! OpenAI SSE 事件解析（统一输出 StreamChunk）。

use serde_json::Value;

use crate::types::stream::{StreamChunk, Usage};

/// 解析 OpenAI SSE `data:` 负载为 [`StreamChunk`]。
pub fn extract_openai_delta(data: &str) -> Option<StreamChunk> {
    let v: Value = serde_json::from_str(data).ok()?;

    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("API error");
        return Some(StreamChunk::Error(msg.to_string()));
    }

    let usage = parse_usage(&v);
    let choice = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());

    let Some(choice) = choice else {
        return usage.map(StreamChunk::Usage);
    };

    let finish = choice
        .get("finish_reason")
        .and_then(|f| f.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| StreamChunk::Done {
            finish_reason: s.to_string(),
        });

    let token = choice
        .pointer("/delta/content")
        .or_else(|| choice.pointer("/message/content"))
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| StreamChunk::Text(s.to_string()));

    let reasoning = choice
        .pointer("/delta/reasoning_content")
        .or_else(|| choice.pointer("/message/reasoning_content"))
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| StreamChunk::Thinking(s.to_string()))
        .or_else(|| {
            // MiniMax reasoning_details: [{type:"reasoning.text", text:"..."}]
            let arr = choice
                .pointer("/delta/reasoning_details")
                .or_else(|| choice.pointer("/message/reasoning_details"))
                .and_then(|v| v.as_array())?;
            let text: String = arr
                .iter()
                .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("");
            if text.is_empty() {
                None
            } else {
                Some(StreamChunk::Thinking(text))
            }
        });

    let tool_deltas = parse_tool_deltas(choice);

    // MiniMax interleaved thinking: 一个 delta 可能同时含 content="\n" 和 tool_calls。
    // 纯空白 text 不应遮蔽 tool_calls / reasoning。
    let meaningful_text = token
        .as_ref()
        .map_or(false, |t| matches!(t, StreamChunk::Text(s) if !s.trim().is_empty()));

    if meaningful_text {
        return token;
    }
    if let Some(r) = reasoning {
        return Some(r);
    }
    if let Some(td) = tool_deltas {
        return Some(td);
    }
    if let Some(t) = token {
        return Some(t);
    }
    if let Some(f) = finish {
        return Some(f);
    }
    if let Some(u) = usage {
        return Some(StreamChunk::Usage(u));
    }
    None
}

fn parse_tool_deltas(choice: &Value) -> Option<StreamChunk> {
    let arr = choice
        .pointer("/delta/tool_calls")
        .or_else(|| choice.pointer("/message/tool_calls"))
        .and_then(|v| v.as_array())?;

    let tc = arr.first()?;
    let index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
    let id = tc.get("id").and_then(|s| s.as_str()).map(str::to_string);
    let name = tc
        .pointer("/function/name")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let arguments = tc
        .pointer("/function/arguments")
        .and_then(|s| s.as_str())
        .map(str::to_string);

    if let Some(id) = id {
        Some(StreamChunk::ToolCallStart {
            index,
            id,
            name: name.unwrap_or_default(),
        })
    } else {
        arguments.map(|args| StreamChunk::ToolCallDelta { index, arguments: args })
    }
}

fn parse_usage(v: &Value) -> Option<Usage> {
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
    let cache_read = details
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let cache_write = details
        .and_then(|d| d.get("cache_write_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let input = prompt_total
        .saturating_sub(cache_read)
        .saturating_sub(cache_write);
    let reasoning = u
        .get("completion_tokens_details")
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 && reasoning == 0 {
        return None;
    }
    Some(Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: reasoning,
        request_count: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_delta() {
        let data = r#"{"choices":[{"delta":{"content":"Hello"}}]}"#;
        match extract_openai_delta(data) {
            Some(StreamChunk::Text(t)) => assert_eq!(t, "Hello"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn usage_only() {
        let data = r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#;
        match extract_openai_delta(data) {
            Some(StreamChunk::Usage(u)) => {
                assert_eq!(u.input_tokens, 10);
                assert_eq!(u.output_tokens, 5);
            }
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_start() {
        let data = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read","arguments":""}}]}}]}"#;
        match extract_openai_delta(data) {
            Some(StreamChunk::ToolCallStart { id, name, .. }) => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "read");
            }
            other => panic!("expected ToolCallStart, got {other:?}"),
        }
    }

    #[test]
    fn error_event() {
        let data = r#"{"error":{"message":"Rate limited"}}"#;
        match extract_openai_delta(data) {
            Some(StreamChunk::Error(msg)) => assert_eq!(msg, "Rate limited"),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn whitespace_content_with_tool_call_yields_tool() {
        // MiniMax interleaved thinking: content="\n" + tool_calls 同时出现
        let data = r#"{"choices":[{"delta":{"content":"\n","tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather","arguments":""}}]}}]}"#;
        match extract_openai_delta(data) {
            Some(StreamChunk::ToolCallStart { id, name, .. }) => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "get_weather");
            }
            other => panic!("expected ToolCallStart (not whitespace Text), got {other:?}"),
        }
    }

    #[test]
    fn meaningful_content_still_wins_over_tool_call() {
        let data = r#"{"choices":[{"delta":{"content":"hello","tool_calls":[{"index":0,"id":"call_1","function":{"name":"f","arguments":""}}]}}]}"#;
        match extract_openai_delta(data) {
            Some(StreamChunk::Text(t)) => assert_eq!(t, "hello"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn reasoning_details_array_format() {
        // MiniMax reasoning_details 是数组格式
        let data = r#"{"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.text","text":"thinking..."}]}}]}"#;
        match extract_openai_delta(data) {
            Some(StreamChunk::Thinking(t)) => assert_eq!(t, "thinking..."),
            other => panic!("expected Thinking, got {other:?}"),
        }
    }
}
