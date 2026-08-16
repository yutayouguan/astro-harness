use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

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
    pub chat_targets: Vec<types::ChatTarget>,
    pub project_root: Option<PathBuf>,
    pub hook_bus: Option<Arc<hooks::PluginHookBus>>,
    pub interrupt_message: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendAgentMessageRequest {
    pub thread_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterruptAgentRequest {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseAgentRequest {
    pub thread_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListAgentThreadsRequest {
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub include_closed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitAgentThreadsRequest {
    pub thread_ids: Vec<String>,
    pub timeout_ms: u64,
}
