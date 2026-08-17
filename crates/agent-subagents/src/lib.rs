//! Codex-style first-class subagent threads.
//!
//! The crate owns durable thread metadata, custom-agent configuration and live
//! lifecycle controls. The actual model/tool loop is implemented by
//! `agent::exec::subagents` to avoid a dependency cycle with the main runtime.

mod config;
mod control;
mod model;
mod store;

pub use config::{
    load_agent_catalog, load_agents_settings, resolve_agent, AgentCatalog, AgentConfigDiagnostic,
    AgentDefinition, AgentsSettings, ResolvedAgent,
};
pub use control::{AgentThreadCommand, AgentThreadControl, LiveAgentThreads};
pub use model::{
    AgentThread, AgentThreadMessage, AgentThreadStatus, CloseAgentRequest, InterruptAgentRequest,
    ListAgentThreadsRequest, ReadAgentThreadRequest, SendAgentMessageRequest, SpawnAgentRequest,
    WaitAgentThreadsRequest,
};
pub use store::AgentThreadStore;
