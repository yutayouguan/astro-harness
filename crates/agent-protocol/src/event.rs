use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::items::{TextItem, TurnItem};

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum EventMsg {
    TurnStarted(TurnStartedEvent),
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
    LegacyUserMessage(TextItem),
    LegacyAgentMessage(TextItem),
    LegacyReasoning(TextItem),
    LegacyMcpToolCallEnd(ItemEvent),
    LegacyPatchApplyEnd(ItemEvent),
    LegacyContextCompacted(ItemEvent),
    LegacySubAgentActivity(ItemEvent),
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
    use crate::items::{TextItem, TurnItem};

    #[test]
    fn item_completed_roundtrips_without_losing_identity() {
        let event = Event {
            id: "turn-1".into(),
            msg: EventMsg::ItemCompleted(ItemEvent {
                turn_id: "turn-1".into(),
                item: TurnItem::AgentMessage(TextItem {
                    id: "item-1".into(),
                    content: "done".into(),
                }),
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
        assert!(!EventMsg::Error(ErrorEvent {
            message: "failed".into(),
            error_type: "internal".into(),
        })
        .is_terminal());
    }
}
