//! 序列化事件协议的稳定有界标识。
//!
//! Provider/session 原始标识符仍为模型历史、工具结果关联和内部精确轮次路由的权威来源。
//! 此处仅对事件侧的副本进行有界截断，防止恶意标识符突破持久化/实时事件载荷上限。

use std::fmt::Write;

use agent_protocol::{EventMsg, TextItem, TurnItem};
use sha2::{Digest, Sha256};

pub(crate) const MAX_EVENT_IDENTITY_BYTES: usize = 4 * 1024;
const RESERVED_EVENT_IDENTITY_PREFIX: &str = "\u{001f}astro:event-identity:v1:";

fn bounded_identity(kind: &str, raw: &str) -> String {
    if raw.len() <= MAX_EVENT_IDENTITY_BYTES && !raw.starts_with(RESERVED_EVENT_IDENTITY_PREFIX) {
        return raw.to_string();
    }

    let domain = if raw.starts_with(RESERVED_EVENT_IDENTITY_PREFIX) {
        "raw_reserved"
    } else {
        "oversized"
    };
    let digest = Sha256::digest(raw.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    format!(
        "{RESERVED_EVENT_IDENTITY_PREFIX}{kind}:{domain}:sha256:{hex}:bytes:{}",
        raw.len()
    )
}

pub(crate) fn event_turn_id(raw: &str) -> String {
    bounded_identity("turn", raw)
}

pub(crate) fn event_item_id(raw: &str) -> String {
    bounded_identity("item", raw)
}

pub(crate) fn event_request_id(raw: &str) -> String {
    bounded_identity("request", raw)
}

pub(crate) fn event_tool_name(raw: &str) -> String {
    bounded_identity("tool", raw)
}

fn normalize_text_item(item: &mut TextItem) {
    item.id = event_item_id(&item.id);
}

fn normalize_turn_item(item: &mut TurnItem) {
    match item {
        TurnItem::UserMessage(item)
        | TurnItem::HookPrompt(item)
        | TurnItem::Plan(item)
        | TurnItem::Reasoning(item)
        | TurnItem::SubAgentActivity(item)
        | TurnItem::ContextCompaction(item)
        | TurnItem::EnteredReviewMode(item)
        | TurnItem::ExitedReviewMode(item) => normalize_text_item(item),
        TurnItem::AgentMessage(item) => item.id = event_item_id(&item.id),
        TurnItem::CommandExecution(item)
        | TurnItem::DynamicToolCall(item)
        | TurnItem::McpToolCall(item)
        | TurnItem::CollabAgentToolCall(item)
        | TurnItem::WebSearch(item)
        | TurnItem::ImageView(item)
        | TurnItem::ImageGeneration(item)
        | TurnItem::FileChange(item) => {
            item.id = event_item_id(&item.id);
            item.name = event_tool_name(&item.name);
        }
        TurnItem::Extension(item) => item.id = event_item_id(&item.id),
    }
}

/// 在持久化/实时分发前规范化所有类型化的关联字段。
/// 返回外层 [`agent_protocol::Event`] 使用的事件侧 turn id。
pub(crate) fn normalize_event_msg(msg: &mut EventMsg, raw_turn_id: &str) -> String {
    let turn_id = event_turn_id(raw_turn_id);
    match msg {
        EventMsg::TurnStarted(event) => event.turn_id.clone_from(&turn_id),
        EventMsg::UserInputCommitted(event) => event.turn_id.clone_from(&turn_id),
        EventMsg::ItemStarted(event)
        | EventMsg::ItemCompleted(event)
        | EventMsg::McpToolCallBegin(event)
        | EventMsg::McpToolCallEnd(event)
        | EventMsg::HookStarted(event)
        | EventMsg::HookCompleted(event)
        | EventMsg::SubAgentActivity(event)
        | EventMsg::ContextCompacted(event) => {
            event.turn_id.clone_from(&turn_id);
            normalize_turn_item(&mut event.item);
        }
        EventMsg::AgentMessageContentDelta(event)
        | EventMsg::PlanDelta(event)
        | EventMsg::ReasoningContentDelta(event)
        | EventMsg::ExecCommandOutputDelta(event)
        | EventMsg::PatchApplyUpdated(event) => {
            event.turn_id.clone_from(&turn_id);
            event.item_id = event_item_id(&event.item_id);
        }
        EventMsg::ExecApprovalRequest(event)
        | EventMsg::ApplyPatchApprovalRequest(event)
        | EventMsg::RequestPermissions(event)
        | EventMsg::RequestUserInput(event)
        | EventMsg::ElicitationRequest(event)
        | EventMsg::DynamicToolCallRequest(event)
        | EventMsg::DynamicToolCallResponse(event) => {
            event.turn_id.clone_from(&turn_id);
            event.item_id = event_item_id(&event.item_id);
            event.request_id = event_request_id(&event.request_id);
            if let Some(name) = event
                .payload
                .get("name")
                .and_then(serde_json::Value::as_str)
            {
                event.payload["name"] = serde_json::Value::String(event_tool_name(name));
            }
        }
        EventMsg::ContextUsage(event) => event.turn_id.clone_from(&turn_id),
        EventMsg::TokenCount(event) => {
            if event.turn_id.is_some() {
                event.turn_id = Some(turn_id.clone());
            }
        }
        EventMsg::TurnComplete(event) => event.turn_id.clone_from(&turn_id),
        EventMsg::TurnAborted(event) => {
            if event.turn_id.is_some() {
                event.turn_id = Some(turn_id.clone());
            }
        }
        EventMsg::ThreadSettingsApplied(_)
        | EventMsg::ThreadRolledBack(_)
        | EventMsg::Error(_)
        | EventMsg::Warning(_)
        | EventMsg::StreamError(_)
        | EventMsg::ShutdownComplete => {}
    }
    turn_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{ControlRequestEvent, ItemEvent, ToolItem, ToolStatus};

    type IdentityProjector = fn(&str) -> String;

    #[test]
    fn normal_identity_is_exact_and_oversized_identity_is_stable() {
        assert_eq!(event_item_id("provider-call-1"), "provider-call-1");
        let raw = "oversized".repeat(MAX_EVENT_IDENTITY_BYTES);
        let first = event_item_id(&raw);
        let second = event_item_id(&raw);
        assert_eq!(first, second);
        assert!(first.len() < MAX_EVENT_IDENTITY_BYTES);
        assert_ne!(first, event_item_id(&(raw + "different")));
    }

    #[test]
    fn projected_markers_cannot_be_spoofed_by_raw_aliases_for_any_identity_kind() {
        let oversized = "oversized-identity".repeat(MAX_EVENT_IDENTITY_BYTES);
        let cases: [(&str, IdentityProjector); 4] = [
            ("turn", event_turn_id),
            ("item", event_item_id),
            ("request", event_request_id),
            ("tool", event_tool_name),
        ];

        for (kind, project) in cases {
            let marker = project(&oversized);
            let raw_marker_alias = project(&marker);
            assert_ne!(
                marker, raw_marker_alias,
                "raw {kind} marker alias must occupy a separate domain"
            );
            assert_ne!(
                raw_marker_alias,
                project(&raw_marker_alias),
                "marker-of-marker must be escaped as raw input"
            );
        }
    }

    fn normalized_tool_lifecycle(
        raw_turn: &str,
        raw_item: &str,
        raw_name: &str,
    ) -> (String, String, String) {
        let mut request = EventMsg::DynamicToolCallRequest(ControlRequestEvent {
            turn_id: raw_turn.into(),
            item_id: raw_item.into(),
            request_id: format!("{raw_turn}:{raw_item}:arguments"),
            payload: serde_json::json!({"name": raw_name}),
        });
        let tool_item = || {
            TurnItem::DynamicToolCall(ToolItem {
                id: raw_item.into(),
                name: raw_name.into(),
                arguments: serde_json::json!({}),
                output: None,
                media: Vec::new(),
                status: ToolStatus::InProgress,
                batch_id: None,
                execution_mode: None,
            })
        };
        let mut started = EventMsg::ItemStarted(ItemEvent {
            turn_id: raw_turn.into(),
            item: tool_item(),
        });
        let mut completed = EventMsg::ItemCompleted(ItemEvent {
            turn_id: raw_turn.into(),
            item: tool_item(),
        });
        normalize_event_msg(&mut request, raw_turn);
        normalize_event_msg(&mut started, raw_turn);
        normalize_event_msg(&mut completed, raw_turn);

        let EventMsg::DynamicToolCallRequest(request) = request else {
            unreachable!()
        };
        let EventMsg::ItemStarted(ItemEvent {
            item: TurnItem::DynamicToolCall(started),
            ..
        }) = started
        else {
            unreachable!()
        };
        let EventMsg::ItemCompleted(ItemEvent {
            item: TurnItem::DynamicToolCall(completed),
            ..
        }) = completed
        else {
            unreachable!()
        };
        assert_eq!(request.item_id, started.id);
        assert_eq!(started.id, completed.id);
        assert_eq!(
            request.payload["name"].as_str(),
            Some(started.name.as_str())
        );
        assert_eq!(started.name, completed.name);
        (request.item_id, started.name, request.request_id)
    }

    #[test]
    fn oversized_tool_and_raw_marker_alias_keep_distinct_lifecycles() {
        let raw_turn = "turn-1";
        let oversized_item = "oversized-call".repeat(MAX_EVENT_IDENTITY_BYTES);
        let oversized_name = "oversized-tool".repeat(MAX_EVENT_IDENTITY_BYTES);
        let marker_item = event_item_id(&oversized_item);
        let marker_name = event_tool_name(&oversized_name);

        let oversized = normalized_tool_lifecycle(raw_turn, &oversized_item, &oversized_name);
        let alias = normalized_tool_lifecycle(raw_turn, &marker_item, &marker_name);
        assert_ne!(oversized.0, alias.0);
        assert_ne!(oversized.1, alias.1);
        assert_ne!(oversized.2, alias.2);
    }

    #[test]
    fn oversized_tool_identity_is_consistent_across_request_start_and_complete() {
        let raw_turn = "turn".repeat(MAX_EVENT_IDENTITY_BYTES);
        let raw_item = "call".repeat(MAX_EVENT_IDENTITY_BYTES);
        let raw_name = "tool".repeat(MAX_EVENT_IDENTITY_BYTES);
        let tool_item = || {
            TurnItem::DynamicToolCall(ToolItem {
                id: raw_item.clone(),
                name: raw_name.clone(),
                arguments: serde_json::json!({}),
                output: None,
                media: Vec::new(),
                status: ToolStatus::InProgress,
                batch_id: None,
                execution_mode: None,
            })
        };
        let mut request = EventMsg::DynamicToolCallRequest(ControlRequestEvent {
            turn_id: raw_turn.clone(),
            item_id: raw_item.clone(),
            request_id: format!("{raw_turn}:{raw_item}:arguments"),
            payload: serde_json::json!({"name": raw_name}),
        });
        let mut started = EventMsg::ItemStarted(ItemEvent {
            turn_id: raw_turn.clone(),
            item: tool_item(),
        });
        let mut completed = EventMsg::ItemCompleted(ItemEvent {
            turn_id: raw_turn.clone(),
            item: tool_item(),
        });

        let request_turn = normalize_event_msg(&mut request, &raw_turn);
        let started_turn = normalize_event_msg(&mut started, &raw_turn);
        let completed_turn = normalize_event_msg(&mut completed, &raw_turn);
        assert_eq!(request_turn, started_turn);
        assert_eq!(started_turn, completed_turn);

        let EventMsg::DynamicToolCallRequest(request) = request else {
            unreachable!()
        };
        let EventMsg::ItemStarted(ItemEvent {
            item: TurnItem::DynamicToolCall(started),
            ..
        }) = started
        else {
            unreachable!()
        };
        let EventMsg::ItemCompleted(ItemEvent {
            item: TurnItem::DynamicToolCall(completed),
            ..
        }) = completed
        else {
            unreachable!()
        };
        assert_eq!(request.item_id, started.id);
        assert_eq!(started.id, completed.id);
        assert_eq!(
            request.payload["name"].as_str(),
            Some(started.name.as_str())
        );
        assert_eq!(started.name, completed.name);
        assert!(request.request_id.len() <= MAX_EVENT_IDENTITY_BYTES);
    }
}
