use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::path::AgentPath;

/// Stable status discriminator for [`AgentStatusV2`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatusKind {
    PendingInit,
    Running,
    Interrupted,
    Completed,
    Errored,
    Shutdown,
}

/// Canonical final V2 lifecycle status; the V2 suffix is transitional while
/// legacy thread statuses remain exported for downstream compilation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum AgentStatusV2 {
    PendingInit,
    Running,
    Interrupted,
    Completed { last_message: String },
    Errored { message: String },
    Shutdown,
}

impl AgentStatusV2 {
    pub fn kind(&self) -> AgentStatusKind {
        match self {
            Self::PendingInit => AgentStatusKind::PendingInit,
            Self::Running => AgentStatusKind::Running,
            Self::Interrupted => AgentStatusKind::Interrupted,
            Self::Completed { .. } => AgentStatusKind::Completed,
            Self::Errored { .. } => AgentStatusKind::Errored,
            Self::Shutdown => AgentStatusKind::Shutdown,
        }
    }
}

/// Durable projection of a V2 agent thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentThreadV2 {
    pub thread_id: String,
    pub root_thread_id: String,
    pub parent_thread_id: Option<String>,
    pub canonical_path: AgentPath,
    pub task_name: String,
    pub agent_type: String,
    pub session_id: String,
    pub status: AgentStatusV2,
    pub created_at: String,
    pub updated_at: String,
}

/// The identity reserved before a V2 runner is started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadReservation {
    pub thread_id: String,
    pub root_thread_id: String,
    pub parent_thread_id: String,
    pub canonical_path: AgentPath,
    pub task_name: String,
    pub agent_type: String,
    pub session_id: String,
}

/// Events emitted by one V2 runner for state projection and waiting callers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunnerEvent {
    TurnStarted {
        turn_id: String,
    },
    TurnCompleted {
        turn_id: String,
        last_message: String,
    },
    TurnInterrupted {
        turn_id: String,
        reason: String,
    },
    TurnErrored {
        turn_id: String,
        message: String,
    },
    RuntimeTerminated,
}

/// Read model for a root thread and all V2 descendants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTreeSnapshotV2 {
    pub root_thread_id: String,
    pub threads: Vec<AgentThreadV2>,
    pub activity_sequence: u64,
}

/// Complete durable Session timeline row exposed only to the desktop control
/// plane.  Keeping every structured field prevents the UI from falling back
/// to the lossy historical subagent transcript table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentThreadMessageV2 {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: Option<String>,
    pub compressed_content: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Option<serde_json::Value>,
    pub tool_name: Option<String>,
    pub timestamp: f64,
    pub token_count: Option<i64>,
    pub finish_reason: Option<String>,
    pub reasoning: Option<String>,
    pub reasoning_content: Option<String>,
    pub reasoning_details: Option<serde_json::Value>,
    pub codex_reasoning_items: Option<serde_json::Value>,
    pub codex_message_items: Option<serde_json::Value>,
    pub media_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentThreadDetailV2 {
    pub thread: AgentThreadV2,
    pub messages: Vec<AgentThreadMessageV2>,
}

/// Model-visible V2 spawn input. The final public name will be
/// `SpawnAgentRequest` after legacy request removal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnAgentV2Request {
    pub task_name: String,
    pub message: String,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub fork_turns: Option<String>,
}

/// Runtime-only V2 spawn material. It intentionally does not implement serde
/// so credentials and process-local dependencies cannot enter projections.
#[derive(Clone)]
pub struct SpawnRuntimeV2Request {
    pub model_request: SpawnAgentV2Request,
    pub parent_thread_id: String,
    pub parent_path: AgentPath,
    pub root_thread_id: String,
    pub parent_session_id: String,
    pub parent_agent_id: String,
    pub developer_instructions: String,
    pub context_snapshot: String,
    pub sandbox_mode: Option<String>,
    pub mcp_servers: BTreeMap<String, toml::Value>,
    pub skills_config: Vec<crate::SkillConfigEntry>,
    pub chat_targets: Vec<types::ChatTarget>,
    pub project_root: Option<PathBuf>,
    pub hook_bus: Option<Arc<hooks::PluginHookBus>>,
    pub interrupt_message: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListAgentsV2Request {
    pub path_prefix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageAgentV2Request {
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitAgentV2Request {
    pub timeout_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterruptAgentV2Request {
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnAgentV2Result {
    pub thread: AgentThreadV2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageAgentV2Result {
    pub message_id: String,
    pub queued: bool,
    pub turn_triggered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitAgentV2Result {
    pub message: String,
    pub timed_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterruptAgentV2Result {
    pub thread: AgentThreadV2,
    pub previous_status: AgentStatusV2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Transitional legacy status; removed in Tasks 6/10.
pub enum AgentThreadStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Interrupted,
    Closed,
}

impl AgentThreadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Closed => "closed",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "interrupted" => Self::Interrupted,
            "closed" => Self::Closed,
            _ => Self::Failed,
        }
    }

    pub fn is_wait_complete(self) -> bool {
        !matches!(self, Self::Pending | Self::Running)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Transitional legacy durable thread projection; removed in Tasks 6/10.
pub struct AgentThread {
    pub id: String,
    pub parent_session_id: String,
    pub parent_agent_id: String,
    pub agent_name: String,
    pub task: String,
    pub status: AgentThreadStatus,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub model: Option<String>,
    pub model_reasoning_effort: Option<String>,
    pub sandbox_mode: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    pub closed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Transitional legacy thread message projection; removed in Tasks 6/10.
pub struct AgentThreadMessage {
    pub id: i64,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

/// Transitional legacy in-memory spawn request; removed in Tasks 6/10.
/// Credentials are intentionally never persisted in the subagent database.
#[derive(Clone)]
pub struct SpawnAgentRequest {
    pub parent_session_id: String,
    pub parent_agent_id: String,
    pub task: String,
    pub agent_name: String,
    pub developer_instructions: String,
    pub context_snapshot: String,
    pub model: Option<String>,
    pub model_reasoning_effort: Option<String>,
    pub sandbox_mode: Option<String>,
    pub mcp_servers: BTreeMap<String, toml::Value>,
    pub skills_config: Vec<crate::SkillConfigEntry>,
    pub chat_targets: Vec<types::ChatTarget>,
    pub project_root: Option<PathBuf>,
    pub hook_bus: Option<Arc<hooks::PluginHookBus>>,
    pub interrupt_message: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Transitional legacy read request; removed in Tasks 6/10.
pub struct ReadAgentThreadRequest {
    pub parent_session_id: String,
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Transitional legacy message request; removed in Tasks 6/10.
pub struct SendAgentMessageRequest {
    pub parent_session_id: String,
    pub thread_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Transitional legacy interrupt request; removed in Tasks 6/10.
pub struct InterruptAgentRequest {
    pub parent_session_id: String,
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Transitional legacy close request; removed in Tasks 6/10.
pub struct CloseAgentRequest {
    pub parent_session_id: String,
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Transitional legacy list request; removed in Tasks 6/10.
pub struct ListAgentThreadsRequest {
    pub parent_session_id: String,
    #[serde(default)]
    pub include_closed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Transitional legacy wait request; removed in Tasks 6/10.
pub struct WaitAgentThreadsRequest {
    pub parent_session_id: String,
    pub thread_ids: Vec<String>,
    pub timeout_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::{
        AgentStatusV2, AgentThreadV2, AgentTreeSnapshotV2, InterruptAgentV2Request,
        InterruptAgentV2Result, ListAgentsV2Request, MessageAgentV2Request, MessageAgentV2Result,
        RunnerEvent, SpawnAgentV2Request, SpawnAgentV2Result, ThreadReservation,
        WaitAgentV2Request, WaitAgentV2Result,
    };
    use crate::AgentPath;

    #[test]
    fn v2_requests_reject_legacy_argument_fields() {
        let error =
            serde_json::from_str::<ListAgentsV2Request>(r#"{"include_closed":true}"#).unwrap_err();
        assert!(error.to_string().contains("unknown field"));

        let error = serde_json::from_str::<MessageAgentV2Request>(
            r#"{"target":"/root/research","message":"m","thread_id":"x"}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));

        let error =
            serde_json::from_str::<WaitAgentV2Request>(r#"{"thread_ids":["x"]}"#).unwrap_err();
        assert!(error.to_string().contains("unknown field"));

        let error = serde_json::from_str::<SpawnAgentV2Request>(
            r#"{"task_name":"research","message":"investigate","task":"x"}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));

        let error = serde_json::from_str::<InterruptAgentV2Request>(
            r#"{"target":"/root/research","thread_id":"x"}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn v2_requests_accept_only_their_canonical_shapes() {
        assert!(serde_json::from_str::<SpawnAgentV2Request>(
            r#"{"task_name":"research","message":"investigate"}"#
        )
        .is_ok());
        assert!(serde_json::from_str::<ListAgentsV2Request>(r#"{}"#).is_ok());
        assert!(serde_json::from_str::<MessageAgentV2Request>(
            r#"{"target":"/root/research","message":"continue"}"#
        )
        .is_ok());
        assert!(serde_json::from_str::<WaitAgentV2Request>(r#"{}"#).is_ok());
        assert!(
            serde_json::from_str::<InterruptAgentV2Request>(r#"{"target":"/root/research"}"#)
                .is_ok()
        );
    }

    #[test]
    fn v2_statuses_and_runner_events_have_stable_json_shapes() {
        let statuses = [
            (
                AgentStatusV2::PendingInit,
                serde_json::json!({"kind":"pending_init"}),
            ),
            (
                AgentStatusV2::Running,
                serde_json::json!({"kind":"running"}),
            ),
            (
                AgentStatusV2::Interrupted,
                serde_json::json!({"kind":"interrupted"}),
            ),
            (
                AgentStatusV2::Completed {
                    last_message: "done".into(),
                },
                serde_json::json!({"kind":"completed","payload":{"last_message":"done"}}),
            ),
            (
                AgentStatusV2::Errored {
                    message: "failed".into(),
                },
                serde_json::json!({"kind":"errored","payload":{"message":"failed"}}),
            ),
            (
                AgentStatusV2::Shutdown,
                serde_json::json!({"kind":"shutdown"}),
            ),
        ];
        for (status, expected) in statuses {
            assert_eq!(serde_json::to_value(&status).unwrap(), expected);
            assert_eq!(
                serde_json::from_value::<AgentStatusV2>(expected).unwrap(),
                status
            );
        }

        let events = [
            (
                RunnerEvent::TurnStarted {
                    turn_id: "t1".into(),
                },
                serde_json::json!({"kind":"turn_started","turn_id":"t1"}),
            ),
            (
                RunnerEvent::TurnCompleted {
                    turn_id: "t2".into(),
                    last_message: "done".into(),
                },
                serde_json::json!({"kind":"turn_completed","turn_id":"t2","last_message":"done"}),
            ),
            (
                RunnerEvent::TurnInterrupted {
                    turn_id: "t3".into(),
                    reason: "cancelled".into(),
                },
                serde_json::json!({"kind":"turn_interrupted","turn_id":"t3","reason":"cancelled"}),
            ),
            (
                RunnerEvent::TurnErrored {
                    turn_id: "t4".into(),
                    message: "failed".into(),
                },
                serde_json::json!({"kind":"turn_errored","turn_id":"t4","message":"failed"}),
            ),
            (
                RunnerEvent::RuntimeTerminated,
                serde_json::json!({"kind":"runtime_terminated"}),
            ),
        ];
        for (event, expected) in events {
            assert_eq!(serde_json::to_value(&event).unwrap(), expected);
            assert_eq!(
                serde_json::from_value::<RunnerEvent>(expected).unwrap(),
                event
            );
        }
    }

    #[test]
    fn v2_projection_and_result_shapes_preserve_runtime_identity() {
        let thread = AgentThreadV2 {
            thread_id: "thread-1".into(),
            root_thread_id: "root-1".into(),
            parent_thread_id: Some("parent-1".into()),
            canonical_path: AgentPath::parse("/root/research").unwrap(),
            task_name: "research".into(),
            agent_type: "explorer".into(),
            session_id: "session-1".into(),
            status: AgentStatusV2::Running,
            created_at: "2026-08-18T00:00:00Z".into(),
            updated_at: "2026-08-18T00:00:00Z".into(),
        };
        let reservation = ThreadReservation {
            thread_id: thread.thread_id.clone(),
            root_thread_id: thread.root_thread_id.clone(),
            parent_thread_id: "parent-1".into(),
            canonical_path: thread.canonical_path.clone(),
            task_name: thread.task_name.clone(),
            agent_type: thread.agent_type.clone(),
            session_id: thread.session_id.clone(),
        };
        assert_eq!(reservation.task_name, "research");
        let turn_started = serde_json::to_value(RunnerEvent::TurnStarted {
            turn_id: "t1".into(),
        })
        .unwrap();
        assert_eq!(
            turn_started,
            serde_json::json!({ "kind": "turn_started", "turn_id": "t1" })
        );
        assert!(turn_started.get("payload").is_none());
        let snapshot = AgentTreeSnapshotV2 {
            root_thread_id: thread.root_thread_id.clone(),
            threads: vec![thread.clone()],
            activity_sequence: 7,
        };
        assert_eq!(snapshot.activity_sequence, 7);
        assert_eq!(
            SpawnAgentV2Result {
                thread: thread.clone()
            }
            .thread
            .canonical_path
            .as_str(),
            "/root/research"
        );
        assert_eq!(
            InterruptAgentV2Result {
                thread,
                previous_status: AgentStatusV2::Running,
            }
            .previous_status,
            AgentStatusV2::Running
        );
        assert_eq!(
            serde_json::to_value(MessageAgentV2Result {
                message_id: "message-1".into(),
                queued: true,
                turn_triggered: false,
            })
            .unwrap()["message_id"],
            "message-1"
        );
        assert_eq!(
            serde_json::to_value(WaitAgentV2Result {
                message: "Wait completed.".into(),
                timed_out: false,
            })
            .unwrap()["timed_out"],
            false
        );
    }
}
