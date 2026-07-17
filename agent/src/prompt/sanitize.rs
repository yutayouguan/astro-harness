//! 会话消息中 tool_call / tool result 配对清理。
//!
//! Provider（OpenAI / Anthropic / Gemini）要求每个 `tool_calls[].id`
//! 都有对应 `role=tool` 且带 `tool_call_id` 的结果；悬挂调用会导致 400。

use common::message::{Message, Role};
use std::collections::HashSet;

/// 清理悬挂的 tool_calls / 孤儿 tool 消息（就地修改）。
///
/// 规则：
/// 1. 收集所有带非空 `tool_call_id` 的 tool 结果 id
/// 2. assistant 的 `tool_calls` 只保留有结果的 id；若全被滤空则置 `None`
/// 3. 丢弃无 `tool_call_id`、或 id 未出现在任一 assistant.tool_calls 中的 tool 消息
pub fn sanitize_tool_pairs(messages: &mut Vec<Message>) {
    let mut result_ids = HashSet::new();
    for m in messages.iter() {
        if m.role == Role::Tool {
            if let Some(id) = m.tool_call_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                result_ids.insert(id.to_string());
            }
        }
    }

    let mut declared_ids = HashSet::new();
    for m in messages.iter_mut() {
        if m.role != Role::Assistant {
            continue;
        }
        if let Some(calls) = m.tool_calls.take() {
            let kept: Vec<_> = calls
                .into_iter()
                .filter(|c| {
                    let id = c.id.trim();
                    !id.is_empty() && result_ids.contains(id)
                })
                .collect();
            for c in &kept {
                declared_ids.insert(c.id.clone());
            }
            m.tool_calls = if kept.is_empty() { None } else { Some(kept) };
        }
    }

    messages.retain(|m| {
        if m.role != Role::Tool {
            return true;
        }
        match m.tool_call_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => declared_ids.contains(id),
            None => false,
        }
    });
}

/// 返回清理后的克隆（不修改原切片）。
pub fn sanitized_tool_pairs(messages: &[Message]) -> Vec<Message> {
    let mut out = messages.to_vec();
    sanitize_tool_pairs(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::message::ToolCall;
    use serde_json::json;

    #[test]
    fn drops_dangling_tool_calls_without_results() {
        let mut msgs = vec![
            Message::user("hi"),
            Message::assistant_with_tools(
                "",
                vec![
                    ToolCall {
                        id: "c1".into(),
                        name: "a".into(),
                        arguments: json!({}),
                                            signature: None,
                    },
                    ToolCall {
                        id: "c2".into(),
                        name: "b".into(),
                        arguments: json!({}),
                                            signature: None,
                    },
                ],
            ),
            Message::tool_with_id("c1", "ok"),
        ];
        sanitize_tool_pairs(&mut msgs);
        let assistant = msgs.iter().find(|m| m.role == Role::Assistant).unwrap();
        let calls = assistant.tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "c1");
        assert_eq!(msgs.iter().filter(|m| m.role == Role::Tool).count(), 1);
    }

    #[test]
    fn clears_tool_calls_when_no_results() {
        let mut msgs = vec![Message::assistant_with_tools(
            "thinking",
            vec![ToolCall {
                id: "c1".into(),
                name: "a".into(),
                arguments: json!({}),
                            signature: None,
            }],
        )];
        sanitize_tool_pairs(&mut msgs);
        assert!(msgs[0].tool_calls.is_none());
        assert_eq!(msgs[0].content_str(), "thinking");
    }

    #[test]
    fn drops_orphan_tool_messages() {
        let mut msgs = vec![
            Message::assistant_with_tools(
                "",
                vec![ToolCall {
                    id: "c1".into(),
                    name: "a".into(),
                    arguments: json!({}),
                                    signature: None,
                }],
            ),
            Message::tool_with_id("c1", "ok"),
            Message::tool("orphan"),
            Message::tool_with_id("unknown", "x"),
        ];
        sanitize_tool_pairs(&mut msgs);
        let tools: Vec<_> = msgs.iter().filter(|m| m.role == Role::Tool).collect();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_call_id.as_deref(), Some("c1"));
    }
}
