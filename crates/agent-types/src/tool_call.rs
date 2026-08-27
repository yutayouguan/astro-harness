//! 原生 function calling 工具调用解析。
//!
//! 流式 `delta.tool_calls` 片段由 [`ToolCallAccumulator`] 累积。自由文本不参与
//! 工具识别，与 Codex 的结构化响应边界保持一致。

use std::collections::BTreeMap;

use serde_json::Value;

/// 解析完成的单次工具调用。
#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub args_parse_error: bool,
    pub signature: Option<String>,
}

impl ParsedToolCall {
    pub fn new(name: impl Into<String>, arguments: Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            arguments,
            args_parse_error: false,
            signature: None,
        }
    }

    pub fn with_id(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments,
            args_parse_error: false,
            signature: None,
        }
    }
}

/// 流式原生 function calling 的单个增量片段。
#[derive(Debug, Clone, Default)]
pub struct ToolCallDelta {
    pub index: u32,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
    pub signature: Option<String>,
}

/// 累积流式 tool_call delta 片段。
#[derive(Debug, Default)]
pub struct ToolCallAccumulator {
    slots: BTreeMap<u32, AccSlot>,
}

#[derive(Debug, Default)]
struct AccSlot {
    id: String,
    name: String,
    arguments: String,
    signature: Option<String>,
}

impl ToolCallAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, delta: &ToolCallDelta) {
        let slot = self.slots.entry(delta.index).or_default();
        if let Some(id) = delta.id.as_deref().filter(|s| !s.is_empty()) {
            slot.id = id.to_string();
        }
        if let Some(name) = delta.name.as_deref().filter(|s| !s.is_empty()) {
            slot.name.push_str(name);
        }
        if let Some(sig) = delta.signature.as_deref().filter(|s| !s.is_empty()) {
            slot.signature = Some(sig.to_string());
        }
        if let Some(args) = delta.arguments.as_deref() {
            if looks_like_complete_json(args)
                && (slot.arguments.is_empty() || looks_like_complete_json(&slot.arguments))
            {
                slot.arguments = args.to_string();
            } else {
                slot.arguments.push_str(args);
            }
        }
    }

    pub fn push_many(&mut self, deltas: &[ToolCallDelta]) {
        for d in deltas {
            self.push(d);
        }
    }

    pub fn finish(self) -> Vec<ParsedToolCall> {
        self.slots
            .into_values()
            .filter(|s| !s.name.is_empty())
            .map(|s| {
                let id = if s.id.is_empty() {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    s.id
                };
                let trimmed = s.arguments.trim();
                if trimmed.is_empty() {
                    let mut call = ParsedToolCall::with_id(id, s.name, serde_json::json!({}));
                    call.signature = s.signature;
                    return call;
                }
                match serde_json::from_str::<Value>(trimmed) {
                    Ok(arguments) => {
                        let mut call = ParsedToolCall::with_id(id, s.name, arguments);
                        call.signature = s.signature;
                        call
                    }
                    Err(err) => {
                        let detail = if err.is_eof() {
                            format!(
                                "工具参数 JSON 不完整，疑似单次输出超长被截断。请将超长内容拆分为多次调用（分段写入 / 追加），或缩短本次内容后重试。原始错误: {err}"
                            )
                        } else {
                            format!("工具参数 JSON 无效（请检查引号与转义）: {err}")
                        };
                        ParsedToolCall {
                            id,
                            name: s.name,
                            arguments: serde_json::json!({
                                "_parse_error": detail,
                                "_raw": trimmed,
                            }),
                            args_parse_error: true,
                            signature: s.signature,
                        }
                    }
                }
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

fn looks_like_complete_json(s: &str) -> bool {
    let t = s.trim();
    (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_streaming_tool_calls() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&ToolCallDelta {
            index: 0,
            id: Some("call_1".into()),
            name: Some("terminal".into()),
            arguments: Some("{\"path\":".into()),
            signature: None,
        });
        acc.push(&ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            arguments: Some("\"a.txt\",\"operation\":\"read\"}".into()),
            signature: None,
        });
        let calls = acc.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "terminal");
        assert_eq!(calls[0].arguments["operation"], "read");
        assert!(!calls[0].args_parse_error);
    }

    #[test]
    fn finish_marks_invalid_json() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&ToolCallDelta {
            index: 0,
            id: Some("x".into()),
            name: Some("terminal".into()),
            arguments: Some("{\"path\":".into()),
            signature: None,
        });
        let calls = acc.finish();
        assert!(calls[0].args_parse_error);
        assert!(calls[0].arguments.get("_parse_error").is_some());
    }

    #[test]
    fn complete_json_delta_replaces_not_appends() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&ToolCallDelta {
            index: 0,
            id: Some("g".into()),
            name: Some("terminal".into()),
            arguments: Some("{\"path\":\"a\"}".into()),
            signature: None,
        });
        acc.push(&ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            arguments: Some("{\"path\":\"b\"}".into()),
            signature: None,
        });
        let calls = acc.finish();
        assert_eq!(calls[0].arguments["path"], "b");
    }

    #[test]
    fn accumulates_signature_from_delta() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&ToolCallDelta {
            index: 0,
            id: Some("fc_1".into()),
            name: Some("image_gen".into()),
            arguments: Some("{\"prompt\":\"cat\"}".into()),
            signature: Some("sig_abc".into()),
        });
        let calls = acc.finish();
        assert_eq!(calls[0].signature.as_deref(), Some("sig_abc"));
    }

    #[test]
    fn preserves_native_tool_name_exactly() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&ToolCallDelta {
            index: 0,
            id: Some("call_namespaced".into()),
            name: Some("default_api:terminal".into()),
            arguments: Some(r#"{"operation":"list"}"#.into()),
            signature: None,
        });
        let calls = acc.finish();
        assert_eq!(calls[0].name, "default_api:terminal");
    }
}
