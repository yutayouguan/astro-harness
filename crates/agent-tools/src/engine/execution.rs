//! V2 Agent Thread 严格执行边界。

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use subagents::{
    AgentThreadV2, InterruptAgentV2Request, InterruptAgentV2Result, ListAgentsV2Request,
    MessageAgentV2Request, MessageAgentV2Result, SpawnAgentV2Request, SpawnAgentV2Result,
    WaitAgentV2Request, WaitAgentV2Result,
};

/// 伴随模型可见的 Agent Thread 请求的不可序列化父运行时材料。
/// 凭证和进程局部依赖绝不能进入工具 schema 或持久化图。
#[derive(Clone)]
pub struct ParentRuntimeMaterial {
    pub memory_dir: PathBuf,
    pub parent_agent_id: String,
    pub parent_model: Option<String>,
    pub root_service_tier: Option<String>,
    pub parent_sandbox_mode: String,
    pub inherited_skill_config: Vec<(PathBuf, bool)>,
    pub model_targets: Vec<types::ModelTarget>,
    pub model_spec: Option<types::ModelSpec>,
    pub project_root: Option<PathBuf>,
    pub workspace_roots: Vec<PathBuf>,
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
    /// 桌面调用方不拥有活跃的父模型凭证。
    /// 可使用进程局部注册的运行时，但无法冷恢复。
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

/// 六个模型可见的 V2 Agent Thread 操作。
/// 桌面端读取和递归关闭操作刻意不在此 trait 中。
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

    /// 序列化的模型请求在工具边界与进程局部运行时材料配对；凭证绝不进入 schema。
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
