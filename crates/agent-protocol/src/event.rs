use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::items::TurnItem;
use crate::realtime::{
    RealtimeConversationClosedEvent, RealtimeConversationListVoicesResponseEvent,
    RealtimeConversationRealtimeEvent, RealtimeConversationSdpEvent,
    RealtimeConversationStartedEvent,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianAssessmentStatus {
    InProgress,
    Approved,
    Denied,
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardianAssessmentEvent {
    pub id: String,
    pub target_item_id: String,
    pub turn_id: String,
    pub status: GuardianAssessmentStatus,
    pub canonical_action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_source: Option<String>,
    pub started_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<i64>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadSettingsSnapshot {
    pub provider: String,
    pub model: String,
    pub interaction_mode: types::InteractionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub workspace_roots: Vec<String>,
    pub context_window: u32,
    pub temperature: f32,
    pub thinking_enabled: bool,
    pub reasoning_effort: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadSettingsAppliedEvent {
    pub thread_settings: ThreadSettingsSnapshot,
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
pub struct ThreadRolledBackEvent {
    pub num_turns: u32,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEventName {
    PreToolUse,
    PermissionRequest,
    PostToolUse,
    PreCompact,
    PostCompact,
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    SubagentStart,
    SubagentStop,
    Stop,
    Interrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookHandlerType {
    Command,
    McpTool,
    Prompt,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookExecutionMode {
    Sync,
    Async,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookScope {
    Thread,
    Turn,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookSource {
    System,
    User,
    Project,
    Mdm,
    SessionFlags,
    Plugin,
    CloudRequirements,
    CloudManagedConfig,
    LegacyManagedConfigFile,
    LegacyManagedConfigMdm,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookTrustStatus {
    Managed,
    Untrusted,
    Trusted,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookRunStatus {
    Running,
    Completed,
    Failed,
    Blocked,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookOutputEntryKind {
    Warning,
    Stop,
    Feedback,
    Context,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HookOutputEntry {
    pub kind: HookOutputEntryKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HookRunSummary {
    pub id: String,
    pub event_name: HookEventName,
    pub handler_type: HookHandlerType,
    pub execution_mode: HookExecutionMode,
    pub scope: HookScope,
    pub source_path: String,
    #[serde(default)]
    pub source: HookSource,
    pub display_order: i64,
    pub status: HookRunStatus,
    pub status_message: Option<String>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub entries: Vec<HookOutputEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HookStartedEvent {
    pub turn_id: Option<String>,
    pub run: HookRunSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HookCompletedEvent {
    pub turn_id: Option<String>,
    pub run: HookRunSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum EventMsg {
    RealtimeConversationStarted(RealtimeConversationStartedEvent),
    RealtimeConversationSdp(RealtimeConversationSdpEvent),
    RealtimeConversationRealtime(RealtimeConversationRealtimeEvent),
    RealtimeConversationClosed(RealtimeConversationClosedEvent),
    RealtimeConversationListVoicesResponse(RealtimeConversationListVoicesResponseEvent),
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
    GuardianAssessment(GuardianAssessmentEvent),
    DynamicToolCallRequest(ControlRequestEvent),
    DynamicToolCallResponse(ControlRequestEvent),
    McpToolCallBegin(ItemEvent),
    McpToolCallEnd(ItemEvent),
    HookStarted(HookStartedEvent),
    HookCompleted(HookCompletedEvent),
    SubAgentActivity(ItemEvent),
    ContextCompacted(ItemEvent),
    ContextUsage(ContextUsageEvent),
    TokenCount(TokenCountEvent),
    ThreadSettingsApplied(ThreadSettingsAppliedEvent),
    ThreadRolledBack(ThreadRolledBackEvent),
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
                    questions: None,
                }),
            }),
        };
        let json = serde_json::to_string(&event).unwrap();
        let restored: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, event);
    }

    #[test]
    fn hook_lifecycle_roundtrips_native_run_summary() {
        let run = HookRunSummary {
            id: "hook-run-1".into(),
            event_name: HookEventName::PreToolUse,
            handler_type: HookHandlerType::Command,
            execution_mode: HookExecutionMode::Sync,
            scope: HookScope::Turn,
            source_path: "/tmp/config.toml".into(),
            source: HookSource::Project,
            display_order: 0,
            status: HookRunStatus::Running,
            status_message: Some("Checking command".into()),
            started_at: 42,
            completed_at: None,
            duration_ms: None,
            entries: Vec::new(),
        };
        let event = Event {
            id: "turn-1".into(),
            msg: EventMsg::HookStarted(HookStartedEvent {
                turn_id: Some("turn-1".into()),
                run,
            }),
        };

        let json = serde_json::to_string(&event).unwrap();
        let restored: Event = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, event);
        assert!(json.contains("\"event_name\":\"pre_tool_use\""));
        assert!(json.contains("\"handler_type\":\"command\""));
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
                questions: None,
            })
        );
    }

    #[test]
    fn async_agent_message_questions_roundtrip() {
        let item = TurnItem::AgentMessage(AgentMessageItem {
            id: "item-questions".into(),
            content: "Choose\n- A\n- B".into(),
            delivery: Some(crate::AgentMessageDelivery::Async),
            questions: Some(vec![crate::AsyncUserInputQuestion {
                title: "Choose".into(),
                options: Some(vec!["A".into(), "B".into()]),
            }]),
        });

        let json = serde_json::to_string(&item).unwrap();
        assert_eq!(serde_json::from_str::<TurnItem>(&json).unwrap(), item);
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
