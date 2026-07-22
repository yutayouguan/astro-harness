//! Anthropic Messages API SSE 事件解析。
//!
//! 支持的事件类型：
//! - `message_start`：初始 usage（input_tokens / cache tokens）
//! - `content_block_start`：text / tool_use / thinking 块开始
//! - `content_block_delta`：text_delta / input_json_delta / thinking_delta / signature_delta
//! - `message_delta`：stop_reason + output usage
//! - `error`

use serde_json::Value;

use crate::streaming::Usage;
use crate::trait_::{ChatChunk, ToolCallDeltaChunk};

/// 解析 Anthropic SSE 事件 JSON 为 [`ChatChunk`]。
pub fn extract_anthropic_delta(data: &str) -> Option<ChatChunk> {
    let v: Value = serde_json::from_str(data).ok()?;
    let event_type = v.get("type")?.as_str()?;
    match event_type {
        "message_start" => {
            let u = v.pointer("/message/usage")?;
            let usage = parse_usage(u)?;
            Some(ChatChunk {
                usage: Some(usage),
                ..Default::default()
            })
        }
        "content_block_start" => {
            let block = v.get("content_block")?;
            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
            match block_type {
                "tool_use" => Some(ChatChunk {
                    tool_call_deltas: vec![ToolCallDeltaChunk {
                        index,
                        id: block.get("id").and_then(|s| s.as_str()).map(str::to_string),
                        name: block
                            .get("name")
                            .and_then(|s| s.as_str())
                            .map(str::to_string),
                        arguments: None,
                        signature: None,
                    }],
                    ..Default::default()
                }),
                "thinking" => Some(ChatChunk {
                    thought_signature: block
                        .get("signature")
                        .and_then(|s| s.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                    ..Default::default()
                }),
                _ => None,
            }
        }
        "content_block_delta" => {
            let delta = v.get("delta")?;
            let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
            let delta_type = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match delta_type {
                "input_json_delta" => {
                    let partial = delta
                        .get("partial_json")
                        .and_then(|s| s.as_str())
                        .map(str::to_string);
                    Some(ChatChunk {
                        tool_call_deltas: vec![ToolCallDeltaChunk {
                            index,
                            id: None,
                            name: None,
                            arguments: partial,
                            signature: None,
                        }],
                        ..Default::default()
                    })
                }
                "thinking_delta" => {
                    let thinking = delta
                        .get("thinking")
                        .and_then(|t| t.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)?;
                    Some(ChatChunk {
                        reasoning: Some(thinking),
                        ..Default::default()
                    })
                }
                "signature_delta" => {
                    let sig = delta
                        .get("signature")
                        .and_then(|s| s.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)?;
                    Some(ChatChunk {
                        thought_signature: Some(sig),
                        ..Default::default()
                    })
                }
                "text_delta" | _ if delta.get("text").is_some() => {
                    let token = delta
                        .get("text")
                        .and_then(|t| t.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)?;
                    Some(ChatChunk {
                        token: Some(token),
                        ..Default::default()
                    })
                }
                _ => None,
            }
        }
        "message_delta" => {
            let finish = v
                .pointer("/delta/stop_reason")
                .and_then(|s| s.as_str())
                .map(str::to_string);
            let usage = v.get("usage").and_then(parse_usage);
            if finish.is_none() && usage.is_none() {
                return None;
            }
            Some(ChatChunk {
                finish_reason: finish,
                usage,
                ..Default::default()
            })
        }
        "error" => {
            let msg = v
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("Anthropic 错误");
            Some(ChatChunk {
                finish_reason: Some(format!("error:{msg}")),
                ..Default::default()
            })
        }
        _ => None,
    }
}

/// 从 Anthropic usage JSON 解析 [`Usage`]，支持 cache tokens。
fn parse_usage(u: &Value) -> Option<Usage> {
    let input = u
        .get("input_tokens")
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let output = u
        .get("output_tokens")
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let cache_read = u
        .get("cache_read_input_tokens")
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let cache_write = u
        .get("cache_creation_input_tokens")
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
        return None;
    }
    Some(Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: 0,
        request_count: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_start_captures_usage() {
        let data = r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-opus-4-8","stop_reason":null,"usage":{"input_tokens":100,"cache_creation_input_tokens":50,"cache_read_input_tokens":30,"output_tokens":1}}}"#;
        let chunk = extract_anthropic_delta(data).expect("message_start");
        let u = chunk.usage.expect("usage");
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 1);
        assert_eq!(u.cache_write_tokens, 50);
        assert_eq!(u.cache_read_tokens, 30);
    }

    #[test]
    fn thinking_block_start() {
        let data = r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":"sig123"}}"#;
        let chunk = extract_anthropic_delta(data).expect("thinking start");
        assert_eq!(chunk.thought_signature.as_deref(), Some("sig123"));
        assert!(chunk.token.is_none());
        assert!(chunk.reasoning.is_none());
    }

    #[test]
    fn thinking_delta() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"let me think..."}}"#;
        let chunk = extract_anthropic_delta(data).expect("thinking_delta");
        assert_eq!(chunk.reasoning.as_deref(), Some("let me think..."));
        assert!(chunk.token.is_none());
    }

    #[test]
    fn signature_delta() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"ErUB"}}"#;
        let chunk = extract_anthropic_delta(data).expect("signature_delta");
        assert_eq!(chunk.thought_signature.as_deref(), Some("ErUB"));
    }

    #[test]
    fn text_delta() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let chunk = extract_anthropic_delta(data).expect("text_delta");
        assert_eq!(chunk.token.as_deref(), Some("Hello"));
    }

    #[test]
    fn tool_use_start() {
        let data = r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_01","name":"read_file","input":{}}}"#;
        let chunk = extract_anthropic_delta(data).expect("tool_use start");
        assert_eq!(chunk.tool_call_deltas.len(), 1);
        assert_eq!(chunk.tool_call_deltas[0].id.as_deref(), Some("toolu_01"));
        assert_eq!(
            chunk.tool_call_deltas[0].name.as_deref(),
            Some("read_file")
        );
    }

    #[test]
    fn input_json_delta() {
        let data = r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"src/"}}"#;
        let chunk = extract_anthropic_delta(data).expect("input_json_delta");
        assert_eq!(chunk.tool_call_deltas.len(), 1);
        assert_eq!(
            chunk.tool_call_deltas[0].arguments.as_deref(),
            Some("{\"path\":\"src/")
        );
    }

    #[test]
    fn message_delta_with_cache_usage() {
        let data = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42,"cache_read_input_tokens":10}}"#;
        let chunk = extract_anthropic_delta(data).expect("message_delta");
        assert_eq!(chunk.finish_reason.as_deref(), Some("end_turn"));
        let u = chunk.usage.expect("usage");
        assert_eq!(u.output_tokens, 42);
        assert_eq!(u.cache_read_tokens, 10);
    }

    #[test]
    fn error_event() {
        let data = r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        let chunk = extract_anthropic_delta(data).expect("error");
        assert_eq!(
            chunk.finish_reason.as_deref(),
            Some("error:Overloaded")
        );
    }

    #[test]
    fn ping_ignored() {
        let data = r#"{"type":"ping"}"#;
        assert!(extract_anthropic_delta(data).is_none());
    }
}
