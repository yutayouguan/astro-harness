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
        LiveAgentThreads::global().ensure_capacity(
            &request.parent_session_id,
            settings.max_concurrent_threads_per_session,
        )?;

        let thread = store.create(&request)?;
        let (control, receiver) = match LiveAgentThreads::global().register_bounded(
            &thread.id,
            &request.parent_session_id,
            settings.max_concurrent_threads_per_session,
        ) {
            Ok(registered) => registered,
            Err(error) => {
                store.set_status(
                    &thread.id,
                    AgentThreadStatus::Failed,
                    None,
                    Some(&error.to_string()),
                )?;
                return Err(error);
            }
        };
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
        enqueue_follow_up(&store, LiveAgentThreads::global(), request)
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

fn enqueue_follow_up(
    store: &AgentThreadStore,
    live_threads: &LiveAgentThreads,
    request: SendAgentMessageRequest,
) -> anyhow::Result<AgentThread> {
    let message = request.message.trim();
    if message.is_empty() {
        anyhow::bail!("subagent follow-up message cannot be empty");
    }

    let thread = store
        .get(&request.thread_id)?
        .ok_or_else(|| anyhow::anyhow!("unknown agent thread: {}", request.thread_id))?;
    if thread.status == AgentThreadStatus::Closed {
        anyhow::bail!("agent thread is closed: {}", request.thread_id);
    }

    // The runner persists this message only after the current assistant turn
    // has finished, keeping the durable transcript in conversational order.
    live_threads.send_follow_up(&request.thread_id, message.to_string())?;
    store
        .get(&request.thread_id)?
        .ok_or_else(|| anyhow::anyhow!("agent thread disappeared: {}", request.thread_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use subagents::AgentThreadCommand;

    fn spawn_request() -> SpawnAgentRequest {
        SpawnAgentRequest {
            parent_session_id: "parent-session".into(),
            parent_agent_id: "parent-agent".into(),
            task: "inspect the project".into(),
            agent_name: "reviewer".into(),
            developer_instructions: String::new(),
            context_snapshot: String::new(),
            model: None,
            model_reasoning_effort: None,
            sandbox_mode: None,
            chat_targets: Vec::new(),
            project_root: None,
            hook_bus: None,
            interrupt_message: false,
        }
    }

    #[tokio::test]
    async fn follow_up_is_persisted_after_current_assistant_message() {
        let temp = tempfile::tempdir().unwrap();
        let store = AgentThreadStore::new(temp.path().join("subagents.db")).unwrap();
        let thread = store.create(&spawn_request()).unwrap();
        store
            .set_status(&thread.id, AgentThreadStatus::Running, None, None)
            .unwrap();
        let live_threads = LiveAgentThreads::default();
        let (_control, mut commands) = live_threads
            .register_bounded(&thread.id, &thread.parent_session_id, 1)
            .unwrap();

        let returned = enqueue_follow_up(
            &store,
            &live_threads,
            SendAgentMessageRequest {
                thread_id: thread.id.clone(),
                message: "  check the tests too  ".into(),
            },
        )
        .unwrap();
        assert_eq!(returned.status, AgentThreadStatus::Running);
        assert_eq!(store.messages(&thread.id).unwrap().len(), 1);

        store
            .append_message(&thread.id, "assistant", "first answer")
            .unwrap();
        let Some(AgentThreadCommand::FollowUp(message)) = commands.recv().await else {
            panic!("expected queued follow-up");
        };
        crate::exec::subagents::record_follow_up(&store, &thread.id, &message).unwrap();

        let messages = store.messages(&thread.id).unwrap();
        let transcript = messages
            .iter()
            .map(|message| (message.role.as_str(), message.content.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            transcript,
            vec![
                ("user", "inspect the project"),
                ("assistant", "first answer"),
                ("user", "check the tests too"),
            ]
        );
    }

    #[test]
    fn empty_follow_up_is_rejected_without_changing_transcript() {
        let temp = tempfile::tempdir().unwrap();
        let store = AgentThreadStore::new(temp.path().join("subagents.db")).unwrap();
        let thread = store.create(&spawn_request()).unwrap();
        let live_threads = LiveAgentThreads::default();
        let (_control, _commands) = live_threads
            .register_bounded(&thread.id, &thread.parent_session_id, 1)
            .unwrap();

        let error = enqueue_follow_up(
            &store,
            &live_threads,
            SendAgentMessageRequest {
                thread_id: thread.id.clone(),
                message: "  \n  ".into(),
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("cannot be empty"));
        assert_eq!(store.messages(&thread.id).unwrap().len(), 1);
    }
}
