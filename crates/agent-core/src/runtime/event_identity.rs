//! Stable bounded identities for the serialized event protocol.
//!
//! Provider/session raw identifiers remain authoritative for model history,
//! tool result correlation, and internal exact-turn routing. Only event-facing
//! copies are bounded here, preventing adversarial identifiers from defeating
//! the durable/live event payload cap.

use std::fmt::Write;

use agent_protocol::{EventMsg, TextItem, TurnItem};
use sha2::{Digest, Sha256};

pub(crate) const MAX_EVENT_IDENTITY_BYTES: usize = 4 * 1024;

fn bounded_identity(kind: &str, raw: &str) -> String {
    if raw.len() <= MAX_EVENT_IDENTITY_BYTES {
        return raw.to_string();
    }

    let digest = Sha256::digest(raw.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    format!("{kind}:sha256:{hex}:bytes:{}", raw.len())
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
        | TurnItem::AgentMessage(item)
        | TurnItem::Plan(item)
        | TurnItem::Reasoning(item)
        | TurnItem::SubAgentActivity(item)
        | TurnItem::ContextCompaction(item)
        | TurnItem::EnteredReviewMode(item)
        | TurnItem::ExitedReviewMode(item) => normalize_text_item(item),
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

/// Normalize all typed correlation fields before persistence/live delivery.
/// Returns the event-facing turn id used by the outer [`agent_protocol::Event`].
pub(crate) fn normalize_event_msg(msg: &mut EventMsg, raw_turn_id: &str) -> String {
    let turn_id = event_turn_id(raw_turn_id);
    match msg {
        EventMsg::TurnStarted(event) => event.turn_id.clone_from(&turn_id),
        EventMsg::ItemStarted(event)
        | EventMsg::ItemCompleted(event)
        | EventMsg::McpToolCallBegin(event)
        | EventMsg::McpToolCallEnd(event)
        | EventMsg::HookStarted(event)
        | EventMsg::HookCompleted(event)
        | EventMsg::SubAgentActivity(event)
        | EventMsg::ContextCompacted(event)
        | EventMsg::LegacyMcpToolCallEnd(event)
        | EventMsg::LegacyPatchApplyEnd(event)
        | EventMsg::LegacyContextCompacted(event)
        | EventMsg::LegacySubAgentActivity(event) => {
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
        EventMsg::LegacyUserMessage(item)
        | EventMsg::LegacyAgentMessage(item)
        | EventMsg::LegacyReasoning(item) => normalize_text_item(item),
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
