use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use subagents::{
    AgentRuntimeHandle, AgentStatusV2, AgentThreadControl, AgentThreadV2, RunnerEvent,
    SpawnRuntimeV2Request,
};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::runtime::{Config, Session};
use crate::streaming::ChatOverride;
use crate::tasks::TurnInput;

pub struct RunAgentTurnRequest {
    pub control: Arc<subagents::AgentControl>,
    pub thread: AgentThreadV2,
    pub runtime: SpawnRuntimeV2Request,
    pub memory_dir: PathBuf,
    pub chat_override: Option<ChatOverride>,
}

#[derive(Debug, Clone)]
pub struct RunnerTermination {
    pub terminal_status: AgentStatusV2,
}

struct ActiveAgentTurn {
    interrupt: Arc<AgentThreadControl>,
    terminated: Option<oneshot::Receiver<RunnerTermination>>,
}

#[derive(Default)]
pub struct AgentRuntimeManager {
    active: Mutex<HashMap<String, ActiveAgentTurn>>,
}

impl AgentRuntimeManager {
    pub async fn start_turn(&self, request: RunAgentTurnRequest) -> anyhow::Result<()> {
        let thread_id = request.thread.thread_id.clone();
        let control = Arc::clone(&request.control);
        let _permit = control.acquire_execution(&thread_id)?;
        let turn_id = Uuid::new_v4().to_string();
        let interrupt = Arc::new(AgentThreadControl::default());
        interrupt.begin_turn();
        let (terminated_tx, terminated_rx) = oneshot::channel();

        {
            let mut active = self.lock_active()?;
            if active.contains_key(&thread_id) {
                anyhow::bail!("agent thread {thread_id:?} already has an active runtime turn");
            }
            active.insert(
                thread_id.clone(),
                ActiveAgentTurn {
                    interrupt: Arc::clone(&interrupt),
                    terminated: Some(terminated_rx),
                },
            );
        }

        if let Err(error) = control.record_runner_event(
            &thread_id,
            RunnerEvent::TurnStarted {
                turn_id: turn_id.clone(),
            },
        ) {
            self.remove_active(&thread_id);
            return Err(error);
        }

        let runtime_interrupt = Arc::clone(&interrupt);
        let runtime_terminate = Arc::clone(&interrupt);
        if let Err(error) = control.register_runtime(
            &thread_id,
            AgentRuntimeHandle {
                interrupt: Arc::new(move || runtime_interrupt.interrupt()),
                terminate: Arc::new(move || runtime_terminate.close()),
            },
        ) {
            let error = error.context("register agent runtime handle");
            self.finish_failed_start(
                &control,
                &thread_id,
                &turn_id,
                error.to_string(),
                terminated_tx,
            )?;
            return Err(error);
        }

        let result = run_request(&request, Arc::clone(&interrupt)).await;
        let (event, terminal_status) = if interrupt.is_closed() || interrupt.is_interrupted() {
            if request.runtime.interrupt_message {
                if let Err(error) = ensure_interrupted_history_boundary(
                    &request.memory_dir,
                    &request.thread.session_id,
                ) {
                    tracing::warn!(%error, "failed to record interrupted agent history boundary");
                }
            }
            (
                RunnerEvent::TurnInterrupted {
                    turn_id: turn_id.clone(),
                    reason: if interrupt.is_closed() {
                        "runtime terminated by parent".into()
                    } else {
                        "interrupted by parent".into()
                    },
                },
                AgentStatusV2::Interrupted,
            )
        } else {
            match &result {
                Ok(output) => (
                    RunnerEvent::TurnCompleted {
                        turn_id: turn_id.clone(),
                        last_message: types::truncate_chars(output, 8_000),
                    },
                    AgentStatusV2::Completed {
                        last_message: types::truncate_chars(output, 8_000),
                    },
                ),
                Err(error) => (
                    RunnerEvent::TurnErrored {
                        turn_id: turn_id.clone(),
                        message: error.to_string(),
                    },
                    AgentStatusV2::Errored {
                        message: error.to_string(),
                    },
                ),
            }
        };

        let finish_result = control.record_runner_event(&thread_id, event);
        let shutdown_result = if finish_result.is_ok() && interrupt.is_closed() {
            control
                .record_runner_event(&thread_id, RunnerEvent::RuntimeTerminated)
                .map(|_| ())
        } else {
            Ok(())
        };
        let _ = control.remove_runtime(&thread_id);
        self.remove_active(&thread_id);
        finish_result?;
        shutdown_result?;
        let _ = terminated_tx.send(RunnerTermination {
            terminal_status: terminal_status.clone(),
        });

        match result {
            Err(error) if !interrupt.is_interrupted() && !interrupt.is_closed() => Err(error),
            _ => Ok(()),
        }
    }

    pub async fn interrupt(&self, thread_id: &str) -> anyhow::Result<AgentStatusV2> {
        let (interrupt, terminated) = self.take_termination_receiver(thread_id)?;
        interrupt.interrupt();
        let termination = terminated
            .await
            .map_err(|_| anyhow::anyhow!("agent runtime ended without interruption ack"))?;
        Ok(termination.terminal_status)
    }

    pub async fn terminate(&self, thread_id: &str) -> anyhow::Result<()> {
        let (interrupt, terminated) = self.take_termination_receiver(thread_id)?;
        interrupt.close();
        terminated
            .await
            .map_err(|_| anyhow::anyhow!("agent runtime ended without termination ack"))?;
        Ok(())
    }

    pub fn is_running(&self, thread_id: &str) -> bool {
        self.active
            .lock()
            .map(|active| active.contains_key(thread_id))
            .unwrap_or(false)
    }

    fn take_termination_receiver(
        &self,
        thread_id: &str,
    ) -> anyhow::Result<(
        Arc<AgentThreadControl>,
        oneshot::Receiver<RunnerTermination>,
    )> {
        let mut active = self.lock_active()?;
        let turn = active.get_mut(thread_id).ok_or_else(|| {
            anyhow::anyhow!("agent thread {thread_id:?} has no active runtime turn")
        })?;
        let terminated = turn
            .terminated
            .take()
            .ok_or_else(|| anyhow::anyhow!("agent runtime acknowledgement is already awaited"))?;
        Ok((Arc::clone(&turn.interrupt), terminated))
    }

    fn finish_failed_start(
        &self,
        control: &subagents::AgentControl,
        thread_id: &str,
        turn_id: &str,
        message: String,
        terminated_tx: oneshot::Sender<RunnerTermination>,
    ) -> anyhow::Result<()> {
        let status = AgentStatusV2::Errored {
            message: message.clone(),
        };
        let event_result = control.record_runner_event(
            thread_id,
            RunnerEvent::TurnErrored {
                turn_id: turn_id.to_string(),
                message,
            },
        );
        let _ = control.remove_runtime(thread_id);
        self.remove_active(thread_id);
        event_result?;
        let _ = terminated_tx.send(RunnerTermination {
            terminal_status: status,
        });
        Ok(())
    }

    fn remove_active(&self, thread_id: &str) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(thread_id);
        }
    }

    fn lock_active(
        &self,
    ) -> anyhow::Result<std::sync::MutexGuard<'_, HashMap<String, ActiveAgentTurn>>> {
        self.active
            .lock()
            .map_err(|_| anyhow::anyhow!("agent runtime manager mutex is poisoned"))
    }
}

fn ensure_interrupted_history_boundary(
    memory_dir: &std::path::Path,
    session_id: &str,
) -> anyhow::Result<()> {
    let sessions = session::SessionStore::open_sessions_dir(&memory_dir.join("sessions"))?;
    let messages = sessions.get_messages(session_id)?;
    if messages
        .last()
        .is_some_and(|message| message.role == "user")
    {
        sessions.append_message(session::NewMessage {
            content: Some("[astro:system]\nThe previous agent turn was interrupted by the parent."),
            finish_reason: Some("interrupted"),
            ..session::NewMessage::empty(session_id, "assistant")
        })?;
    }
    Ok(())
}

async fn run_request(
    request: &RunAgentTurnRequest,
    interrupt: Arc<AgentThreadControl>,
) -> anyhow::Result<String> {
    let mut config = Config::with_defaults(request.memory_dir.clone());
    config.soul = format!(
        "{}\n\n## Subagent developer instructions\n{}",
        config.soul, request.runtime.developer_instructions
    );
    if let Some(effort) = request.runtime.model_request.reasoning_effort.as_deref() {
        config.additional_params = serde_json::json!({ "reasoning_effort": effort });
    }
    let mut session = Session::with_session_id_for_agent_thread(
        config,
        request.thread.session_id.clone(),
        &request.runtime.parent_agent_id,
        Arc::clone(&request.control),
        request.thread.canonical_path.clone(),
    )?;
    session.set_project_root(request.runtime.project_root.clone());
    session.set_permission_profile(sandbox_profile(request.runtime.sandbox_mode.as_deref()));
    session.set_mcp_config_override(mcp::decode_inline_mcp_servers(
        &request.runtime.mcp_servers,
    )?);
    session.set_skill_config_overrides(
        request
            .runtime
            .skills_config
            .iter()
            .map(|entry| (entry.path.clone(), entry.enabled))
            .collect(),
    );
    if let Some(bus) = request.runtime.hook_bus.as_ref() {
        session.set_hook_bus(Arc::clone(bus));
    }

    let mut targets = request.runtime.chat_targets.clone();
    if let Some(model) = request.runtime.model_request.model.as_deref() {
        let spec = types::ModelSpec::parse(model)?;
        if let Some(primary) = targets.first_mut() {
            *primary = spec.apply_to(primary);
        }
    }
    anyhow::ensure!(!targets.is_empty(), "agent turn has no chat target");
    session.set_chat_targets(targets.clone());
    let session = Arc::new(tokio::sync::Mutex::new(session));
    let (output, _) = crate::exec::background::run_background_multi_turn_controlled_with_chat(
        session,
        targets,
        vec![TurnInput::UserInput {
            content: request.runtime.model_request.message.clone(),
            image_data_urls: Vec::new(),
        }],
        Some(interrupt),
        request.chat_override.clone(),
    )
    .await?;
    Ok(output)
}

fn sandbox_profile(mode: Option<&str>) -> Option<String> {
    mode.map(|value| match value.trim().to_ascii_lowercase().as_str() {
        "read-only" | "read_only" => types::READ_ONLY_PROFILE.to_string(),
        "workspace-write" | "workspace_write" => types::WORKSPACE_PROFILE.to_string(),
        "danger-full-access" | "danger_full_access" => {
            types::DANGER_FULL_ACCESS_PROFILE.to_string()
        }
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use futures::stream;
    use providers::types::stream::StreamChunk;
    use providers::CompletionStream;
    use subagents::{
        AgentControl, AgentGraphStore, AgentPath, AgentStatusV2, Limits, RunnerEvent,
        SpawnAgentV2Request, SpawnRuntimeV2Request,
    };

    use crate::streaming::ChatOverride;

    use super::{AgentRuntimeManager, RunAgentTurnRequest};

    fn scripted_chat(reply: &str) -> ChatOverride {
        let reply = reply.to_string();
        Arc::new(move |_messages, _tools, _config| {
            let reply = reply.clone();
            Box::pin(async move {
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::Text(reply)),
                    Ok(StreamChunk::Done {
                        finish_reason: "stop".into(),
                    }),
                ])) as CompletionStream)
            })
        })
    }

    fn pending_chat() -> ChatOverride {
        Arc::new(move |_messages, _tools, _config| {
            Box::pin(async move {
                Ok(Box::pin(stream::pending::<anyhow::Result<StreamChunk>>()) as CompletionStream)
            })
        })
    }

    fn history_asserting_chat(saw_structured_tool: Arc<AtomicBool>) -> ChatOverride {
        Arc::new(move |messages, _tools, _config| {
            let serialized = serde_json::to_string(&messages).unwrap();
            if serialized.contains("call-1") && serialized.contains("tool result") {
                saw_structured_tool.store(true, Ordering::SeqCst);
            }
            Box::pin(async move {
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::Text("follow-up answer".into())),
                    Ok(StreamChunk::Done {
                        finish_reason: "stop".into(),
                    }),
                ])) as CompletionStream)
            })
        })
    }

    fn setup(
        dir: &tempfile::TempDir,
        task_name: &str,
    ) -> (Arc<AgentControl>, subagents::AgentThreadV2) {
        let store = AgentGraphStore::open(dir.path().join("agents.db")).unwrap();
        let control = AgentControl::open(
            "root".into(),
            store,
            Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 1,
            },
        )
        .unwrap();
        let reservation = control
            .reserve_spawn(&AgentPath::root(), task_name)
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().unwrap();
        (control, thread)
    }

    fn request(
        control: Arc<AgentControl>,
        thread: subagents::AgentThreadV2,
        memory_dir: std::path::PathBuf,
        chat_override: ChatOverride,
    ) -> RunAgentTurnRequest {
        RunAgentTurnRequest {
            control,
            thread,
            runtime: SpawnRuntimeV2Request {
                model_request: SpawnAgentV2Request {
                    task_name: "worker".into(),
                    message: "do the work".into(),
                    agent_type: None,
                    model: None,
                    reasoning_effort: None,
                    fork_turns: None,
                },
                parent_thread_id: "root".into(),
                parent_path: AgentPath::root(),
                root_thread_id: "root".into(),
                parent_session_id: "root".into(),
                parent_agent_id: home::DEFAULT_AGENT_ID.into(),
                developer_instructions: "be concise".into(),
                context_snapshot: String::new(),
                sandbox_mode: Some("read-only".into()),
                mcp_servers: BTreeMap::new(),
                skills_config: Vec::new(),
                chat_targets: vec![types::ChatTarget {
                    provider_id: "test".into(),
                    backend_id: "openai".into(),
                    model: "test".into(),
                    api_key: "test".into(),
                    base_url: "http://127.0.0.1.invalid".into(),
                }],
                project_root: None,
                hook_bus: None,
                interrupt_message: true,
            },
            memory_dir,
            chat_override: Some(chat_override),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn completed_turn_releases_runtime_and_execution_but_keeps_identity() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = AgentRuntimeManager::default();

        manager
            .start_turn(request(
                Arc::clone(&control),
                thread.clone(),
                dir.path().join("memory"),
                scripted_chat("finished"),
            ))
            .await
            .unwrap();

        assert!(!manager.is_running(&thread.thread_id));
        assert!(control.runtime_handle(&thread.thread_id).unwrap().is_none());
        assert_eq!(
            control
                .resolve_target(&AgentPath::root(), "worker")
                .unwrap()
                .status,
            AgentStatusV2::Completed {
                last_message: "finished".into()
            }
        );
        let permit = control.acquire_execution(&thread.thread_id).unwrap();
        drop(permit);
        let events = control
            .status_events(&thread.thread_id)
            .unwrap()
            .into_iter()
            .map(|event| event.event)
            .collect::<Vec<_>>();
        assert!(matches!(
            events.as_slice(),
            [
                RunnerEvent::TurnStarted { .. },
                RunnerEvent::TurnCompleted { .. }
            ]
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn session_construction_failure_records_error_and_cleans_active_state() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "broken");
        let broken_memory = dir.path().join("not-a-directory");
        std::fs::write(&broken_memory, "file").unwrap();
        let manager = AgentRuntimeManager::default();

        let error = manager
            .start_turn(request(
                Arc::clone(&control),
                thread.clone(),
                broken_memory,
                scripted_chat("unreachable"),
            ))
            .await
            .unwrap_err();

        assert!(!error.to_string().is_empty());
        assert!(!manager.is_running(&thread.thread_id));
        assert!(control.runtime_handle(&thread.thread_id).unwrap().is_none());
        assert!(matches!(
            control
                .resolve_target(&AgentPath::root(), "broken")
                .unwrap()
                .status,
            AgentStatusV2::Errored { .. }
        ));
        let permit = control.acquire_execution(&thread.thread_id).unwrap();
        drop(permit);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn interrupt_waits_for_durable_turn_interrupted_ack() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = Arc::new(AgentRuntimeManager::default());
        let run_manager = Arc::clone(&manager);
        let run = request(
            Arc::clone(&control),
            thread.clone(),
            dir.path().join("memory"),
            pending_chat(),
        );

        let (run_result, interrupted) = tokio::join!(run_manager.start_turn(run), async {
            while !manager.is_running(&thread.thread_id) {
                tokio::task::yield_now().await;
            }
            manager.interrupt(&thread.thread_id).await
        });

        run_result.unwrap();
        assert_eq!(interrupted.unwrap(), AgentStatusV2::Interrupted);
        assert!(!manager.is_running(&thread.thread_id));
        let events = control.status_events(&thread.thread_id).unwrap();
        assert!(matches!(
            events.last().map(|event| &event.event),
            Some(RunnerEvent::TurnInterrupted { .. })
        ));
        assert_eq!(
            session::SessionStore::open_sessions_dir(&dir.path().join("memory/sessions"))
                .unwrap()
                .get_messages(&thread.session_id)
                .unwrap()
                .into_iter()
                .map(|message| message.role)
                .collect::<Vec<_>>(),
            vec!["user", "assistant"]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn terminate_records_one_turn_terminal_then_runtime_terminated() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = Arc::new(AgentRuntimeManager::default());
        let run_manager = Arc::clone(&manager);
        let run = request(
            Arc::clone(&control),
            thread.clone(),
            dir.path().join("memory"),
            pending_chat(),
        );

        let (run_result, terminate_result) = tokio::join!(run_manager.start_turn(run), async {
            while !manager.is_running(&thread.thread_id) {
                tokio::task::yield_now().await;
            }
            manager.terminate(&thread.thread_id).await
        });

        run_result.unwrap();
        terminate_result.unwrap();
        assert_eq!(
            control
                .resolve_target(&AgentPath::root(), "worker")
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
        assert_eq!(
            control
                .status_events(&thread.thread_id)
                .unwrap()
                .into_iter()
                .map(|event| event.event)
                .collect::<Vec<_>>()
                .iter()
                .map(|event| match event {
                    RunnerEvent::TurnStarted { .. } => "started",
                    RunnerEvent::TurnInterrupted { .. } => "interrupted",
                    RunnerEvent::RuntimeTerminated => "terminated",
                    RunnerEvent::TurnCompleted { .. } => "completed",
                    RunnerEvent::TurnErrored { .. } => "errored",
                })
                .collect::<Vec<_>>(),
            vec!["started", "interrupted", "terminated"]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn follow_up_after_interrupt_hydrates_structured_session_history() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions
            .create_session("root", "tauri", None, None, None)
            .unwrap();
        sessions
            .create_session(&thread.session_id, "tauri", None, None, Some("root"))
            .unwrap();
        sessions
            .append_message(session::NewMessage {
                content: Some("first question"),
                ..session::NewMessage::empty(&thread.session_id, "user")
            })
            .unwrap();
        sessions
            .append_message(session::NewMessage {
                content: Some("calling tool"),
                tool_calls: Some(serde_json::json!([{
                    "id": "call-1", "name": "inspect", "arguments": {"path": "a.rs"}
                }])),
                ..session::NewMessage::empty(&thread.session_id, "assistant")
            })
            .unwrap();
        sessions
            .append_message(session::NewMessage {
                content: Some("tool result"),
                tool_call_id: Some("call-1"),
                tool_name: Some("inspect"),
                ..session::NewMessage::empty(&thread.session_id, "tool")
            })
            .unwrap();
        control
            .record_runner_event(
                &thread.thread_id,
                RunnerEvent::TurnStarted {
                    turn_id: "old-turn".into(),
                },
            )
            .unwrap();
        control
            .record_runner_event(
                &thread.thread_id,
                RunnerEvent::TurnInterrupted {
                    turn_id: "old-turn".into(),
                    reason: "parent interrupt".into(),
                },
            )
            .unwrap();

        let saw_structured_tool = Arc::new(AtomicBool::new(false));
        AgentRuntimeManager::default()
            .start_turn(request(
                Arc::clone(&control),
                thread.clone(),
                memory_dir,
                history_asserting_chat(Arc::clone(&saw_structured_tool)),
            ))
            .await
            .unwrap();

        assert!(saw_structured_tool.load(Ordering::SeqCst));
        let stored = sessions.get_messages(&thread.session_id).unwrap();
        assert_eq!(
            stored
                .iter()
                .filter(|message| message.role != "tool")
                .map(|message| message.role.as_str())
                .collect::<Vec<_>>(),
            vec!["user", "assistant", "user", "assistant"]
        );
        assert_eq!(stored[1].tool_calls.as_ref().unwrap()[0]["id"], "call-1");
        assert_eq!(stored[2].tool_call_id.as_deref(), Some("call-1"));
    }
}
