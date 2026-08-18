//! Codex-style first-class subagent threads.
//!
//! The crate owns durable thread metadata, custom-agent configuration and live
//! lifecycle controls. The actual model/tool loop is implemented by
//! `agent::exec::subagents` to avoid a dependency cycle with the main runtime.

mod activity;
mod config;
mod control;
mod mailbox;
mod migration;
mod model;
pub mod path;
mod registry;
mod store;

pub use activity::{ActivityBus, ActivityCursor, AgentActivity, AgentActivityKind};
pub use config::{
    load_agent_catalog, load_agents_settings, resolve_agent, AgentCatalog, AgentConfigDiagnostic,
    AgentDefinition, AgentsSettings, ResolvedAgent, SkillConfigEntry, SkillsLayer,
};
pub use control::{
    AgentControl, AgentRuntimeHandle, AgentSpawnReservation, AgentThreadCommand,
    AgentThreadControl, CloseAdmissionGuard, LiveAgentThreads, RuntimeHandleRegistry,
    WaitAgentResult, WaitOutcome,
};
pub use mailbox::{MailboxKind, MailboxMessage, NewMailboxMessage};
pub use migration::{HistoricalAgentMessage, HistoricalAgentThread};
pub use model::{
    AgentStatusKind, AgentStatusV2, AgentThreadDetailV2, AgentThreadMessageV2, AgentThreadV2,
    AgentTreeSnapshotV2, InterruptAgentV2Request, InterruptAgentV2Result, ListAgentsV2Request,
    MessageAgentV2Request, MessageAgentV2Result, RunnerEvent, SpawnAgentV2Request,
    SpawnAgentV2Result, SpawnRuntimeV2Request, ThreadReservation, WaitAgentV2Request,
    WaitAgentV2Result,
};
// Transitional legacy exports; removed in Tasks 6/10.
pub use model::{
    AgentThread, AgentThreadMessage, AgentThreadStatus, CloseAgentRequest, InterruptAgentRequest,
    ListAgentThreadsRequest, ReadAgentThreadRequest, SendAgentMessageRequest, SpawnAgentRequest,
    WaitAgentThreadsRequest,
};
pub use path::AgentPath;
pub use registry::{AgentRegistry, ExecutionPermit, Limits, SpawnReservation};
pub use store::{AgentGraphStore, AgentThreadStore, StoredStatusEvent};
