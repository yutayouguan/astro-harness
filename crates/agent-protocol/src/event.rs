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
    /// 总输入 token，包含 cache read/write。
    pub input_tokens: u64,
    /// 版本标记；旧 rollout 的 `input_tokens` 仅表示未缓存输入。
    #[serde(default)]
    pub input_tokens_include_cache: bool,
    /// 未命中缓存的输入 token，用于计费与审计。
    #[serde(default)]
    pub uncached_input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    /// Provider wire response 原始 total；`None` 表示由分项重算。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_total_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub request_count: u64,
    #[serde(default)]
    pub cache_read_reported: bool,
    #[serde(default)]
    pub cache_write_reported: bool,
    #[serde(default)]
    pub reasoning_reported: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextUsageSource {
    ProviderReported,
    ProviderRecomputed,
    #[default]
    LocalEstimate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextUsageBreakdown {
    /// Provider 语义的总输入，包含 cache read/write。
    pub input_tokens: u64,
    pub uncached_input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_total_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    /// `output_tokens` 的子集。
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub cache_read_reported: bool,
    #[serde(default)]
    pub cache_write_reported: bool,
    #[serde(default)]
    pub reasoning_reported: bool,
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
    /// 本地分层估算，即使 top-line 采用 Provider actual 也保留。
    #[serde(default)]
    pub estimated_total_tokens: u32,
    #[serde(default)]
    pub source: ContextUsageSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_usage: Option<ContextUsageBreakdown>,
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
    fn legacy_token_count_keeps_old_input_semantics_marker() {
        let event: Event = serde_json::from_value(serde_json::json!({
            "id": "turn-1",
            "msg": {
                "type": "token_count",
                "data": {
                    "turn_id": "turn-1",
                    "input_tokens": 60,
                    "output_tokens": 25,
                    "total_tokens": 125,
                    "cache_read_tokens": 40,
                    "cache_write_tokens": 0,
                    "reasoning_tokens": 0,
                    "request_count": 1
                }
            }
        }))
        .unwrap();

        let EventMsg::TokenCount(tokens) = event.msg else {
            panic!("expected token_count event");
        };
        assert!(!tokens.input_tokens_include_cache);
        assert_eq!(tokens.input_tokens, 60);
        assert_eq!(tokens.uncached_input_tokens, 0);
    }

    #[test]
    fn context_usage_roundtrips_with_segment_items() {
        let event = Event {
            id: "turn-1".into(),
            msg: EventMsg::ContextUsage(ContextUsageEvent {
                turn_id: "turn-1".into(),
                context_window: 128_000,
                total_tokens: 42,
                estimated_total_tokens: 40,
                source: ContextUsageSource::ProviderReported,
                latest_usage: Some(ContextUsageBreakdown {
                    input_tokens: 40,
                    uncached_input_tokens: 8,
                    output_tokens: 2,
                    total_tokens: 42,
                    provider_total_tokens: Some(42),
                    cache_read_tokens: 32,
                    cache_write_tokens: 0,
                    reasoning_tokens: 1,
                    cache_read_reported: true,
                    cache_write_reported: false,
                    reasoning_reported: true,
                }),
                segments: vec![ContextUsageSegment {
                    id: "tools".into(),
                    tokens: 42,
                    count: Some(1),
                    items: vec![ContextUsageItem {
                        id: "exec_command".into(),
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
