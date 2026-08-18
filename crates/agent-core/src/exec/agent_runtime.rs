use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use subagents::{
    AgentRuntimeHandle, AgentStatusV2, AgentThreadControl, AgentThreadV2, RunnerEvent,
    SpawnRuntimeV2Request,
};
use tokio::sync::watch;
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

#[derive(Debug, Clone)]
pub enum RunnerAck {
    Terminated(RunnerTermination),
    Failed(String),
}

struct ActiveAgentTurn {
    turn_id: String,
    interrupt: Arc<AgentThreadControl>,
    terminated: watch::Receiver<Option<RunnerAck>>,
}

#[cfg(test)]
type TerminalPersistenceHook =
    Arc<dyn Fn(&RunnerEvent) -> anyhow::Result<()> + Send + Sync + 'static>;

struct StartTurnOwnerGuard<'a> {
    manager: &'a AgentRuntimeManager,
    control: &'a subagents::AgentControl,
    thread_id: String,
    turn_id: String,
    runtime_handle: AgentRuntimeHandle,
    terminated_tx: watch::Sender<Option<RunnerAck>>,
    memory_dir: PathBuf,
    session_id: String,
    interrupt_message: bool,
    armed: bool,
    _permit: subagents::ExecutionPermit<'a>,
}

impl StartTurnOwnerGuard<'_> {
    fn publish_termination(&self, terminal_status: AgentStatusV2) {
        let _ = self
            .terminated_tx
            .send(Some(RunnerAck::Terminated(RunnerTermination {
                terminal_status,
            })));
    }

    fn publish_failure(&self, error: &anyhow::Error) {
        let _ = self
            .terminated_tx
            .send(Some(RunnerAck::Failed(format!("{error:#}"))));
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StartTurnOwnerGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let owns_turn = match self
            .manager
            .remove_active_if_turn(&self.thread_id, &self.turn_id)
        {
            Ok(owns_turn) => owns_turn,
            Err(error) => {
                tracing::warn!(
                    thread_id = %self.thread_id,
                    turn_id = %self.turn_id,
                    %error,
                    "failed to remove cancelled agent turn from active runtime map"
                );
                false
            }
        };
        let durable_result = if owns_turn {
            self.control
                .status_events(&self.thread_id)
                .and_then(|events| {
                    let durable_running = events.last().is_some_and(|event| {
                        matches!(
                            &event.event,
                            RunnerEvent::TurnStarted { turn_id } if turn_id == &self.turn_id
                        )
                    });
                    if durable_running {
                        Ok(())
                    } else {
                        anyhow::bail!(
                            "cancelled agent turn no longer has its durable Running projection"
                        )
                    }
                })
                .and_then(|()| {
                    if self.interrupt_message {
                        ensure_interrupted_history_boundary(
                            &self.memory_dir,
                            &self.session_id,
                            "[astro:system]\nThe previous agent turn was interrupted because its runtime owner was dropped.",
                        )
                    } else {
                        Ok(())
                    }
                })
                .and_then(|()| {
                    self.manager.record_terminal_event(
                        self.control,
                        &self.thread_id,
                        RunnerEvent::TurnInterrupted {
                            turn_id: self.turn_id.clone(),
                            reason: "start_turn future cancelled or owner dropped".into(),
                        },
                    )?;
                    Ok(())
                })
        } else {
            Err(anyhow::anyhow!(
                "cancelled agent turn no longer owns its active runtime generation"
            ))
        };
        if owns_turn {
            if let Err(error) = self
                .control
                .remove_runtime_if_same(&self.thread_id, &self.runtime_handle)
            {
                tracing::warn!(
                    thread_id = %self.thread_id,
                    turn_id = %self.turn_id,
                    %error,
                    "failed to remove cancelled agent runtime handle"
                );
            }
        }
        match durable_result {
            Ok(()) => self.publish_termination(AgentStatusV2::Interrupted),
            Err(error) => {
                tracing::warn!(
                    thread_id = %self.thread_id,
                    turn_id = %self.turn_id,
                    %error,
                    "failed to durably interrupt cancelled agent turn"
                );
                self.publish_failure(&error);
            }
        }
    }
}

#[cfg(test)]
#[derive(Clone)]
struct AckSubscribeHook {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[derive(Default)]
pub struct AgentRuntimeManager {
    active: Mutex<HashMap<String, ActiveAgentTurn>>,
    #[cfg(test)]
    ack_subscribe_hook: Mutex<Option<AckSubscribeHook>>,
    #[cfg(test)]
    terminal_persistence_hook: Mutex<Option<TerminalPersistenceHook>>,
}

impl AgentRuntimeManager {
    pub async fn start_turn(&self, request: RunAgentTurnRequest) -> anyhow::Result<()> {
        let thread_id = request.thread.thread_id.clone();
        let control = Arc::clone(&request.control);
        let permit = control.acquire_execution(&thread_id)?;
        let turn_id = Uuid::new_v4().to_string();
        let interrupt = Arc::new(AgentThreadControl::default());
        interrupt.begin_turn();
        let (terminated_tx, terminated_rx) = watch::channel(None);

        {
            let mut active = self.lock_active()?;
            if active.contains_key(&thread_id) {
                anyhow::bail!("agent thread {thread_id:?} already has an active runtime turn");
            }
            active.insert(
                thread_id.clone(),
                ActiveAgentTurn {
                    turn_id: turn_id.clone(),
                    interrupt: Arc::clone(&interrupt),
                    terminated: terminated_rx,
                },
            );
        }

        if let Err(error) = control.record_runner_event(
            &thread_id,
            RunnerEvent::TurnStarted {
                turn_id: turn_id.clone(),
            },
        ) {
            let _ = self.remove_active_if_turn(&thread_id, &turn_id);
            return Err(error);
        }

        let runtime_interrupt = Arc::clone(&interrupt);
        let runtime_terminate = Arc::clone(&interrupt);
        let runtime_handle = AgentRuntimeHandle {
            interrupt: Arc::new(move || runtime_interrupt.interrupt()),
            terminate: Arc::new(move || runtime_terminate.close()),
        };
        if let Err(error) = control.register_runtime(&thread_id, runtime_handle.clone()) {
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

        let mut owner_guard = StartTurnOwnerGuard {
            manager: self,
            control: control.as_ref(),
            thread_id: thread_id.clone(),
            turn_id: turn_id.clone(),
            runtime_handle,
            terminated_tx,
            memory_dir: request.memory_dir.clone(),
            session_id: request.thread.session_id.clone(),
            interrupt_message: request.runtime.interrupt_message,
            armed: true,
            _permit: permit,
        };

        let result = run_request(&request, Arc::clone(&interrupt)).await;
        let terminal_boundary_result = if (interrupt.is_closed() || interrupt.is_interrupted())
            && request.runtime.interrupt_message
        {
            ensure_interrupted_history_boundary(
                &request.memory_dir,
                &request.thread.session_id,
                "[astro:system]\nThe previous agent turn was interrupted by the parent.",
            )
        } else {
            Ok(())
        };
        let (event, terminal_status) = if interrupt.is_closed() || interrupt.is_interrupted() {
            let terminal_status = if interrupt.is_closed() {
                AgentStatusV2::Shutdown
            } else {
                AgentStatusV2::Interrupted
            };
            (
                RunnerEvent::TurnInterrupted {
                    turn_id: turn_id.clone(),
                    reason: if interrupt.is_closed() {
                        "runtime terminated by parent".into()
                    } else {
                        "interrupted by parent".into()
                    },
                },
                terminal_status,
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

        let durable_result = terminal_boundary_result
            .and_then(|()| {
                self.record_terminal_event(&control, &thread_id, event)
                    .map(|_| ())
            })
            .and_then(|()| {
                if interrupt.is_closed() {
                    self.record_terminal_event(&control, &thread_id, RunnerEvent::RuntimeTerminated)
                        .map(|_| ())
                } else {
                    Ok(())
                }
            });
        match &durable_result {
            Ok(()) => owner_guard.publish_termination(terminal_status.clone()),
            Err(error) => owner_guard.publish_failure(error),
        }
        let active_result = self.remove_active_if_turn(&thread_id, &turn_id);
        let runtime_result =
            control.remove_runtime_if_same(&thread_id, &owner_guard.runtime_handle);
        owner_guard.disarm();
        durable_result?;
        active_result?;
        runtime_result?;

        match result {
            Err(error) if !interrupt.is_interrupted() && !interrupt.is_closed() => Err(error),
            _ => Ok(()),
        }
    }

    pub async fn interrupt(&self, thread_id: &str) -> anyhow::Result<AgentStatusV2> {
        let (interrupt, terminated) = self.termination_subscription(thread_id)?;
        self.pause_after_ack_subscribe().await;
        interrupt.interrupt();
        let termination = wait_for_termination(terminated, "interruption").await?;
        Ok(termination.terminal_status)
    }

    pub async fn terminate(&self, thread_id: &str) -> anyhow::Result<()> {
        let (interrupt, terminated) = self.termination_subscription(thread_id)?;
        self.pause_after_ack_subscribe().await;
        interrupt.close();
        wait_for_termination(terminated, "termination").await?;
        Ok(())
    }

    pub fn is_running(&self, thread_id: &str) -> bool {
        self.active
            .lock()
            .map(|active| active.contains_key(thread_id))
            .unwrap_or(false)
    }

    fn termination_subscription(
        &self,
        thread_id: &str,
    ) -> anyhow::Result<(Arc<AgentThreadControl>, watch::Receiver<Option<RunnerAck>>)> {
        let active = self.lock_active()?;
        let turn = active.get(thread_id).ok_or_else(|| {
            anyhow::anyhow!("agent thread {thread_id:?} has no active runtime turn")
        })?;
        Ok((Arc::clone(&turn.interrupt), turn.terminated.clone()))
    }

    #[cfg(test)]
    async fn pause_after_ack_subscribe(&self) {
        let hook = self.ack_subscribe_hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook.entered.notify_one();
            hook.release.notified().await;
        }
    }

    #[cfg(not(test))]
    async fn pause_after_ack_subscribe(&self) {}

    #[cfg(test)]
    fn set_ack_subscribe_hook(&self, hook: Option<AckSubscribeHook>) {
        *self.ack_subscribe_hook.lock().unwrap() = hook;
    }

    #[cfg(test)]
    fn set_terminal_persistence_hook(&self, hook: Option<TerminalPersistenceHook>) {
        *self.terminal_persistence_hook.lock().unwrap() = hook;
    }

    fn record_terminal_event(
        &self,
        control: &subagents::AgentControl,
        thread_id: &str,
        event: RunnerEvent,
    ) -> anyhow::Result<AgentThreadV2> {
        #[cfg(test)]
        if let Some(hook) = self
            .terminal_persistence_hook
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal persistence hook mutex is poisoned"))?
            .clone()
        {
            hook(&event)?;
        }
        control.record_runner_event(thread_id, event)
    }

    fn finish_failed_start(
        &self,
        control: &subagents::AgentControl,
        thread_id: &str,
        turn_id: &str,
        message: String,
        terminated_tx: watch::Sender<Option<RunnerAck>>,
    ) -> anyhow::Result<()> {
        let status = AgentStatusV2::Errored {
            message: message.clone(),
        };
        let event_result = self.record_terminal_event(
            control,
            thread_id,
            RunnerEvent::TurnErrored {
                turn_id: turn_id.to_string(),
                message,
            },
        );
        let _ = control.remove_runtime(thread_id);
        let _ = self.remove_active_if_turn(thread_id, turn_id);
        match &event_result {
            Ok(_) => {
                let _ = terminated_tx.send(Some(RunnerAck::Terminated(RunnerTermination {
                    terminal_status: status,
                })));
            }
            Err(error) => {
                let _ = terminated_tx.send(Some(RunnerAck::Failed(format!("{error:#}"))));
            }
        }
        event_result?;
        Ok(())
    }

    fn remove_active_if_turn(&self, thread_id: &str, turn_id: &str) -> anyhow::Result<bool> {
        let mut active = self.lock_active()?;
        let matches = active
            .get(thread_id)
            .is_some_and(|turn| turn.turn_id == turn_id);
        if matches {
            active.remove(thread_id);
        }
        Ok(matches)
    }

    fn lock_active(
        &self,
    ) -> anyhow::Result<std::sync::MutexGuard<'_, HashMap<String, ActiveAgentTurn>>> {
        self.active
            .lock()
            .map_err(|_| anyhow::anyhow!("agent runtime manager mutex is poisoned"))
    }
}

async fn wait_for_termination(
    mut terminated: watch::Receiver<Option<RunnerAck>>,
    operation: &str,
) -> anyhow::Result<RunnerTermination> {
    loop {
        if let Some(ack) = terminated.borrow().clone() {
            return match ack {
                RunnerAck::Terminated(termination) => Ok(termination),
                RunnerAck::Failed(message) => Err(anyhow::anyhow!(
                    "{operation} acknowledgement failed: {message}"
                )),
            };
        }
        terminated.changed().await.map_err(|_| {
            anyhow::anyhow!("agent runtime ended without {operation} acknowledgement")
        })?;
    }
}

fn ensure_interrupted_history_boundary(
    memory_dir: &std::path::Path,
    session_id: &str,
    content: &str,
) -> anyhow::Result<()> {
    let sessions = session::SessionStore::open_sessions_dir(&memory_dir.join("sessions"))?;
    let messages = sessions.get_messages(session_id)?;
    if messages
        .last()
        .is_some_and(|message| message.role == "user")
    {
        sessions.append_message(session::NewMessage {
            content: Some(content),
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
    let result = crate::exec::background::run_background_multi_turn_controlled_with_chat(
        Arc::clone(&session),
        targets,
        vec![TurnInput::UserInput {
            content: request.runtime.model_request.message.clone(),
            image_data_urls: Vec::new(),
        }],
        Some(Arc::clone(&interrupt)),
        request.chat_override.clone(),
    )
    .await;
    if result.is_err() && !interrupt.is_interrupted() && !interrupt.is_closed() {
        session
            .lock()
            .await
            .ensure_assistant_error_boundary()
            .await?;
    }
    let (output, _) = result?;
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

    use super::{wait_for_termination, AckSubscribeHook, AgentRuntimeManager, RunAgentTurnRequest};

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

    fn barrier_pending_chat(barrier: Arc<tokio::sync::Barrier>) -> ChatOverride {
        Arc::new(move |_messages, _tools, _config| {
            let barrier = Arc::clone(&barrier);
            Box::pin(async move {
                barrier.wait().await;
                Ok(Box::pin(stream::pending::<anyhow::Result<StreamChunk>>()) as CompletionStream)
            })
        })
    }

    fn gated_scripted_chat(
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        reply: &str,
    ) -> ChatOverride {
        let reply = reply.to_string();
        Arc::new(move |_messages, _tools, _config| {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            let reply = reply.clone();
            Box::pin(async move {
                entered.notify_one();
                release.notified().await;
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::Text(reply)),
                    Ok(StreamChunk::Done {
                        finish_reason: "stop".into(),
                    }),
                ])) as CompletionStream)
            })
        })
    }

    fn gated_failing_chat(
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    ) -> ChatOverride {
        Arc::new(move |_messages, _tools, _config| {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            Box::pin(async move {
                entered.notify_one();
                release.notified().await;
                Err(anyhow::anyhow!("provider failed before assistant output"))
            })
        })
    }

    fn role_capturing_chat(captured_roles: Arc<std::sync::Mutex<Vec<String>>>) -> ChatOverride {
        Arc::new(move |messages, _tools, _config| {
            *captured_roles.lock().unwrap() = messages
                .iter()
                .filter(|message| message.role() != providers::types::message::Role::System)
                .map(|message| message.role().as_str().to_string())
                .collect();
            Box::pin(async move {
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::Text("recovered answer".into())),
                    Ok(StreamChunk::Done {
                        finish_reason: "stop".into(),
                    }),
                ])) as CompletionStream)
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
        let manager = Arc::new(AgentRuntimeManager::default());
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let run = request(
            Arc::clone(&control),
            thread.clone(),
            dir.path().join("memory"),
            gated_scripted_chat(Arc::clone(&entered), Arc::clone(&release), "finished"),
        );

        let (run_result, observed_ack) = tokio::join!(manager.start_turn(run), async {
            entered.notified().await;
            let (_, observer) = manager.termination_subscription(&thread.thread_id).unwrap();
            release.notify_one();
            wait_for_termination(observer, "completion observer").await
        });

        run_result.unwrap();
        assert_eq!(
            observed_ack.unwrap().terminal_status,
            AgentStatusV2::Completed {
                last_message: "finished".into()
            }
        );

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
    async fn provider_failure_records_assistant_boundary_before_follow_up() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let memory_dir = dir.path().join("memory");
        let manager = Arc::new(AgentRuntimeManager::default());
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let run = request(
            Arc::clone(&control),
            thread.clone(),
            memory_dir.clone(),
            gated_failing_chat(Arc::clone(&entered), Arc::clone(&release)),
        );

        let (run_result, observed_ack) = tokio::join!(manager.start_turn(run), async {
            entered.notified().await;
            let (_, observer) = manager.termination_subscription(&thread.thread_id).unwrap();
            release.notify_one();
            wait_for_termination(observer, "error observer").await
        });
        run_result.unwrap_err();
        assert!(matches!(
            observed_ack.unwrap().terminal_status,
            AgentStatusV2::Errored { message }
                if message.contains("provider failed before assistant output")
        ));
        assert!(matches!(
            control
                .resolve_target(&AgentPath::root(), "worker")
                .unwrap()
                .status,
            AgentStatusV2::Errored { message }
                if message.contains("provider failed before assistant output")
        ));

        let captured_roles = Arc::new(std::sync::Mutex::new(Vec::new()));
        manager
            .start_turn(request(
                Arc::clone(&control),
                thread.clone(),
                memory_dir.clone(),
                role_capturing_chat(Arc::clone(&captured_roles)),
            ))
            .await
            .unwrap();

        assert_eq!(
            *captured_roles.lock().unwrap(),
            vec!["user", "assistant", "user"]
        );
        let stored =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        assert_eq!(
            stored
                .get_messages(&thread.session_id)
                .unwrap()
                .into_iter()
                .map(|message| message.role)
                .collect::<Vec<_>>(),
            vec!["user", "assistant", "user", "assistant"]
        );
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
    async fn interrupt_ack_reports_terminal_persistence_failure() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = Arc::new(AgentRuntimeManager::default());
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let run = request(
            Arc::clone(&control),
            thread.clone(),
            dir.path().join("memory"),
            barrier_pending_chat(Arc::clone(&barrier)),
        );

        let (run_result, (interrupt_result, observer_one, observer_two)) =
            tokio::join!(manager.start_turn(run), async {
                barrier.wait().await;
                assert_eq!(
                    control
                        .resolve_target(&AgentPath::root(), "worker")
                        .unwrap()
                        .status,
                    AgentStatusV2::Running
                );
                manager.set_terminal_persistence_hook(Some(Arc::new(|event| {
                    if matches!(event, RunnerEvent::TurnInterrupted { .. }) {
                        anyhow::bail!("injected durable terminal persistence failure");
                    }
                    Ok(())
                })));
                let (_, observer_one) =
                    manager.termination_subscription(&thread.thread_id).unwrap();
                let observer_two = observer_one.clone();
                (
                    manager.interrupt(&thread.thread_id).await,
                    observer_one,
                    observer_two,
                )
            });

        assert!(run_result
            .unwrap_err()
            .to_string()
            .contains("durable terminal persistence failure"));
        assert!(interrupt_result
            .unwrap_err()
            .to_string()
            .contains("durable terminal persistence failure"));
        for observer in [observer_one, observer_two] {
            assert!(wait_for_termination(observer, "terminal persistence")
                .await
                .unwrap_err()
                .to_string()
                .contains("durable terminal persistence failure"));
        }
        assert!(!manager.is_running(&thread.thread_id));
        assert!(control.runtime_handle(&thread.thread_id).unwrap().is_none());
        let permit = control.acquire_execution(&thread.thread_id).unwrap();
        drop(permit);
        assert_eq!(
            control
                .resolve_target(&AgentPath::root(), "worker")
                .unwrap()
                .status,
            AgentStatusV2::Running
        );
        assert!(matches!(
            control
                .status_events(&thread.thread_id)
                .unwrap()
                .last()
                .map(|event| &event.event),
            Some(RunnerEvent::TurnStarted { .. })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn canceled_interrupt_waiter_does_not_consume_runtime_ack() {
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
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());

        let (run_result, takeover) = tokio::join!(run_manager.start_turn(run), async {
            while !manager.is_running(&thread.thread_id) {
                tokio::task::yield_now().await;
            }
            manager.set_ack_subscribe_hook(Some(AckSubscribeHook {
                entered: Arc::clone(&entered),
                release,
            }));
            let waiter = tokio::spawn({
                let manager = Arc::clone(&manager);
                let thread_id = thread.thread_id.clone();
                async move { manager.interrupt(&thread_id).await }
            });
            entered.notified().await;
            waiter.abort();
            assert!(waiter.await.unwrap_err().is_cancelled());
            manager.set_ack_subscribe_hook(None);

            let takeover = manager.terminate(&thread.thread_id).await;
            if takeover.is_err() {
                let runtime = control
                    .runtime_handle(&thread.thread_id)
                    .unwrap()
                    .expect("runtime handle for RED cleanup");
                (runtime.terminate)();
            }
            takeover
        });

        run_result.unwrap();
        takeover.unwrap();
        assert!(!manager.is_running(&thread.thread_id));
        assert!(control.runtime_handle(&thread.thread_id).unwrap().is_none());
        let permit = control.acquire_execution(&thread.thread_id).unwrap();
        drop(permit);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn aborted_start_turn_owner_cleans_runtime_and_allows_follow_up() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = Arc::new(AgentRuntimeManager::default());
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let owner = tokio::spawn({
            let manager = Arc::clone(&manager);
            let request = request(
                Arc::clone(&control),
                thread.clone(),
                dir.path().join("memory"),
                barrier_pending_chat(Arc::clone(&barrier)),
            );
            async move { manager.start_turn(request).await }
        });

        barrier.wait().await;
        assert!(manager.is_running(&thread.thread_id));
        assert!(control.runtime_handle(&thread.thread_id).unwrap().is_some());
        assert_eq!(
            control
                .resolve_target(&AgentPath::root(), "worker")
                .unwrap()
                .status,
            AgentStatusV2::Running
        );
        let (_, termination) = manager.termination_subscription(&thread.thread_id).unwrap();

        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());

        let termination = wait_for_termination(termination, "owner cancellation")
            .await
            .ok();
        let active = manager.is_running(&thread.thread_id);
        let has_runtime = control.runtime_handle(&thread.thread_id).unwrap().is_some();
        let permit = control.acquire_execution(&thread.thread_id).ok();
        let permit_available = permit.is_some();
        drop(permit);
        let status_after_abort = control
            .resolve_target(&AgentPath::root(), "worker")
            .unwrap()
            .status;

        assert!(!active);
        assert!(!has_runtime);
        assert!(permit_available);
        assert_eq!(status_after_abort, AgentStatusV2::Interrupted);
        assert_eq!(
            termination.map(|termination| termination.terminal_status),
            Some(AgentStatusV2::Interrupted)
        );
        assert!(matches!(
            control
                .status_events(&thread.thread_id)
                .unwrap()
                .last()
                .map(|event| &event.event),
            Some(RunnerEvent::TurnInterrupted { reason, .. })
                if reason.contains("start_turn future cancelled")
        ));

        manager
            .start_turn(request(
                Arc::clone(&control),
                thread.clone(),
                dir.path().join("memory"),
                scripted_chat("follow-up completed"),
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
                last_message: "follow-up completed".into()
            }
        );
        assert_eq!(
            session::SessionStore::open_sessions_dir(&dir.path().join("memory/sessions"))
                .unwrap()
                .get_messages(&thread.session_id)
                .unwrap()
                .into_iter()
                .map(|message| message.role)
                .collect::<Vec<_>>(),
            vec!["user", "assistant", "user", "assistant"]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn aborted_owner_ack_reports_terminal_persistence_failure() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = Arc::new(AgentRuntimeManager::default());
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let owner = tokio::spawn({
            let manager = Arc::clone(&manager);
            let request = request(
                Arc::clone(&control),
                thread.clone(),
                dir.path().join("memory"),
                barrier_pending_chat(Arc::clone(&barrier)),
            );
            async move { manager.start_turn(request).await }
        });

        barrier.wait().await;
        manager.set_terminal_persistence_hook(Some(Arc::new(|event| {
            if matches!(event, RunnerEvent::TurnInterrupted { .. }) {
                anyhow::bail!("injected owner-drop persistence failure");
            }
            Ok(())
        })));
        let (_, termination) = manager.termination_subscription(&thread.thread_id).unwrap();
        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());

        assert!(wait_for_termination(termination, "owner cancellation")
            .await
            .unwrap_err()
            .to_string()
            .contains("owner-drop persistence failure"));
        assert!(!manager.is_running(&thread.thread_id));
        assert!(control.runtime_handle(&thread.thread_id).unwrap().is_none());
        let permit = control.acquire_execution(&thread.thread_id).unwrap();
        drop(permit);
        assert_eq!(
            control
                .resolve_target(&AgentPath::root(), "worker")
                .unwrap()
                .status,
            AgentStatusV2::Running
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

        let (run_result, (terminate_result, observed_ack)) =
            tokio::join!(run_manager.start_turn(run), async {
                while !manager.is_running(&thread.thread_id) {
                    tokio::task::yield_now().await;
                }
                let (_, observer) = manager.termination_subscription(&thread.thread_id).unwrap();
                let terminate_result = manager.terminate(&thread.thread_id).await;
                let observed_ack = wait_for_termination(observer, "termination observer").await;
                (terminate_result, observed_ack)
            });

        run_result.unwrap();
        terminate_result.unwrap();
        assert_eq!(
            observed_ack.unwrap().terminal_status,
            AgentStatusV2::Shutdown
        );
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
    async fn terminate_ack_reports_runtime_terminated_persistence_failure() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = Arc::new(AgentRuntimeManager::default());
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let run = request(
            Arc::clone(&control),
            thread.clone(),
            dir.path().join("memory"),
            barrier_pending_chat(Arc::clone(&barrier)),
        );

        let (run_result, terminate_result) = tokio::join!(manager.start_turn(run), async {
            barrier.wait().await;
            manager.set_terminal_persistence_hook(Some(Arc::new(|event| {
                if matches!(event, RunnerEvent::RuntimeTerminated) {
                    anyhow::bail!("injected RuntimeTerminated persistence failure");
                }
                Ok(())
            })));
            manager.terminate(&thread.thread_id).await
        });

        assert!(run_result
            .unwrap_err()
            .to_string()
            .contains("RuntimeTerminated persistence failure"));
        assert!(terminate_result
            .unwrap_err()
            .to_string()
            .contains("RuntimeTerminated persistence failure"));
        assert!(!manager.is_running(&thread.thread_id));
        assert!(control.runtime_handle(&thread.thread_id).unwrap().is_none());
        let permit = control.acquire_execution(&thread.thread_id).unwrap();
        drop(permit);
        assert_eq!(
            control
                .resolve_target(&AgentPath::root(), "worker")
                .unwrap()
                .status,
            AgentStatusV2::Interrupted
        );
        assert!(matches!(
            control
                .status_events(&thread.thread_id)
                .unwrap()
                .last()
                .map(|event| &event.event),
            Some(RunnerEvent::TurnInterrupted { .. })
        ));
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
