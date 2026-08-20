//! Strict Codex V2 Agent Thread execution boundary.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use subagents::{
    AgentThreadV2, InterruptAgentV2Request, InterruptAgentV2Result, ListAgentsV2Request,
    MessageAgentV2Request, MessageAgentV2Result, SpawnAgentV2Request, SpawnAgentV2Result,
    WaitAgentV2Request, WaitAgentV2Result,
};

/// Non-serializable parent runtime material accompanying model-visible Agent
/// Thread requests. Credentials and process-local dependencies must never
/// enter the tool schema or durable graph.
#[derive(Clone)]
pub struct ParentRuntimeMaterial {
    pub memory_dir: PathBuf,
    pub parent_agent_id: String,
    pub parent_model: Option<String>,
    pub parent_sandbox_mode: String,
    pub inherited_skill_config: Vec<(PathBuf, bool)>,
    pub chat_targets: Vec<types::ChatTarget>,
    pub project_root: Option<PathBuf>,
    pub hook_runtime: Option<Arc<hooks::HookRuntime>>,
    pub hook_bus: Option<Arc<hooks::PluginHookBus>>,
}

#[derive(Clone)]
pub struct SpawnAgentDispatchRequest {
    pub request: SpawnAgentV2Request,
    pub runtime: ParentRuntimeMaterial,
}

#[derive(Clone)]
pub struct FollowupAgentDispatchRequest {
    pub request: MessageAgentV2Request,
    /// Desktop callers do not own the active parent model credentials. They
    /// may use a process-local registered runtime, but cannot cold-recover it.
    pub runtime: Option<ParentRuntimeMaterial>,
}

impl From<MessageAgentV2Request> for FollowupAgentDispatchRequest {
    fn from(request: MessageAgentV2Request) -> Self {
        Self {
            request,
            runtime: None,
        }
    }
}

/// The six model-visible Codex V2 Agent Thread operations.
/// Desktop reads and recursive closes deliberately live outside this trait.
#[async_trait]
pub trait AgentThreadDispatch: Send + Sync {
    async fn spawn_agent(
        &self,
        request: SpawnAgentDispatchRequest,
    ) -> anyhow::Result<SpawnAgentV2Result>;

    async fn list_agents(&self, request: ListAgentsV2Request)
        -> anyhow::Result<Vec<AgentThreadV2>>;

    async fn send_message(
        &self,
        request: MessageAgentV2Request,
    ) -> anyhow::Result<MessageAgentV2Result>;

    /// The serialized model request is paired with process-local runtime
    /// material at the tool boundary; credentials never enter the schema.
    async fn followup_task(
        &self,
        request: FollowupAgentDispatchRequest,
    ) -> anyhow::Result<MessageAgentV2Result>;

    async fn wait_agent(&self, request: WaitAgentV2Request) -> anyhow::Result<WaitAgentV2Result>;

    async fn interrupt_agent(
        &self,
        request: InterruptAgentV2Request,
    ) -> anyhow::Result<InterruptAgentV2Result>;
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    struct SixOperationDispatch;

    #[async_trait]
    impl AgentThreadDispatch for SixOperationDispatch {
        async fn spawn_agent(
            &self,
            _request: SpawnAgentDispatchRequest,
        ) -> anyhow::Result<SpawnAgentV2Result> {
            unreachable!()
        }

        async fn list_agents(
            &self,
            _request: ListAgentsV2Request,
        ) -> anyhow::Result<Vec<AgentThreadV2>> {
            unreachable!()
        }

        async fn send_message(
            &self,
            _request: MessageAgentV2Request,
        ) -> anyhow::Result<MessageAgentV2Result> {
            unreachable!()
        }

        async fn followup_task(
            &self,
            _request: FollowupAgentDispatchRequest,
        ) -> anyhow::Result<MessageAgentV2Result> {
            unreachable!()
        }

        async fn wait_agent(
            &self,
            _request: WaitAgentV2Request,
        ) -> anyhow::Result<WaitAgentV2Result> {
            unreachable!()
        }

        async fn interrupt_agent(
            &self,
            _request: InterruptAgentV2Request,
        ) -> anyhow::Result<InterruptAgentV2Result> {
            unreachable!()
        }
    }

    #[test]
    fn public_dispatch_contract_has_exactly_six_operations() {
        fn assert_dispatch<T: AgentThreadDispatch>() {}
        assert_dispatch::<SixOperationDispatch>();
    }
}
