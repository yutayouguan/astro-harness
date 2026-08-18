//! Codex-style first-class subagent threads.
//!
//! The crate owns durable thread metadata, custom-agent configuration and live
//! lifecycle controls. The actual model/tool loop is implemented by
//! `agent::exec::subagents` to avoid a dependency cycle with the main runtime.

mod config;
mod control;
mod model;
pub mod path;
mod store;

pub use config::{
    load_agent_catalog, load_agents_settings, resolve_agent, AgentCatalog, AgentConfigDiagnostic,
    AgentDefinition, AgentsSettings, ResolvedAgent, SkillConfigEntry, SkillsLayer,
};
pub use control::{AgentThreadCommand, AgentThreadControl, LiveAgentThreads};
pub use model::{
    AgentStatusKind, AgentStatusV2, AgentThread, AgentThreadMessage, AgentThreadStatus,
    AgentThreadV2, AgentTreeSnapshotV2, CloseAgentRequest, InterruptAgentRequest,
    InterruptAgentV2Request, InterruptAgentV2Result, ListAgentThreadsRequest, ListAgentsV2Request,
    MessageAgentV2Request, MessageAgentV2Result, ReadAgentThreadRequest, RunnerEvent,
    SendAgentMessageRequest, SpawnAgentRequest, SpawnAgentV2Request, SpawnAgentV2Result,
    SpawnRuntimeV2Request, ThreadReservation, WaitAgentThreadsRequest, WaitAgentV2Request,
    WaitAgentV2Result,
};
pub use path::AgentPath;
pub use store::AgentThreadStore;
