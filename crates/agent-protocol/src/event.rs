use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::items::TurnItem;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub msg: EventMsg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemEvent {
    pub turn_id: String,
    pub item: TurnItem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaEvent {
    pub turn_id: String,
    pub item_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlRequestEvent {
    pub turn_id: String,
    pub item_id: String,
    pub request_id: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnStartedEvent {
    pub turn_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserInputCommittedEvent {
    pub turn_id: String,
    pub client_message_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEvent {
    pub message: String,
    pub error_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnCompleteEvent {
    pub turn_id: String,
    pub last_agent_message: Option<String>,
    pub error: Option<ErrorEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnAbortReason {
    Interrupted,
    Replaced,
    ReviewEnded,
    BudgetLimited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnAbortedEvent {
    pub turn_id: Option<String>,
    pub reason: TurnAbortReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCountEvent {
    pub turn_id: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub request_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextUsageItem {
    pub id: String,
    pub label: String,
    pub tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextUsageSegment {
    pub id: String,
    pub tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ContextUsageItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextUsageEvent {
    pub turn_id: String,
    pub context_window: u32,
    pub total_tokens: u32,
    pub segments: Vec<ContextUsageSegment>,
    pub updated_at: i64,
    #[serde(default)]
    pub recommend_compact: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum EventMsg {
    TurnStarted(TurnStartedEvent),
    UserInputCommitted(UserInputCommittedEvent),
    ItemStarted(ItemEvent),
    ItemCompleted(ItemEvent),
    AgentMessageContentDelta(DeltaEvent),
    PlanDelta(DeltaEvent),
    ReasoningContentDelta(DeltaEvent),
    ExecCommandOutputDelta(DeltaEvent),
    PatchApplyUpdated(DeltaEvent),
    ExecApprovalRequest(ControlRequestEvent),
    ApplyPatchApprovalRequest(ControlRequestEvent),
    RequestPermissions(ControlRequestEvent),
    RequestUserInput(ControlRequestEvent),
    ElicitationRequest(ControlRequestEvent),
    DynamicToolCallRequest(ControlRequestEvent),
    DynamicToolCallResponse(ControlRequestEvent),
    McpToolCallBegin(ItemEvent),
    McpToolCallEnd(ItemEvent),
    HookStarted(ItemEvent),
    HookCompleted(ItemEvent),
    SubAgentActivity(ItemEvent),
    ContextCompacted(ItemEvent),
    ContextUsage(ContextUsageEvent),
    TokenCount(TokenCountEvent),
    ThreadSettingsApplied(Value),
    ThreadRolledBack(Value),
    Error(ErrorEvent),
    Warning(ErrorEvent),
    StreamError(ErrorEvent),
    TurnComplete(TurnCompleteEvent),
    TurnAborted(TurnAbortedEvent),
    ShutdownComplete,
}

impl EventMsg {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::TurnComplete(_) | Self::TurnAborted(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::{AgentMessageItem, TurnItem};

    #[test]
    fn item_completed_roundtrips_without_losing_identity() {
        let event = Event {
            id: "turn-1".into(),
            msg: EventMsg::ItemCompleted(ItemEvent {
                turn_id: "turn-1".into(),
                item: TurnItem::AgentMessage(AgentMessageItem {
                    id: "item-1".into(),
                    content: "done".into(),
                    delivery: None,
                }),
            }),
        };
        let json = serde_json::to_string(&event).unwrap();
        let restored: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, event);
    }

    #[test]
    fn legacy_agent_message_without_delivery_remains_compatible() {
        let item: TurnItem = serde_json::from_value(serde_json::json!({
            "type": "agent_message",
            "data": {"id": "item-1", "content": "done"}
        }))
        .unwrap();

        assert_eq!(
            item,
            TurnItem::AgentMessage(AgentMessageItem {
                id: "item-1".into(),
                content: "done".into(),
                delivery: None,
            })
        );
    }

    #[test]
    fn context_usage_roundtrips_with_segment_items() {
        let event = Event {
            id: "turn-1".into(),
            msg: EventMsg::ContextUsage(ContextUsageEvent {
                turn_id: "turn-1".into(),
                context_window: 128_000,
                total_tokens: 42,
                segments: vec![ContextUsageSegment {
                    id: "tools".into(),
                    tokens: 42,
                    count: Some(1),
                    items: vec![ContextUsageItem {
                        id: "terminal".into(),
                        label: "Terminal".into(),
                        tokens: 42,
                    }],
                }],
                updated_at: 123,
                recommend_compact: false,
            }),
        };

        let json = serde_json::to_string(&event).unwrap();
        let restored: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, event);
    }

    #[test]
    fn user_input_committed_roundtrips_with_both_identities() {
        let event = Event {
            id: "event-1".into(),
            msg: EventMsg::UserInputCommitted(UserInputCommittedEvent {
                turn_id: "turn-1".into(),
                client_message_id: "client-message-1".into(),
            }),
        };

        let json = serde_json::to_string(&event).unwrap();
        let restored: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, event);
    }

    #[test]
    fn only_complete_and_aborted_are_terminal() {
        assert!(EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "t".into(),
            last_agent_message: None,
            error: None,
        })
        .is_terminal());
        assert!(EventMsg::TurnAborted(TurnAbortedEvent {
            turn_id: Some("t".into()),
            reason: TurnAbortReason::Interrupted,
        })
        .is_terminal());
        assert!(!EventMsg::UserInputCommitted(UserInputCommittedEvent {
            turn_id: "t".into(),
            client_message_id: "client-message-1".into(),
        })
        .is_terminal());
        assert!(!EventMsg::Error(ErrorEvent {
            message: "failed".into(),
            error_type: "internal".into(),
        })
        .is_terminal());
    }

    #[test]
    fn removed_legacy_event_variants_are_rejected() {
        let json = serde_json::json!({
            "id": "turn-1",
            "msg": {
                "type": "legacy_agent_message",
                "data": {"id": "item-1", "content": "old payload"}
            }
        });

        assert!(serde_json::from_value::<Event>(json).is_err());
    }
}
