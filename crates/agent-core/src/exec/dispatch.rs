//! Default first-class subagent thread dispatch implementation.

use std::time::Duration;

use async_trait::async_trait;
use subagents::{
    AgentThread, AgentThreadMessage, AgentThreadStatus, AgentThreadStore, CloseAgentRequest,
    InterruptAgentRequest, ListAgentThreadsRequest, LiveAgentThreads, ReadAgentThreadRequest,
    SendAgentMessageRequest, SpawnAgentRequest, WaitAgentThreadsRequest,
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
            .list(Some(&request.parent_session_id), request.include_closed)
    }

    async fn read_agent(
        &self,
        request: ReadAgentThreadRequest,
    ) -> anyhow::Result<(AgentThread, Vec<AgentThreadMessage>)> {
        let store = AgentThreadStore::open_default()?;
        read_owned_agent(&store, &request)
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
        let store = AgentThreadStore::open_default()?;
        wait_owned_agents(&store, &request).await
    }

    async fn interrupt_agent(&self, request: InterruptAgentRequest) -> anyhow::Result<AgentThread> {
        let store = AgentThreadStore::open_default()?;
        interrupt_owned_agent(&store, LiveAgentThreads::global(), &request)
    }

    async fn close_agent(&self, request: CloseAgentRequest) -> anyhow::Result<AgentThread> {
        let store = AgentThreadStore::open_default()?;
        close_owned_agent(&store, LiveAgentThreads::global(), &request)
    }
}

fn require_owned_thread(
    store: &AgentThreadStore,
    parent_session_id: &str,
    thread_id: &str,
) -> anyhow::Result<AgentThread> {
    store
        .get(thread_id)?
        .filter(|thread| thread.parent_session_id == parent_session_id)
        .ok_or_else(|| anyhow::anyhow!("unknown agent thread: {thread_id}"))
}

fn read_owned_agent(
    store: &AgentThreadStore,
    request: &ReadAgentThreadRequest,
) -> anyhow::Result<(AgentThread, Vec<AgentThreadMessage>)> {
    let thread = require_owned_thread(store, &request.parent_session_id, &request.thread_id)?;
    let messages = store.messages(&request.thread_id)?;
    Ok((thread, messages))
}

async fn wait_owned_agents(
    store: &AgentThreadStore,
    request: &WaitAgentThreadsRequest,
) -> anyhow::Result<Vec<AgentThread>> {
    for thread_id in &request.thread_ids {
        require_owned_thread(store, &request.parent_session_id, thread_id)?;
    }
    store
        .wait(
            &request.thread_ids,
            Duration::from_millis(request.timeout_ms),
        )
        .await
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

    let thread = require_owned_thread(store, &request.parent_session_id, &request.thread_id)?;
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

fn interrupt_owned_agent(
    store: &AgentThreadStore,
    live_threads: &LiveAgentThreads,
    request: &InterruptAgentRequest,
) -> anyhow::Result<AgentThread> {
    let thread = require_owned_thread(store, &request.parent_session_id, &request.thread_id)?;
    if matches!(
        thread.status,
        AgentThreadStatus::Pending | AgentThreadStatus::Running
    ) {
        live_threads.interrupt(&request.thread_id)?;
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

fn close_owned_agent(
    store: &AgentThreadStore,
    live_threads: &LiveAgentThreads,
    request: &CloseAgentRequest,
) -> anyhow::Result<AgentThread> {
    require_owned_thread(store, &request.parent_session_id, &request.thread_id)?;
    if live_threads.is_live(&request.thread_id) {
        live_threads.close(&request.thread_id)?;
    }
    store.set_status(&request.thread_id, AgentThreadStatus::Closed, None, None)?;
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
                parent_session_id: thread.parent_session_id.clone(),
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
                parent_session_id: thread.parent_session_id.clone(),
                thread_id: thread.id.clone(),
                message: "  \n  ".into(),
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("cannot be empty"));
        assert_eq!(store.messages(&thread.id).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn thread_management_is_scoped_to_parent_session() {
        let temp = tempfile::tempdir().unwrap();
        let store = AgentThreadStore::new(temp.path().join("subagents.db")).unwrap();
        let thread = store.create(&spawn_request()).unwrap();
        store
            .set_status(&thread.id, AgentThreadStatus::Running, None, None)
            .unwrap();
        let live_threads = LiveAgentThreads::default();
        let (control, mut commands) = live_threads
            .register_bounded(&thread.id, &thread.parent_session_id, 1)
            .unwrap();
        let foreign_session = "different-parent".to_string();

        assert_eq!(
            store
                .list(Some(&thread.parent_session_id), true)
                .unwrap()
                .len(),
            1
        );
        assert!(store.list(Some(&foreign_session), true).unwrap().is_empty());

        let read_error = read_owned_agent(
            &store,
            &ReadAgentThreadRequest {
                parent_session_id: foreign_session.clone(),
                thread_id: thread.id.clone(),
            },
        )
        .unwrap_err();
        assert!(read_error.to_string().contains("unknown agent thread"));

        let send_error = enqueue_follow_up(
            &store,
            &live_threads,
            SendAgentMessageRequest {
                parent_session_id: foreign_session.clone(),
                thread_id: thread.id.clone(),
                message: "foreign follow-up".into(),
            },
        )
        .unwrap_err();
        assert!(send_error.to_string().contains("unknown agent thread"));
        assert!(commands.try_recv().is_err());

        let wait_error = wait_owned_agents(
            &store,
            &WaitAgentThreadsRequest {
                parent_session_id: foreign_session.clone(),
                thread_ids: vec![thread.id.clone()],
                timeout_ms: 0,
            },
        )
        .await
        .unwrap_err();
        assert!(wait_error.to_string().contains("unknown agent thread"));

        let interrupt_error = interrupt_owned_agent(
            &store,
            &live_threads,
            &InterruptAgentRequest {
                parent_session_id: foreign_session.clone(),
                thread_id: thread.id.clone(),
            },
        )
        .unwrap_err();
        assert!(interrupt_error.to_string().contains("unknown agent thread"));
        assert!(!control.is_interrupted());

        let close_error = close_owned_agent(
            &store,
            &live_threads,
            &CloseAgentRequest {
                parent_session_id: foreign_session,
                thread_id: thread.id.clone(),
            },
        )
        .unwrap_err();
        assert!(close_error.to_string().contains("unknown agent thread"));
        assert!(!control.is_closed());

        let owner = thread.parent_session_id.clone();
        assert!(read_owned_agent(
            &store,
            &ReadAgentThreadRequest {
                parent_session_id: owner.clone(),
                thread_id: thread.id.clone(),
            }
        )
        .is_ok());
        assert!(wait_owned_agents(
            &store,
            &WaitAgentThreadsRequest {
                parent_session_id: owner.clone(),
                thread_ids: vec![thread.id.clone()],
                timeout_ms: 0,
            }
        )
        .await
        .is_ok());
        assert!(enqueue_follow_up(
            &store,
            &live_threads,
            SendAgentMessageRequest {
                parent_session_id: owner.clone(),
                thread_id: thread.id.clone(),
                message: "owner follow-up".into(),
            }
        )
        .is_ok());
        assert!(matches!(
            commands.try_recv(),
            Ok(AgentThreadCommand::FollowUp(message)) if message == "owner follow-up"
        ));
        assert!(interrupt_owned_agent(
            &store,
            &live_threads,
            &InterruptAgentRequest {
                parent_session_id: owner.clone(),
                thread_id: thread.id.clone(),
            }
        )
        .is_ok());
        assert!(control.is_interrupted());
        assert!(close_owned_agent(
            &store,
            &live_threads,
            &CloseAgentRequest {
                parent_session_id: owner,
                thread_id: thread.id,
            }
        )
        .is_ok());
        assert!(control.is_closed());
    }
}
