//! 会话消息中 tool_call / tool result 配对归一化。
//!
//! Provider（OpenAI / Anthropic / Gemini）要求每个 `tool_calls[].id`
//! 都有对应 `role=tool` 且带 `tool_call_id` 的结果；悬挂调用会导致 400。

use std::collections::HashSet;
use types::message::{Message, Role};

/// Normalize native Responses tool items without changing or flattening any
/// surviving item. Missing outputs are synthesized with deterministic ids so
/// retries and cold restores produce the same request.
pub fn sanitized_response_items(
    items: &[agent_protocol::ResponseItem],
) -> Vec<agent_protocol::ResponseItem> {
    use agent_protocol::ResponseItem;

    let declared = items
        .iter()
        .filter_map(response_call_id)
        .map(str::to_string)
        .collect::<HashSet<_>>();
    let outputs = items
        .iter()
        .filter_map(response_output_call_id)
        .map(str::to_string)
        .collect::<HashSet<_>>();
    let mut normalized = Vec::with_capacity(items.len());
    for item in items {
        if let Some(call_id) = response_output_call_id(item) {
            if declared.contains(call_id) {
                normalized.push(item.clone());
            }
            continue;
        }
        normalized.push(item.clone());
        let Some(call_id) = response_call_id(item) else {
            continue;
        };
        if outputs.contains(call_id) {
            continue;
        }
        let id = Some(format!("auto_output_{call_id}"));
        let output = serde_json::Value::String("aborted".into());
        normalized.push(match item {
            ResponseItem::CustomToolCall { name, .. } => ResponseItem::CustomToolCallOutput {
                id,
                call_id: call_id.to_string(),
                name: Some(name.clone()),
                output,
                internal_chat_message_metadata_passthrough: None,
            },
            ResponseItem::ToolSearchCall { execution, .. } => ResponseItem::ToolSearchOutput {
                id,
                call_id: Some(call_id.to_string()),
                status: "completed".into(),
                execution: execution.clone(),
                tools: Vec::new(),
                internal_chat_message_metadata_passthrough: None,
            },
            ResponseItem::FunctionCall {
                name, namespace, ..
            } => ResponseItem::FunctionCallOutput {
                id,
                call_id: Some(call_id.to_string()),
                name: Some(name.clone()),
                namespace: namespace.clone(),
                output,
                internal_chat_message_metadata_passthrough: None,
            },
            _ => unreachable!("response_call_id only returns tool call variants"),
        });
    }
    normalized
}

fn response_call_id(item: &agent_protocol::ResponseItem) -> Option<&str> {
    match item {
        agent_protocol::ResponseItem::FunctionCall { call_id, .. }
        | agent_protocol::ResponseItem::CustomToolCall { call_id, .. } => {
            (!call_id.trim().is_empty()).then_some(call_id.as_str())
        }
        agent_protocol::ResponseItem::ToolSearchCall {
            call_id: Some(call_id),
            ..
        } => (!call_id.trim().is_empty()).then_some(call_id.as_str()),
        _ => None,
    }
}

fn response_output_call_id(item: &agent_protocol::ResponseItem) -> Option<&str> {
    match item {
        agent_protocol::ResponseItem::FunctionCallOutput {
            call_id: Some(call_id),
            ..
        }
        | agent_protocol::ResponseItem::ToolSearchOutput {
            call_id: Some(call_id),
            ..
        } => (!call_id.trim().is_empty()).then_some(call_id.as_str()),
        agent_protocol::ResponseItem::CustomToolCallOutput { call_id, .. } => {
            (!call_id.trim().is_empty()).then_some(call_id.as_str())
        }
        _ => None,
    }
}

/// 归一化 tool_calls / tool result 对（就地修改）。
///
/// 与 Codex 的 prompt-history 边界一致：
/// 1. 收集所有带非空 `tool_call_id` 的 tool 结果 id
/// 2. 保留合法调用；对缺失结果的调用在其 assistant 后补一条 `aborted`
/// 3. 丢弃无 `tool_call_id`、或 id 未出现在任一 assistant.tool_calls 中的 tool 消息
///
/// 该函数应作用于 prompt 快照，不改写持久化原始历史。
pub fn sanitize_tool_pairs(messages: &mut Vec<Message>) {
    let mut result_ids = HashSet::new();
    for m in messages.iter() {
        if m.role == Role::Tool {
            if let Some(id) = m
                .tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                result_ids.insert(id.to_string());
            }
        }
    }

    let mut declared_ids = HashSet::new();
    let mut missing_outputs = Vec::new();
    for (index, m) in messages.iter_mut().enumerate() {
        if m.role != Role::Assistant {
            continue;
        }
        if let Some(calls) = m.tool_calls.take() {
            let kept: Vec<_> = calls
                .into_iter()
                .filter(|call| !call.id.trim().is_empty())
                .collect();
            for c in &kept {
                declared_ids.insert(c.id.clone());
            }
            let missing = kept
                .iter()
                .filter(|call| !result_ids.contains(call.id.trim()))
                .map(|call| Message::tool_with_id(&call.id, "aborted"))
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                missing_outputs.push((index, missing));
            }
            m.tool_calls = if kept.is_empty() { None } else { Some(kept) };
        }
    }

    // 倒序插入，避免前面的位置变化影响后续 index。
    for (index, outputs) in missing_outputs.into_iter().rev() {
        messages.splice(index + 1..index + 1, outputs);
    }

    messages.retain(|m| {
        if m.role != Role::Tool {
            return true;
        }
        match m
            .tool_call_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(id) => declared_ids.contains(id),
            None => false,
        }
    });
}

/// 返回归一化后的 prompt 快照（不修改原切片）。
pub fn sanitized_tool_pairs(messages: &[Message]) -> Vec<Message> {
    let mut out = messages.to_vec();
    sanitize_tool_pairs(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use types::message::ToolCall;

    #[test]
    fn synthesizes_aborted_results_for_dangling_tool_calls() {
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
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "c1");
        assert_eq!(calls[1].id, "c2");
        let tools: Vec<_> = msgs.iter().filter(|m| m.role == Role::Tool).collect();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].tool_call_id.as_deref(), Some("c2"));
        assert_eq!(tools[0].content_str(), "aborted");
        assert_eq!(tools[1].tool_call_id.as_deref(), Some("c1"));
    }

    #[test]
    fn preserves_call_and_synthesizes_output_when_no_result_exists() {
        let original = vec![Message::assistant_with_tools(
            "thinking",
            vec![ToolCall {
                id: "c1".into(),
                name: "a".into(),
                arguments: json!({}),
                signature: None,
            }],
        )];
        let msgs = sanitized_tool_pairs(&original);
        assert_eq!(original.len(), 1, "raw history must remain unchanged");
        assert_eq!(msgs[0].tool_calls.as_ref().map(Vec::len), Some(1));
        assert_eq!(msgs[0].content_str(), "thinking");
        assert_eq!(msgs[1].role, Role::Tool);
        assert_eq!(msgs[1].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(msgs[1].content_str(), "aborted");
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

    #[test]
    fn native_items_get_stable_missing_output_without_flattening() {
        let call = agent_protocol::ResponseItem::FunctionCall {
            id: Some("item_1".into()),
            name: "lookup".into(),
            namespace: Some("mcp".into()),
            arguments: "{}".into(),
            encrypted_function_args: Some(vec!["opaque".into()]),
            call_id: "call_1".into(),
            internal_chat_message_metadata_passthrough: None,
        };
        let normalized = sanitized_response_items(std::slice::from_ref(&call));
        assert_eq!(normalized[0], call);
        assert!(matches!(
            &normalized[1],
            agent_protocol::ResponseItem::FunctionCallOutput {
                id: Some(id),
                call_id: Some(call_id),
                namespace: Some(namespace),
                output,
                ..
            } if id == "auto_output_call_1"
                && call_id == "call_1"
                && namespace == "mcp"
                && output == "aborted"
        ));
    }

    #[test]
    fn native_items_drop_orphan_outputs() {
        let orphan = agent_protocol::ResponseItem::FunctionCallOutput {
            id: Some("out_1".into()),
            call_id: Some("unknown".into()),
            name: Some("lookup".into()),
            namespace: None,
            output: serde_json::Value::String("ok".into()),
            internal_chat_message_metadata_passthrough: None,
        };
        assert!(sanitized_response_items(&[orphan]).is_empty());
    }
}
