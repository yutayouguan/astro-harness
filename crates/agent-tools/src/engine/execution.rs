//! First-class subagent thread dispatch contract.

use async_trait::async_trait;
use subagents::{
    AgentThread, AgentThreadMessage, CloseAgentRequest, InterruptAgentRequest,
    ListAgentThreadsRequest, SendAgentMessageRequest, SpawnAgentRequest, WaitAgentThreadsRequest,
};

/// Runtime boundary used by the model tools and desktop commands.
///
/// `agent-core` owns the actual model/tool loop; this trait keeps `agent-tools`
/// independent from the runtime implementation and makes lifecycle behavior
/// testable.
#[async_trait]
pub trait AgentThreadDispatch: Send + Sync {
    async fn spawn_agent(&self, request: SpawnAgentRequest) -> anyhow::Result<AgentThread>;

    async fn list_agents(
        &self,
        request: ListAgentThreadsRequest,
    ) -> anyhow::Result<Vec<AgentThread>>;

    async fn read_agent(&self, thread_id: &str) -> anyhow::Result<(AgentThread, Vec<AgentThreadMessage>)>;

    async fn send_message(&self, request: SendAgentMessageRequest) -> anyhow::Result<AgentThread>;

    async fn wait_agents(
        &self,
        request: WaitAgentThreadsRequest,
    ) -> anyhow::Result<Vec<AgentThread>>;

    async fn interrupt_agent(&self, request: InterruptAgentRequest) -> anyhow::Result<AgentThread>;

    async fn close_agent(&self, request: CloseAgentRequest) -> anyhow::Result<AgentThread>;
}
