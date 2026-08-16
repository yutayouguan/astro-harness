//! Default first-class subagent thread dispatch implementation.

use std::time::Duration;

use async_trait::async_trait;
use subagents::{
    AgentThread, AgentThreadMessage, AgentThreadStatus, AgentThreadStore, CloseAgentRequest,
    InterruptAgentRequest, ListAgentThreadsRequest, LiveAgentThreads, SendAgentMessageRequest,
    SpawnAgentRequest, WaitAgentThreadsRequest,
};
use tools::AgentThreadDispatch;

pub struct DefaultAgentThreadDispatch;

#[async_trait]
impl AgentThreadDispatch for DefaultAgentThreadDispatch {
    async fn spawn_agent(&self, request: SpawnAgentRequest) -> anyhow::Result<AgentThread> {
        let store = AgentThreadStore::open_default()?;
        let settings = subagents::load_agents_settings(
            &home::default_memory_dir(),
            request.project_root.as_deref(),
        );
        if !settings.enabled {
            anyhow::bail!("subagent threads are disabled");
        }
        let active = store.count_active(&request.parent_session_id)?;
        if active >= settings.max_concurrent_threads_per_session {
            anyhow::bail!(
                "subagent concurrency limit reached ({active}/{})",
                settings.max_concurrent_threads_per_session
            );
        }

        let thread = store.create(&request)?;
        let (control, receiver) = LiveAgentThreads::global().register(&thread.id);
        let thread_id = thread.id.clone();
        tokio::task::spawn_blocking(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    tracing::warn!(thread_id, %error, "failed to create subagent runtime");
                    return;
                }
            };
            let result = runtime.block_on(crate::exec::subagents::run_agent_thread(
                thread_id.clone(),
                request,
                control,
                receiver,
            ));
            if let Err(error) = result {
                tracing::warn!(thread_id, %error, "subagent thread runner failed");
                LiveAgentThreads::global().remove(&thread_id);
                if let Ok(store) = AgentThreadStore::open_default() {
                    let _ = store.set_status(
                        &thread_id,
                        AgentThreadStatus::Failed,
                        None,
                        Some(&error.to_string()),
                    );
                }
            }
        });
        Ok(thread)
    }

    async fn list_agents(
        &self,
        request: ListAgentThreadsRequest,
    ) -> anyhow::Result<Vec<AgentThread>> {
        AgentThreadStore::open_default()?
            .list(request.parent_session_id.as_deref(), request.include_closed)
    }

    async fn read_agent(
        &self,
        thread_id: &str,
    ) -> anyhow::Result<(AgentThread, Vec<AgentThreadMessage>)> {
        let store = AgentThreadStore::open_default()?;
        let thread = store
            .get(thread_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown agent thread: {thread_id}"))?;
        let messages = store.messages(thread_id)?;
        Ok((thread, messages))
    }

    async fn send_message(&self, request: SendAgentMessageRequest) -> anyhow::Result<AgentThread> {
        let store = AgentThreadStore::open_default()?;
        let thread = store
            .get(&request.thread_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown agent thread: {}", request.thread_id))?;
        if thread.status == AgentThreadStatus::Closed {
            anyhow::bail!("agent thread is closed: {}", request.thread_id);
        }
        if !LiveAgentThreads::global().is_live(&request.thread_id) {
            anyhow::bail!("agent thread is not live: {}", request.thread_id);
        }
        store.append_message(&request.thread_id, "user", &request.message)?;
        store.set_status(&request.thread_id, AgentThreadStatus::Pending, None, None)?;
        LiveAgentThreads::global().send_follow_up(&request.thread_id, request.message)?;
        store
            .get(&request.thread_id)?
            .ok_or_else(|| anyhow::anyhow!("agent thread disappeared: {}", request.thread_id))
    }

    async fn wait_agents(
        &self,
        request: WaitAgentThreadsRequest,
    ) -> anyhow::Result<Vec<AgentThread>> {
        if request.thread_ids.is_empty() {
            return Ok(Vec::new());
        }
        AgentThreadStore::open_default()?
            .wait(
                &request.thread_ids,
                Duration::from_millis(request.timeout_ms),
            )
            .await
    }

    async fn interrupt_agent(&self, request: InterruptAgentRequest) -> anyhow::Result<AgentThread> {
        let store = AgentThreadStore::open_default()?;
        let thread = store
            .get(&request.thread_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown agent thread: {}", request.thread_id))?;
        if matches!(
            thread.status,
            AgentThreadStatus::Pending | AgentThreadStatus::Running
        ) {
            LiveAgentThreads::global().interrupt(&request.thread_id)?;
            store.set_status(
                &request.thread_id,
                AgentThreadStatus::Interrupted,
                None,
                Some("interrupted by parent"),
            )?;
        }
        store
            .get(&request.thread_id)?
            .ok_or_else(|| anyhow::anyhow!("agent thread disappeared: {}", request.thread_id))
    }

    async fn close_agent(&self, request: CloseAgentRequest) -> anyhow::Result<AgentThread> {
        let store = AgentThreadStore::open_default()?;
        store
            .get(&request.thread_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown agent thread: {}", request.thread_id))?;
        if LiveAgentThreads::global().is_live(&request.thread_id) {
            LiveAgentThreads::global().close(&request.thread_id)?;
        }
        store.set_status(&request.thread_id, AgentThreadStatus::Closed, None, None)?;
        store
            .get(&request.thread_id)?
            .ok_or_else(|| anyhow::anyhow!("agent thread disappeared: {}", request.thread_id))
    }
}
