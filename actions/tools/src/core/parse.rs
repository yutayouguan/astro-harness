//! 工具调用解析：从 LLM 回复中提取结构化 tool call。
//!
//! 支持两种协议：
//! - **原生 function calling**：流式 `delta.tool_calls` 片段由 [`ToolCallAccumulator`] 累积。
//! - **XML 兼容协议**：`<tool_call>{"name":...,"arguments":...}</tool_call>` 由 [`extract_tool_calls`] 解析。
//!
//! [`resolve_tool_calls`] 合并两者，原生结果优先于 XML 回退。

use std::collections::BTreeMap;

use serde_json::Value;

/// 解析完成的单次工具调用，可直接交给 [`crate::dispatch::dispatch_tool`]。
#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    /// 调用唯一 id；流式场景来自 Provider，XML 协议则自动生成 UUID。
    pub id: String,
    /// 工具名称，与 [`crate::registry::ToolEntry::name`] 对齐。
    pub name: String,
    /// 已解析的 JSON 参数对象。
    pub arguments: Value,
    /// `arguments` JSON 解析失败时为 `true`（`arguments` 内含 `_parse_error` 与 `_raw`）。
    pub args_parse_error: bool,
    /// Google Interactions：`function_call.signature`；无则 `None`。
    pub signature: Option<String>,
}

impl ParsedToolCall {
    /// 构造无显式 id 的调用（自动生成 UUID v4）。
    pub fn new(name: impl Into<String>, arguments: Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            arguments,
            args_parse_error: false,
            signature: None,
        }
    }

    /// 构造带 Provider 下发 id 的调用（流式 function calling 场景）。
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

/// 从助手纯文本回复中提取 `<tool_call>...</tool_call>` 块。
///
/// 兼容旧版 XML 协议；每个块内须为合法 JSON，且包含 `name` 字段。
/// `arguments` 缺失时默认为空对象 `{}`。
pub fn extract_tool_calls(text: &str) -> Vec<ParsedToolCall> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<tool_call>") {
        let after = &rest[start + "<tool_call>".len()..];
        let Some(end) = after.find("</tool_call>") else {
            break;
        };
        let json_str = after[..end].trim();
        if let Ok(v) = serde_json::from_str::<Value>(json_str) {
            if let Some(name) = v.get("name").and_then(|n| n.as_str()) {
                let arguments = v
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                out.push(ParsedToolCall::new(name, arguments));
            }
        }
        rest = &after[end + "</tool_call>".len()..];
    }
    out
}

/// 流式原生 function calling 的单个增量片段（OpenAI `delta.tool_calls` 风格）。
#[derive(Debug, Clone, Default)]
pub struct ToolCallDelta {
    /// 同一轮回复中多个并行调用的槽位索引。
    pub index: u32,
    /// 调用 id 片段；仅在首个片段中出现。
    pub id: Option<String>,
    /// 工具名片段；可能分多次下发，需拼接。
    pub name: Option<String>,
    /// 参数 JSON 字符串片段；可能分多次下发，需拼接或覆盖。
    pub arguments: Option<String>,
    /// Google Interactions：`function_call.signature`（通常随首包下发）。
    pub signature: Option<String>,
}

/// 累积 OpenAI 风格 `delta.tool_calls` 片段，直至流结束调用 [`finish`](Self::finish)。
#[derive(Debug, Default)]
pub struct ToolCallAccumulator {
    /// 按 `index` 分槽存储各并行调用的累积状态。
    slots: BTreeMap<u32, AccSlot>,
}

/// 单个槽位的内部累积状态。
#[derive(Debug, Default)]
struct AccSlot {
    /// 调用 id。
    id: String,
    /// 已拼接的工具名。
    name: String,
    /// 已拼接或覆盖的参数 JSON 字符串。
    arguments: String,
    /// Google Interactions signature。
    signature: Option<String>,
}

impl ToolCallAccumulator {
    /// 创建空的累积器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 合并一个增量片段到对应 `index` 槽位。
    ///
    /// Google / Ollama 等 Provider 常一次性下发完整 JSON；若新片段形如完整 JSON，
    /// 且当前槽已有完整 JSON，则覆盖而非追加，避免重复拼接。
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
            // Google / Ollama 常下发完整 JSON；若当前槽已是完整 JSON 且新片段也是，则覆盖
            if looks_like_complete_json(args)
                && (slot.arguments.is_empty() || looks_like_complete_json(&slot.arguments))
            {
                slot.arguments = args.to_string();
            } else {
                slot.arguments.push_str(args);
            }
        }
    }

    /// 批量合并多个增量片段。
    pub fn push_many(&mut self, deltas: &[ToolCallDelta]) {
        for d in deltas {
            self.push(d);
        }
    }

    /// 将累积结果转为 [`ParsedToolCall`] 列表。
    ///
    /// - 名称为空的槽位被丢弃。
    /// - id 为空时自动生成 UUID。
    /// - 参数 JSON 解析失败时设置 `args_parse_error = true` 并保留 `_parse_error`、`_raw`。
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
                        // EOF 通常意味着参数 JSON 被截断（模型单次输出超长），
                        // 给出可操作提示，便于循环把结果回喂后让模型拆分重试。
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

    /// 是否尚未收到任何增量片段。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

/// 判断字符串是否形如完整的 JSON 对象或数组（首尾括号匹配）。
///
/// 用于区分「流式片段」与「一次性完整 JSON」，以决定覆盖还是追加参数。
fn looks_like_complete_json(s: &str) -> bool {
    let t = s.trim();
    (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'))
}

/// 合并原生 tool_calls 与 XML `<tool_call>` 解析结果。
///
/// 原生结果非空时直接返回，忽略助手文本中的 XML 块；否则回退到 [`extract_tool_calls`]。
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
    fn truncated_args_hint_mentions_split() {
        // 模拟超长 HTML 被截断：字符串未闭合、对象未收尾。
        let mut acc = ToolCallAccumulator::new();
        acc.push(&ToolCallDelta {
            index: 0,
            id: Some("t".into()),
            name: Some("file_ops".into()),
            arguments: Some("{\"operation\":\"write\",\"content\":\"<div>very long".into()),
            signature: None,
        });
        let calls = acc.finish();
        assert!(calls[0].args_parse_error);
        let msg = calls[0].arguments["_parse_error"].as_str().unwrap();
        assert!(msg.contains("截断"), "should hint truncation: {msg}");
        assert!(calls[0].arguments.get("_raw").is_some());
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
        assert!(calls[0].signature.is_none());
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
}
