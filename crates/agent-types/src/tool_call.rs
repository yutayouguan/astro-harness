//! 工具调用解析：从 LLM 回复中提取结构化 tool call。
//!
//! 原生 function calling 的流式 `delta.tool_calls` 片段由 [`ToolCallAccumulator`] 累积。
//! 对仍返回旧 XML 工具块的模型，兼容层会用 [`LegacyToolCallTextStream`] 将其从
//! 用户可见正文中隔离，再由 [`extract_tool_calls`] 解析；原生结果始终优先。

use std::collections::BTreeMap;

use serde_json::Value;

const XML_TOOL_CALL_OPEN: &str = "<tool_call>";
const XML_TOOL_CALL_CLOSE: &str = "</tool_call>";

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
            name: normalize_model_tool_name(name.into()),
            arguments,
            args_parse_error: false,
            signature: None,
        }
    }

    pub fn with_id(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            id: id.into(),
            name: normalize_model_tool_name(name.into()),
            arguments,
            args_parse_error: false,
            signature: None,
        }
    }
}

/// 去掉部分模型/兼容层为默认工具命名空间附加的展示前缀。
///
/// Astro 实际注册的是 `file_ops` 这类 canonical 名称；只接受已知的
/// `default_api:` 包装，避免把任意命名空间误映射成可执行工具。StepContext
/// 仍会在归一化后校验该工具是否确实向本次模型调用公开。
fn normalize_model_tool_name(name: String) -> String {
    name.strip_prefix("default_api:")
        .filter(|canonical| !canonical.is_empty())
        .map(str::to_owned)
        .unwrap_or(name)
}

fn parse_xml_tool_call_payload(payload: &str) -> Option<ParsedToolCall> {
    let value = serde_json::from_str::<Value>(payload.trim()).ok()?;
    let name = value.get("name")?.as_str()?;
    let arguments = value
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Some(ParsedToolCall::new(name, arguments))
}

/// 从助手纯文本回复中提取旧协议 `<tool_call>...</tool_call>` 块。
pub fn extract_tool_calls(text: &str) -> Vec<ParsedToolCall> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(XML_TOOL_CALL_OPEN) {
        let after = &rest[start + XML_TOOL_CALL_OPEN.len()..];
        let Some(end) = after.find(XML_TOOL_CALL_CLOSE) else {
            break;
        };
        if let Some(call) = parse_xml_tool_call_payload(&after[..end]) {
            out.push(call);
        }
        rest = &after[end + XML_TOOL_CALL_CLOSE.len()..];
    }
    out
}

/// 流式隔离旧 XML 工具块，只把普通助手文本交给 UI 和持久化层。
///
/// 完整且可解析的工具块会被隐藏；无效或未闭合的标记仍按普通文本返回，避免吞掉
/// 模型的非工具输出。解析后的调用继续由 [`extract_tool_calls`] 统一生成。
#[derive(Debug, Default)]
pub struct LegacyToolCallTextStream {
    pending: String,
    in_tool_call: bool,
}

impl LegacyToolCallTextStream {
    pub fn new() -> Self {
        Self::default()
    }

    /// 推入一个文本增量，返回当前已确认可展示的正文。
    pub fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        let mut visible = String::new();

        loop {
            if self.in_tool_call {
                let Some(close_start) = self.pending.find(XML_TOOL_CALL_CLOSE) else {
                    break;
                };
                let block_end = close_start + XML_TOOL_CALL_CLOSE.len();
                let remainder = self.pending.split_off(block_end);
                let block = std::mem::replace(&mut self.pending, remainder);
                let payload = &block[XML_TOOL_CALL_OPEN.len()..close_start];
                if parse_xml_tool_call_payload(payload).is_none() {
                    visible.push_str(&block);
                }
                self.in_tool_call = false;
                continue;
            }

            if let Some(open_start) = self.pending.find(XML_TOOL_CALL_OPEN) {
                let remainder = self.pending.split_off(open_start);
                visible.push_str(&self.pending);
                self.pending = remainder;
                self.in_tool_call = true;
                continue;
            }

            let retained = longest_suffix_matching_prefix(&self.pending, XML_TOOL_CALL_OPEN);
            let split_at = self.pending.len() - retained;
            let suffix = self.pending.split_off(split_at);
            visible.push_str(&self.pending);
            self.pending = suffix;
            break;
        }

        visible
    }

    /// 结束流；未形成有效完整工具块的尾部按普通正文返回。
    pub fn finish(mut self) -> String {
        std::mem::take(&mut self.pending)
    }
}

fn longest_suffix_matching_prefix(text: &str, prefix: &str) -> usize {
    let max_len = text.len().min(prefix.len().saturating_sub(1));
    (1..=max_len)
        .rev()
        .find(|len| {
            let start = text.len() - len;
            text.is_char_boundary(start) && prefix.starts_with(&text[start..])
        })
        .unwrap_or(0)
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
                            name: normalize_model_tool_name(s.name),
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

/// 合并原生 tool_calls 与旧 XML 解析结果。原生非空时忽略 XML。
pub fn resolve_tool_calls(
    native: Vec<ParsedToolCall>,
    assistant_text: &str,
) -> Vec<ParsedToolCall> {
    if !native.is_empty() {
        return native;
    }
    extract_tool_calls(assistant_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_xml_tool_call() {
        let text = r#"
before
<tool_call>{"name":"file_ops","arguments":{"path":"a.txt","operation":"read"}}</tool_call>
"#;
        let calls = extract_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "file_ops");
        assert_eq!(calls[0].arguments["path"], "a.txt");
    }

    #[test]
    fn legacy_xml_stream_hides_valid_tool_calls_across_chunks() {
        let mut stream = LegacyToolCallTextStream::new();
        assert_eq!(stream.push("before <tool_"), "before ");
        assert_eq!(
            stream
                .push("call>{\"name\":\"file_ops\",\"arguments\":{\"operation\":\"read\"}}</tool"),
            ""
        );
        assert_eq!(stream.push("_call> after"), " after");
        assert_eq!(stream.finish(), "");
    }

    #[test]
    fn legacy_xml_stream_preserves_invalid_or_unclosed_markup() {
        let mut invalid = LegacyToolCallTextStream::new();
        assert_eq!(
            invalid.push("a<tool_call>not-json</tool_call>b"),
            "a<tool_call>not-json</tool_call>b"
        );
        assert_eq!(invalid.finish(), "");

        let mut unclosed = LegacyToolCallTextStream::new();
        assert_eq!(unclosed.push("a<tool_call>{\"name\":\"file_ops\"}"), "a");
        assert_eq!(unclosed.finish(), "<tool_call>{\"name\":\"file_ops\"}");
    }

    #[test]
    fn legacy_xml_stream_does_not_hold_unrelated_angle_brackets() {
        let mut stream = LegacyToolCallTextStream::new();
        assert_eq!(
            stream.push("Use <section> and 1 < 2"),
            "Use <section> and 1 < 2"
        );
        assert_eq!(stream.finish(), "");
    }

    #[test]
    fn accumulates_streaming_tool_calls() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&ToolCallDelta {
            index: 0,
            id: Some("call_1".into()),
            name: Some("file_ops".into()),
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
        assert_eq!(calls[0].name, "file_ops");
        assert_eq!(calls[0].arguments["operation"], "read");
        assert!(!calls[0].args_parse_error);
    }

    #[test]
    fn finish_marks_invalid_json() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&ToolCallDelta {
            index: 0,
            id: Some("x".into()),
            name: Some("file_ops".into()),
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
            name: Some("file_ops".into()),
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
    fn normalizes_default_api_namespace_from_xml_tool_call() {
        let calls = extract_tool_calls(
            r#"<tool_call>{"name":"default_api:file_ops","arguments":{"operation":"list"}}</tool_call>"#,
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "file_ops");
    }

    #[test]
    fn normalizes_default_api_namespace_from_native_tool_call() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&ToolCallDelta {
            index: 0,
            id: Some("call_namespaced".into()),
            name: Some("default_api:file_ops".into()),
            arguments: Some(r#"{"operation":"list"}"#.into()),
            signature: None,
        });
        let calls = acc.finish();
        assert_eq!(calls[0].name, "file_ops");
    }

    #[test]
    fn preserves_unknown_or_empty_namespaces() {
        assert_eq!(
            ParsedToolCall::new("other_api:file_ops", serde_json::json!({})).name,
            "other_api:file_ops"
        );
        assert_eq!(
            ParsedToolCall::new("default_api:", serde_json::json!({})).name,
            "default_api:"
        );
    }
}
