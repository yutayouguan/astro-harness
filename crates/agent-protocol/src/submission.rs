use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::items::ExtensionItem;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnInput {
    pub content: String,
    pub image_data_urls: Vec<String>,
    /// Frontend queue identity acknowledged only after the input is durably recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnInputRequest {
    pub input: Vec<TurnInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnInputMode {
    StartOrSteer,
    StartIfIdle,
    Steer { expected_turn_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnInputSubmission {
    Started { turn_id: String },
    Steered { turn_id: String },
    NotSubmitted { reason: String },
}

impl TurnInputSubmission {
    pub fn turn_id(&self) -> Option<&str> {
        match self {
            Self::Started { turn_id } | Self::Steered { turn_id } => Some(turn_id),
            Self::NotSubmitted { .. } => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum TurnInputError {
    #[error("session submission queue is closed")]
    QueueClosed,
    #[error("turn input reply channel closed")]
    ReplyClosed,
    #[error("invalid turn input: {0}")]
    Invalid(String),
}

/// Result of stopping an unfinished regular turn without recording a terminal event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuspendTurnOutcome {
    Suspended { turn_id: String },
    NotActive,
    HasLiveDescendants,
    UnsupportedTask,
}

#[derive(Debug)]
pub enum Op {
    TurnInput {
        request: TurnInputRequest,
        mode: TurnInputMode,
        reply: tokio::sync::oneshot::Sender<Result<TurnInputSubmission, TurnInputError>>,
    },
    /// Resume sampling for a persisted interrupted turn without appending user input.
    RecoverTurn {
        turn_id: String,
        reply: tokio::sync::oneshot::Sender<Result<TurnInputSubmission, TurnInputError>>,
    },
    /// Stop a regular turn without a terminal event, flush it, then close the runtime.
    SuspendTurnAndShutdown {
        reply: tokio::sync::oneshot::Sender<Result<SuspendTurnOutcome, TurnInputError>>,
    },
    Interrupt,
    /// Terminate this thread's background terminal jobs without interrupting the active turn.
    CleanBackgroundTerminals,
    ThreadSettings {
        settings: Value,
    },
    ExecApproval {
        id: String,
        decision: Value,
    },
    PatchApproval {
        id: String,
        decision: Value,
    },
    UserInputAnswer {
        id: String,
        response: Value,
    },
    RequestPermissionsResponse {
        id: String,
        response: Value,
    },
    DynamicToolResponse {
        id: String,
        response: Value,
    },
    RefreshMcpServers,
    ReloadUserConfig,
    Compact,
    ThreadRollback {
        num_turns: u32,
    },
    Review {
        request: Value,
    },
    InterAgentCommunication {
        communication: Value,
    },
    EmitExtension {
        item: ExtensionItem,
        /// Authoritative turn that owns this item. `None` attaches to the active turn.
        turn_id: Option<String>,
    },
    Shutdown,
}

#[derive(Debug)]
pub struct Submission {
    pub id: String,
    pub op: Op,
}
