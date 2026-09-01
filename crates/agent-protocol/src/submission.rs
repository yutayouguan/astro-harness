use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::control::{
    DynamicToolResponse, ElicitationResponse, InterAgentCommunication, RequestPermissionsResponse,
    RequestUserInputResponse, ReviewDecision, ReviewRequest, ThreadSettingsOverrides,
    TurnSettingsOutcome, TurnSettingsUpdate, UserShellLaunch,
};
use crate::items::ExtensionItem;
use crate::realtime::{
    ConversationAudioParams, ConversationSpeechParams, ConversationStartParams,
    ConversationTextParams,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnInput {
    pub content: String,
    pub image_data_urls: Vec<String>,
    /// Frontend queue identity acknowledged only after the input is durably recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnInputRequest {
    pub input: Vec<TurnInput>,
    /// Persistent settings applied only after this input is accepted.
    #[serde(skip, default)]
    pub thread_settings: ThreadSettingsOverrides,
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
#[non_exhaustive]
pub enum Op {
    /// Start a realtime conversation stream for this thread.
    RealtimeConversationStart {
        params: ConversationStartParams,
        /// Realtime credentials and routing stay scoped to this connection.
        target: types::ChatTarget,
        /// Completes only after the provider handshake succeeds or fails.
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    /// Append an audio frame to the running realtime conversation.
    RealtimeConversationAudio(ConversationAudioParams),
    /// Append a role-bearing text item to the running realtime conversation.
    RealtimeConversationText(ConversationTextParams),
    /// Ask the running realtime conversation to speak the supplied text.
    RealtimeConversationSpeech(ConversationSpeechParams),
    /// Close the running realtime conversation.
    RealtimeConversationClose,
    /// Emit the voices supported by the realtime transport.
    RealtimeConversationListVoices,
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
        thread_settings: ThreadSettingsOverrides,
    },
    ExecApproval {
        id: String,
        decision: ReviewDecision,
    },
    PatchApproval {
        id: String,
        decision: ReviewDecision,
    },
    UserInputAnswer {
        id: String,
        response: RequestUserInputResponse,
    },
    RequestPermissionsResponse {
        id: String,
        response: RequestPermissionsResponse,
    },
    DynamicToolResponse {
        id: String,
        response: DynamicToolResponse,
    },
    /// Resolve one pending MCP `elicitation/create` request.
    ResolveElicitation {
        server_name: String,
        request_id: String,
        response: ElicitationResponse,
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    /// Atomically update the next sampling step of the named active turn.
    TurnSettings {
        turn_id: String,
        update: TurnSettingsUpdate,
        reply: tokio::sync::oneshot::Sender<TurnSettingsOutcome>,
    },
    /// Arm exactly one retry for a previously denied Guardian assessment.
    ApproveGuardianDeniedAction {
        assessment_id: String,
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    /// Run an explicit user-authored login-shell command outside the agent sandbox.
    RunUserShellCommand {
        command: String,
        cwd: Option<std::path::PathBuf>,
        reply: tokio::sync::oneshot::Sender<Result<UserShellLaunch, String>>,
    },
    RefreshMcpServers,
    ReloadUserConfig,
    Compact,
    ThreadRollback {
        num_turns: u32,
    },
    Review {
        review_request: ReviewRequest,
    },
    InterAgentCommunication {
        communication: InterAgentCommunication,
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
