use agent_protocol::EventMsg;

use crate::RolloutItem;

/// Returns whether an event is part of durable rollout history.
pub fn should_persist_event_msg(event: &EventMsg) -> bool {
    match event {
        EventMsg::ItemCompleted(_) => true,
        EventMsg::TurnStarted(_)
        | EventMsg::UserInputCommitted(_)
        | EventMsg::TurnComplete(_)
        | EventMsg::TurnAborted(_)
        | EventMsg::TokenCount(_)
        | EventMsg::ContextUsage(_)
        | EventMsg::ThreadSettingsApplied(_)
        | EventMsg::ThreadRolledBack(_) => true,
        EventMsg::ItemStarted(_)
        | EventMsg::AgentMessageContentDelta(_)
        | EventMsg::PlanDelta(_)
        | EventMsg::ReasoningContentDelta(_)
        | EventMsg::ExecCommandOutputDelta(_)
        | EventMsg::PatchApplyUpdated(_)
        | EventMsg::ExecApprovalRequest(_)
        | EventMsg::ApplyPatchApprovalRequest(_)
        | EventMsg::RequestPermissions(_)
        | EventMsg::RequestUserInput(_)
        | EventMsg::ElicitationRequest(_)
        | EventMsg::DynamicToolCallRequest(_)
        | EventMsg::DynamicToolCallResponse(_)
        | EventMsg::McpToolCallBegin(_)
        | EventMsg::McpToolCallEnd(_)
        | EventMsg::HookStarted(_)
        | EventMsg::HookCompleted(_)
        | EventMsg::SubAgentActivity(_)
        | EventMsg::ContextCompacted(_)
        | EventMsg::Error(_)
        | EventMsg::Warning(_)
        | EventMsg::StreamError(_)
        | EventMsg::ShutdownComplete => false,
    }
}

/// Returns whether a rollout item belongs in durable history.
pub fn is_persisted_rollout_item(item: &RolloutItem) -> bool {
    match item {
        RolloutItem::EventMsg(event) => should_persist_event_msg(event),
        RolloutItem::SessionMeta(_)
        | RolloutItem::ResponseItem(_)
        | RolloutItem::TurnContext(_)
        | RolloutItem::WorldState(_)
        | RolloutItem::Compacted(_)
        | RolloutItem::InterAgentCommunication(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{is_persisted_rollout_item, should_persist_event_msg, RolloutItem};
    use agent_protocol::event::{
        ContextUsageEvent, DeltaEvent, ErrorEvent, EventMsg, ItemEvent, TurnCompleteEvent,
        UserInputCommittedEvent,
    };
    use agent_protocol::items::{AgentMessageItem, TurnItem};

    #[test]
    fn durable_policy_persists_completed_items_but_not_deltas() {
        let completed = EventMsg::ItemCompleted(ItemEvent {
            turn_id: "turn-1".into(),
            item: TurnItem::AgentMessage(AgentMessageItem {
                id: "item-1".into(),
                content: "done".into(),
                delivery: None,
            }),
        });
        let delta = EventMsg::AgentMessageContentDelta(DeltaEvent {
            turn_id: "turn-1".into(),
            item_id: "item-1".into(),
            delta: "partial".into(),
        });

        assert!(should_persist_event_msg(&completed));
        assert!(!should_persist_event_msg(&delta));
    }

    #[test]
    fn terminal_state_is_durable_but_error_notification_is_transient() {
        let complete = EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-1".into(),
            last_agent_message: Some("done".into()),
            error: None,
        });
        let error = EventMsg::Error(ErrorEvent {
            message: "failed".into(),
            error_type: "internal".into(),
        });

        assert!(should_persist_event_msg(&complete));
        assert!(!should_persist_event_msg(&error));
    }

    #[test]
    fn context_usage_snapshot_is_durable() {
        let context = EventMsg::ContextUsage(ContextUsageEvent {
            turn_id: "turn-1".into(),
            context_window: 128_000,
            total_tokens: 42,
            segments: Vec::new(),
            updated_at: 123,
            recommend_compact: false,
        });

        assert!(should_persist_event_msg(&context));
    }

    #[test]
    fn user_input_commit_ack_is_durable() {
        let committed = EventMsg::UserInputCommitted(UserInputCommittedEvent {
            turn_id: "turn-1".into(),
            client_message_id: "client-message-1".into(),
        });

        assert!(should_persist_event_msg(&committed));
    }

    #[test]
    fn non_event_rollout_items_are_always_persisted() {
        let item = RolloutItem::TurnContext(serde_json::json!({"turn_id": "turn-1"}));

        assert!(is_persisted_rollout_item(&item));
    }

    #[test]
    fn response_items_compare_structurally() {
        let item = RolloutItem::ResponseItem(types::message::Message::assistant_with_tools(
            "run it",
            vec![types::message::ToolCall {
                id: "call-1".into(),
                name: "exec_command".into(),
                arguments: serde_json::json!({"command": "pwd"}),
                signature: Some("sig-1".into()),
            }],
        ));
        let equal = item.clone();
        let mut changed = item.clone();
        if let RolloutItem::ResponseItem(message) = &mut changed {
            message.tool_calls.as_mut().unwrap()[0].arguments =
                serde_json::json!({"command": "ls"});
        }

        assert_eq!(item, equal);
        assert_ne!(item, changed);
    }
}
