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
    pub agent_type: Option<String>,
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
    pub parent_thread_id: Option<String>,
    pub canonical_path: AgentPath,
}

/// Events emitted by one V2 runner for state projection and waiting callers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum RunnerEvent {
    TurnStarted,
    TurnCompleted { last_message: String },
    TurnInterrupted,
    TurnErrored { message: String },
    RuntimeTerminated,
}

/// Read model for a root thread and all V2 descendants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTreeSnapshotV2 {
    pub root_thread_id: String,
    pub agents: Vec<AgentThreadV2>,
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
    pub parent_thread_id: String,
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
    pub thread_id: String,
    pub task_name: String,
    pub canonical_path: AgentPath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageAgentV2Result {
    pub delivered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitAgentV2Result {
    pub message: String,
    pub timed_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterruptAgentV2Result {
    pub previous_status: AgentStatusV2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
pub struct AgentThreadMessage {
    pub id: i64,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

/// In-memory spawn request. Credentials are intentionally never persisted in
/// the subagent database.
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
pub struct ReadAgentThreadRequest {
    pub parent_session_id: String,
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendAgentMessageRequest {
    pub parent_session_id: String,
    pub thread_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterruptAgentRequest {
    pub parent_session_id: String,
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseAgentRequest {
    pub parent_session_id: String,
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAgentThreadsRequest {
    pub parent_session_id: String,
    #[serde(default)]
    pub include_closed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitAgentThreadsRequest {
    pub parent_session_id: String,
    pub thread_ids: Vec<String>,
    pub timeout_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::{
        ListAgentsV2Request, MessageAgentV2Request, SpawnAgentV2Request, WaitAgentV2Request,
    };

    #[test]
    fn v2_requests_reject_legacy_argument_fields() {
        let error =
            serde_json::from_str::<ListAgentsV2Request>(r#"{"include_closed":true}"#).unwrap_err();
        assert!(error.to_string().contains("unknown field"));

        let error =
            serde_json::from_str::<MessageAgentV2Request>(r#"{"thread_id":"x","message":"m"}"#)
                .unwrap_err();
        assert!(error.to_string().contains("unknown field"));

        let error =
            serde_json::from_str::<WaitAgentV2Request>(r#"{"thread_ids":["x"]}"#).unwrap_err();
        assert!(error.to_string().contains("unknown field"));

        let error = serde_json::from_str::<SpawnAgentV2Request>(r#"{"task":"x"}"#).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }
}
