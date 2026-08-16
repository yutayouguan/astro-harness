//! OpenAI SSE 事件解析（统一输出 StreamChunk）。

use serde_json::Value;

use crate::types::stream::{StreamChunk, Usage};

/// 解析 OpenAI SSE `data:` 负载为一组 [`StreamChunk`]。
///
/// MiniMax interleaved thinking 场景下，一个 SSE 事件可能同时包含
/// `reasoning_details` + `tool_calls` + `content` + `finish_reason`，
/// 必须全部提取而非只返回优先级最高的。
pub fn extract_openai_delta(data: &str) -> Vec<StreamChunk> {
    let Some(v) = serde_json::from_str::<Value>(data).ok() else {
        return Vec::new();
    };

    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("API error");
        return vec![StreamChunk::Error(msg.to_string())];
    }

    let usage = parse_usage(&v);
    let choice = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());

    let Some(choice) = choice else {
        return usage.map_or_else(Vec::new, |u| vec![StreamChunk::Usage(u)]);
    };

    let mut chunks = Vec::with_capacity(4);

    // reasoning（优先 reasoning_content 字符串，后备 reasoning_details 数组）
    let reasoning = choice
        .pointer("/delta/reasoning_content")
        .or_else(|| choice.pointer("/message/reasoning_content"))
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| StreamChunk::Thinking(s.to_string()))
        .or_else(|| {
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
    if let Some(r) = reasoning {
        chunks.push(r);
    }

    // text content
    let content_str = choice
        .pointer("/delta/content")
        .or_else(|| choice.pointer("/message/content"))
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty());
    let meaningful_text = content_str.is_some_and(|s| !s.trim().is_empty());

    if let (true, Some(s)) = (meaningful_text, content_str) {
        chunks.push(StreamChunk::Text(s.to_string()));
    }

    // tool_calls
    parse_tool_deltas(choice, &mut chunks);

    // 纯空白 text 兜底（仅在无其他内容时保留，避免遮蔽 tool_calls）
    if !meaningful_text {
        if let Some(s) = content_str {
            if chunks.is_empty() {
                chunks.push(StreamChunk::Text(s.to_string()));
            }
        }
    }

    // finish_reason
    if let Some(f) = choice
        .get("finish_reason")
        .and_then(|f| f.as_str())
        .filter(|s| !s.is_empty())
    {
        chunks.push(StreamChunk::Done {
            finish_reason: f.to_string(),
        });
    }

    // usage
    if let Some(u) = usage {
        chunks.push(StreamChunk::Usage(u));
    }

    chunks
}

fn parse_tool_deltas(choice: &Value, chunks: &mut Vec<StreamChunk>) {
    let Some(arr) = choice
        .pointer("/delta/tool_calls")
        .or_else(|| choice.pointer("/message/tool_calls"))
        .and_then(|v| v.as_array())
    else {
        return;
    };

    let Some(tc) = arr.first() else { return };
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
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if let Some(id) = id {
        chunks.push(StreamChunk::ToolCallStart {
            index,
            id,
            name: name.unwrap_or_default(),
        });
        // MiniMax：id 和 arguments 在同一对象，不能丢 arguments
        if let Some(args) = arguments {
            chunks.push(StreamChunk::ToolCallDelta {
                index,
                arguments: args,
            });
        }
    } else if let Some(args) = arguments {
        chunks.push(StreamChunk::ToolCallDelta {
            index,
            arguments: args,
        });
    }
}

fn parse_usage(v: &Value) -> Option<Usage> {
    super::parse_openai_usage(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_delta() {
        let data = r#"{"choices":[{"delta":{"content":"Hello"}}]}"#;
        let chunks = extract_openai_delta(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Text(t) if t == "Hello"));
    }

    #[test]
    fn usage_only() {
        let data = r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#;
        let chunks = extract_openai_delta(data);
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], StreamChunk::Usage(u) if u.input_tokens == 10 && u.output_tokens == 5)
        );
    }

    #[test]
    fn tool_call_start() {
        let data = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read","arguments":""}}]}}]}"#;
        let chunks = extract_openai_delta(data);
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], StreamChunk::ToolCallStart { id, name, .. } if id == "call_1" && name == "read")
        );
    }

    #[test]
    fn error_event() {
        let data = r#"{"error":{"message":"Rate limited"}}"#;
        let chunks = extract_openai_delta(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Error(msg) if msg == "Rate limited"));
    }

    #[test]
    fn minimax_full_delta_all_fields() {
        // MiniMax interleaved thinking: reasoning + content="\n" + tool_calls(含arguments) + finish
        let data = r#"{"choices":[{"finish_reason":"tool_calls","message":{"content":"\n","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"location\":\"SF\"}"}}],"reasoning_details":[{"type":"reasoning.text","text":"thinking..."}]}}]}"#;
        let chunks = extract_openai_delta(data);
        let types: Vec<&str> = chunks
            .iter()
            .map(|c| match c {
                StreamChunk::Thinking(_) => "Thinking",
                StreamChunk::Text(_) => "Text",
                StreamChunk::ToolCallStart { .. } => "ToolCallStart",
                StreamChunk::ToolCallDelta { .. } => "ToolCallDelta",
                StreamChunk::Done { .. } => "Done",
                StreamChunk::Usage(_) => "Usage",
                _ => "Other",
            })
            .collect();
        // Thinking + ToolCallStart + ToolCallDelta(arguments) + Done
        assert_eq!(
            types,
            vec!["Thinking", "ToolCallStart", "ToolCallDelta", "Done"],
            "should extract all fields, got {types:?}"
        );

        // 验证 arguments 正确
        if let StreamChunk::ToolCallDelta { arguments, .. } = &chunks[2] {
            assert_eq!(arguments, r#"{"location":"SF"}"#);
        } else {
            panic!("expected ToolCallDelta at index 2");
        }
    }

    #[test]
    fn reasoning_details_array_format() {
        let data = r#"{"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.text","text":"thinking..."}]}}]}"#;
        let chunks = extract_openai_delta(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Thinking(t) if t == "thinking..."));
    }

    #[test]
    fn whitespace_only_content_alone_still_emitted() {
        let data = r#"{"choices":[{"delta":{"content":"\n"}}]}"#;
        let chunks = extract_openai_delta(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Text(t) if t == "\n"));
    }

    #[test]
    fn meaningful_content_emitted() {
        let data = r#"{"choices":[{"delta":{"content":"hello"}}]}"#;
        let chunks = extract_openai_delta(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Text(t) if t == "hello"));
    }

    #[test]
    fn minimax_native_format_think_tags_in_content() {
        // 原生格式 (reasoning_split=False): <think>...</think> 嵌在 content 中 + tool_calls
        let data = r#"{"choices":[{"finish_reason":"tool_calls","message":{"content":"<think>\nI should call get_weather.\n</think>\n\n\n","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"location\":\"SF\"}"}}]}}]}"#;
        let chunks = extract_openai_delta(data);
        let types: Vec<&str> = chunks
            .iter()
            .map(|c| match c {
                StreamChunk::Text(_) => "Text",
                StreamChunk::ToolCallStart { .. } => "ToolCallStart",
                StreamChunk::ToolCallDelta { .. } => "ToolCallDelta",
                StreamChunk::Done { .. } => "Done",
                _ => "Other",
            })
            .collect();
        // content 含 <think> 标签 → Text（非空白）+ ToolCallStart + ToolCallDelta + Done
        assert_eq!(
            types,
            vec!["Text", "ToolCallStart", "ToolCallDelta", "Done"],
            "native format: got {types:?}"
        );
    }

    #[test]
    fn reasoning_content_string_with_tool_calls() {
        // DeepSeek 风格：reasoning_content 字符串 + tool_calls
        let data = r#"{"choices":[{"finish_reason":"tool_calls","delta":{"reasoning_content":"let me think","tool_calls":[{"index":0,"id":"c1","function":{"name":"f","arguments":"{}"}}]}}]}"#;
        let chunks = extract_openai_delta(data);
        let types: Vec<&str> = chunks
            .iter()
            .map(|c| match c {
                StreamChunk::Thinking(_) => "Thinking",
                StreamChunk::ToolCallStart { .. } => "ToolCallStart",
                StreamChunk::ToolCallDelta { .. } => "ToolCallDelta",
                StreamChunk::Done { .. } => "Done",
                _ => "Other",
            })
            .collect();
        assert_eq!(
            types,
            vec!["Thinking", "ToolCallStart", "ToolCallDelta", "Done"]
        );
    }
}
