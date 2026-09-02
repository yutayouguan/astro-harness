use agent_protocol::EventMsg;

use crate::RolloutItem;

/// Returns whether an event is part of durable rollout history.
pub fn should_persist_event_msg(event: &EventMsg) -> bool {
    match event {
        EventMsg::ItemCompleted(_)
        | EventMsg::RealtimeConversationStarted(_)
        | EventMsg::RealtimeConversationClosed(_) => true,
        EventMsg::TurnStarted(_)
        | EventMsg::UserInputCommitted(_)
        | EventMsg::TurnComplete(_)
        | EventMsg::TurnAborted(_)
        | EventMsg::GuardianAssessment(_)
        | EventMsg::TokenCount(_)
        | EventMsg::ContextUsage(_)
        | EventMsg::ThreadSettingsApplied(_)
        | EventMsg::ThreadRolledBack(_) => true,
        EventMsg::RealtimeConversationSdp(_)
        | EventMsg::RealtimeConversationRealtime(_)
        | EventMsg::RealtimeConversationListVoicesResponse(_)
        | EventMsg::ItemStarted(_)
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
        | RolloutItem::RealtimeItem(_)
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
                questions: None,
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
            estimated_total_tokens: 42,
            source: agent_protocol::ContextUsageSource::LocalEstimate,
            latest_usage: None,
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
    fn realtime_boundaries_are_durable_but_stream_payloads_are_transient() {
        let started = EventMsg::RealtimeConversationStarted(
            agent_protocol::RealtimeConversationStartedEvent {
                realtime_session_id: Some("rt-1".into()),
                call_id: None,
                model: "gpt-realtime".into(),
                version: agent_protocol::RealtimeConversationVersion::V2,
            },
        );
        let payload = EventMsg::RealtimeConversationRealtime(
            agent_protocol::RealtimeConversationRealtimeEvent {
                payload: agent_protocol::RealtimeEvent::AudioOut(
                    agent_protocol::RealtimeAudioFrame {
                        data: vec![0],
                        sample_rate: 24_000,
                        num_channels: 1,
                        format: agent_protocol::RealtimeAudioFormat::Pcm16,
                    },
                ),
            },
        );
        let closed =
            EventMsg::RealtimeConversationClosed(agent_protocol::RealtimeConversationClosedEvent {
                reason: Some("requested".into()),
            });

        assert!(should_persist_event_msg(&started));
        assert!(!should_persist_event_msg(&payload));
        assert!(should_persist_event_msg(&closed));
    }

    #[test]
    fn non_event_rollout_items_are_always_persisted() {
        let item = RolloutItem::TurnContext(serde_json::json!({"turn_id": "turn-1"}));

        assert!(is_persisted_rollout_item(&item));
    }

    #[test]
    fn response_items_compare_structurally() {
        let item = RolloutItem::ResponseItem(agent_protocol::ResponseItem::FunctionCall {
            id: None,
            name: "exec_command".into(),
            namespace: None,
            arguments: "{\"command\":\"pwd\"}".into(),
            encrypted_function_args: None,
            call_id: "call-1".into(),
            internal_chat_message_metadata_passthrough: None,
        });
        let equal = item.clone();
        let mut changed = item.clone();
        if let RolloutItem::ResponseItem(agent_protocol::ResponseItem::FunctionCall {
            arguments,
            ..
        }) = &mut changed
        {
            *arguments = "{\"command\":\"ls\"}".into();
        }

        assert_eq!(item, equal);
        assert_ne!(item, changed);
    }
}
