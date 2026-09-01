//! 会话消息中 tool_call / tool result 配对归一化。
//!
//! Provider（OpenAI / Anthropic / Gemini）要求每个 `tool_calls[].id`
//! 都有对应 `role=tool` 且带 `tool_call_id` 的结果；悬挂调用会导致 400。

use std::collections::HashSet;
use uuid::Uuid;

// Keep prompt-only synthetic IDs stable across retries and cold restores.
// This is the same namespace used by Codex for missing call outputs.
const SYNTHETIC_OUTPUT_ID_NAMESPACE: Uuid = Uuid::from_u128(0x90d38d3e_6a5b_4d52_bfe2_2f1e634bfac4);

/// Normalize native Responses tool items without changing or flattening any
/// surviving item. Missing outputs are synthesized with deterministic ids so
/// retries and cold restores produce the same request.
pub fn sanitized_response_items(
    items: &[agent_protocol::ResponseItem],
) -> Vec<agent_protocol::ResponseItem> {
    use agent_protocol::ResponseItem;

    let mut function_calls = HashSet::new();
    let mut tool_search_calls = HashSet::new();
    let mut custom_tool_calls = HashSet::new();
    let mut function_outputs = HashSet::new();
    let mut tool_search_outputs = HashSet::new();
    let mut custom_tool_outputs = HashSet::new();
    for item in items {
        match item {
            ResponseItem::FunctionCall { call_id, .. }
            | ResponseItem::LocalShellCall {
                call_id: Some(call_id),
                ..
            } if !call_id.trim().is_empty() => {
                function_calls.insert(call_id.as_str());
            }
            ResponseItem::ToolSearchCall {
                call_id: Some(call_id),
                ..
            } if !call_id.trim().is_empty() => {
                tool_search_calls.insert(call_id.as_str());
            }
            ResponseItem::CustomToolCall { call_id, .. } if !call_id.trim().is_empty() => {
                custom_tool_calls.insert(call_id.as_str());
            }
            ResponseItem::FunctionCallOutput {
                call_id: Some(call_id),
                ..
            } if !call_id.trim().is_empty() => {
                function_outputs.insert(call_id.as_str());
            }
            ResponseItem::ToolSearchOutput {
                call_id: Some(call_id),
                ..
            } if !call_id.trim().is_empty() => {
                tool_search_outputs.insert(call_id.as_str());
            }
            ResponseItem::CustomToolCallOutput { call_id, .. } if !call_id.trim().is_empty() => {
                custom_tool_outputs.insert(call_id.as_str());
            }
            _ => {}
        }
    }

    let mut normalized = Vec::with_capacity(items.len());
    for item in items {
        let orphan = match item {
            ResponseItem::FunctionCallOutput {
                call_id: Some(call_id),
                ..
            } => !function_calls.contains(call_id.as_str()),
            ResponseItem::CustomToolCallOutput { call_id, .. } => {
                !custom_tool_calls.contains(call_id.as_str())
            }
            ResponseItem::ToolSearchOutput {
                call_id: Some(call_id),
                execution,
                ..
            } => execution != "server" && !tool_search_calls.contains(call_id.as_str()),
            _ => false,
        };
        if orphan {
            continue;
        }

        normalized.push(item.clone());

        let missing_output = match item {
            ResponseItem::FunctionCall { id, call_id, .. }
                if !function_outputs.contains(call_id.as_str()) =>
            {
                Some(ResponseItem::FunctionCallOutput {
                    id: synthetic_output_id("fco", id.as_ref()),
                    call_id: Some(call_id.clone()),
                    name: None,
                    namespace: None,
                    output: agent_protocol::FunctionCallOutputPayload::from_text("aborted".into()),
                    internal_chat_message_metadata_passthrough: None,
                })
            }
            ResponseItem::LocalShellCall {
                id,
                call_id: Some(call_id),
                ..
            } if !function_outputs.contains(call_id.as_str()) => {
                Some(ResponseItem::FunctionCallOutput {
                    id: synthetic_output_id("fco", id.as_ref()),
                    call_id: Some(call_id.clone()),
                    name: None,
                    namespace: None,
                    output: agent_protocol::FunctionCallOutputPayload::from_text("aborted".into()),
                    internal_chat_message_metadata_passthrough: None,
                })
            }
            ResponseItem::CustomToolCall { id, call_id, .. }
                if !custom_tool_outputs.contains(call_id.as_str()) =>
            {
                Some(ResponseItem::CustomToolCallOutput {
                    id: synthetic_output_id("ctco", id.as_ref()),
                    call_id: call_id.clone(),
                    name: None,
                    output: agent_protocol::FunctionCallOutputPayload::from_text("aborted".into()),
                    internal_chat_message_metadata_passthrough: None,
                })
            }
            ResponseItem::ToolSearchCall {
                id,
                call_id: Some(call_id),
                ..
            } if !tool_search_outputs.contains(call_id.as_str()) => {
                Some(ResponseItem::ToolSearchOutput {
                    id: synthetic_output_id("tso", id.as_ref()),
                    call_id: Some(call_id.clone()),
                    status: "completed".into(),
                    execution: "client".into(),
                    tools: Vec::new(),
                    internal_chat_message_metadata_passthrough: None,
                })
            }
            _ => None,
        };
        if let Some(output) = missing_output {
            normalized.push(output);
        }
    }
    normalized
}

fn synthetic_output_id(
    prefix: &str,
    item_id: Option<&agent_protocol::ResponseItemId>,
) -> Option<agent_protocol::ResponseItemId> {
    let source_id = item_id.filter(|id| !id.is_empty())?;
    let name = format!("{prefix}:{source_id}");
    Some(agent_protocol::ResponseItemId::with_suffix(
        prefix,
        Uuid::new_v5(&SYNTHETIC_OUTPUT_ID_NAMESPACE, name.as_bytes()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let normalized_again = sanitized_response_items(std::slice::from_ref(&call));
        assert_eq!(normalized[0], call);
        assert_eq!(normalized, normalized_again);
        assert!(matches!(
            &normalized[1],
            agent_protocol::ResponseItem::FunctionCallOutput {
                id: Some(id),
                call_id: Some(call_id),
                namespace: None,
                output,
                ..
            } if id.as_str().starts_with("fco_")
                && call_id == "call_1"
                && output.text_content() == Some("aborted")
        ));
    }

    #[test]
    fn native_local_shell_call_gets_function_output() {
        let call = agent_protocol::ResponseItem::LocalShellCall {
            id: Some("item_shell".into()),
            call_id: Some("call_shell".into()),
            status: serde_json::json!("completed"),
            action: serde_json::json!({"command": "pwd"}),
            internal_chat_message_metadata_passthrough: None,
        };
        let normalized = sanitized_response_items(&[call]);
        assert!(matches!(
            &normalized[1],
            agent_protocol::ResponseItem::FunctionCallOutput {
                id: Some(id),
                call_id: Some(call_id),
                output,
                ..
            } if id.as_str().starts_with("fco_")
                && call_id == "call_shell"
                && output.text_content() == Some("aborted")
        ));
    }

    #[test]
    fn native_server_tool_search_output_may_stand_alone() {
        let output = agent_protocol::ResponseItem::ToolSearchOutput {
            id: Some("tso_server".into()),
            call_id: Some("server_call".into()),
            status: "completed".into(),
            execution: "server".into(),
            tools: Vec::new(),
            internal_chat_message_metadata_passthrough: None,
        };
        assert_eq!(
            sanitized_response_items(std::slice::from_ref(&output)),
            vec![output]
        );
    }

    #[test]
    fn native_items_drop_orphan_outputs() {
        let orphan = agent_protocol::ResponseItem::FunctionCallOutput {
            id: Some("out_1".into()),
            call_id: Some("unknown".into()),
            name: Some("lookup".into()),
            namespace: None,
            output: agent_protocol::FunctionCallOutputPayload::from_text("ok".into()),
            internal_chat_message_metadata_passthrough: None,
        };
        assert!(sanitized_response_items(&[orphan]).is_empty());
    }
}
