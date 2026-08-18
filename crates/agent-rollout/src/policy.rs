use agent_protocol::EventMsg;

use crate::{RolloutItem, ThreadHistoryMode};

/// Returns whether an event is part of the durable rollout history for `mode`.
pub fn should_persist_event_msg(event: &EventMsg, mode: ThreadHistoryMode) -> bool {
    match event {
        EventMsg::ItemCompleted(_) => mode == ThreadHistoryMode::Paginated,
        EventMsg::LegacyUserMessage(_)
        | EventMsg::LegacyAgentMessage(_)
        | EventMsg::LegacyReasoning(_)
        | EventMsg::LegacyMcpToolCallEnd(_)
        | EventMsg::LegacyPatchApplyEnd(_)
        | EventMsg::LegacyContextCompacted(_)
        | EventMsg::LegacySubAgentActivity(_) => mode == ThreadHistoryMode::Legacy,
        EventMsg::TurnStarted(_)
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

/// Returns whether a rollout item belongs in durable history for `mode`.
pub fn is_persisted_rollout_item(item: &RolloutItem, mode: ThreadHistoryMode) -> bool {
    match item {
        RolloutItem::EventMsg(event) => should_persist_event_msg(event, mode),
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
    use super::super::{
        is_persisted_rollout_item, should_persist_event_msg, RolloutItem, ThreadHistoryMode,
    };
    use agent_protocol::event::{
        ContextUsageEvent, DeltaEvent, ErrorEvent, EventMsg, ItemEvent, TurnCompleteEvent,
    };
    use agent_protocol::items::{TextItem, TurnItem};

    #[test]
    fn paginated_persists_completed_items_but_not_deltas() {
        let completed = EventMsg::ItemCompleted(ItemEvent {
            turn_id: "turn-1".into(),
            item: TurnItem::AgentMessage(TextItem {
                id: "item-1".into(),
                content: "done".into(),
            }),
        });
        let delta = EventMsg::AgentMessageContentDelta(DeltaEvent {
            turn_id: "turn-1".into(),
            item_id: "item-1".into(),
            delta: "partial".into(),
        });

        assert!(should_persist_event_msg(
            &completed,
            ThreadHistoryMode::Paginated
        ));
        assert!(!should_persist_event_msg(
            &delta,
            ThreadHistoryMode::Paginated
        ));
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

        assert!(should_persist_event_msg(
            &complete,
            ThreadHistoryMode::Paginated
        ));
        assert!(!should_persist_event_msg(
            &error,
            ThreadHistoryMode::Paginated
        ));
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

        assert!(should_persist_event_msg(
            &context,
            ThreadHistoryMode::Paginated
        ));
    }

    #[test]
    fn legacy_completion_events_only_persist_in_legacy_mode() {
        let event = EventMsg::LegacyAgentMessage(TextItem {
            id: "item-1".into(),
            content: "done".into(),
        });

        assert!(should_persist_event_msg(&event, ThreadHistoryMode::Legacy));
        assert!(!should_persist_event_msg(
            &event,
            ThreadHistoryMode::Paginated
        ));
    }

    #[test]
    fn non_event_rollout_items_are_always_persisted() {
        let item = RolloutItem::TurnContext(serde_json::json!({"turn_id": "turn-1"}));

        assert!(is_persisted_rollout_item(&item, ThreadHistoryMode::Legacy));
        assert!(is_persisted_rollout_item(
            &item,
            ThreadHistoryMode::Paginated
        ));
    }

    #[test]
    fn response_items_compare_structurally() {
        let item = RolloutItem::ResponseItem(types::message::Message::assistant_with_tools(
            "run it",
            vec![types::message::ToolCall {
                id: "call-1".into(),
                name: "terminal".into(),
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
