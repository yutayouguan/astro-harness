use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use subagents::{
    AgentRuntimeHandle, AgentStatusV2, AgentThreadControl, AgentThreadV2, RunnerEvent,
    SpawnRuntimeV2Request,
};
use tokio::sync::watch;
use uuid::Uuid;

use crate::runtime::{Config, Session, TurnResult};
use crate::streaming::ChatOverride;
use crate::tasks::TurnInput;

pub struct RunAgentTurnRequest {
    pub control: Arc<subagents::AgentControl>,
    pub thread: AgentThreadV2,
    pub runtime: SpawnRuntimeV2Request,
    pub memory_dir: PathBuf,
    pub chat_override: Option<ChatOverride>,
    /// Follow-up turns obtain their user input from the durable mailbox at the
    /// first sampling boundary instead of duplicating it as an initial input.
    pub consume_mailbox: bool,
    /// Independent admission acknowledgement used by spawn.  It is published
    /// once TurnStarted is durable and the active runtime handle is registered;
    /// terminal turn completion is deliberately not part of this protocol.
    pub(super) startup_tx: Option<watch::Sender<Option<Result<(), String>>>>,
    pub(super) startup_accept_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    pub(super) unaccepted_spawn_cleanup: Option<Arc<UnacceptedSpawnCleanup>>,
    pub(super) followup_start_tx: Option<watch::Sender<Option<Result<(), String>>>>,
    pub(super) start_token: Option<String>,
}

type UnacceptedSpawnCleanupFn = dyn Fn(Option<&str>) -> anyhow::Result<()> + Send + Sync + 'static;

pub(super) struct UnacceptedSpawnCleanup {
    accepted: AtomicBool,
    turn_id: Mutex<Option<String>>,
    cleanup: Arc<UnacceptedSpawnCleanupFn>,
}

impl UnacceptedSpawnCleanup {
    pub(super) fn new(
        cleanup: impl Fn(Option<&str>) -> anyhow::Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            accepted: AtomicBool::new(false),
            turn_id: Mutex::new(None),
            cleanup: Arc::new(cleanup),
        }
    }

    fn set_turn_id(&self, turn_id: &str) {
        if let Ok(mut stored) = self.turn_id.lock() {
            *stored = Some(turn_id.to_string());
        }
    }

    fn accept(&self) {
        self.accepted.store(true, Ordering::Release);
    }

    pub(super) fn run(&self) -> anyhow::Result<()> {
        if self.accepted.load(Ordering::Acquire) {
            return Ok(());
        }
        let turn_id = self
            .turn_id
            .lock()
            .map_err(|_| anyhow::anyhow!("unaccepted spawn cleanup mutex is poisoned"))?
            .clone();
        (self.cleanup)(turn_id.as_deref())
    }
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
    pending_followup: Option<PendingFollowup>,
}

struct StartingAgentTurn {
    token: String,
    result_rx: watch::Receiver<Option<Result<(), String>>>,
}

enum RuntimeSlot {
    Starting(StartingAgentTurn),
    Running(Box<ActiveAgentTurn>),
}

enum RuntimeTerminationState {
    Missing,
    Starting,
    Running {
        control: Arc<AgentThreadControl>,
        terminated: watch::Receiver<Option<RunnerAck>>,
    },
}

pub(super) enum CloseThreadStart {
    Complete,
    Starting,
    TerminationRequested(watch::Receiver<Option<RunnerAck>>),
}

struct PendingFollowup {
    request: RunAgentTurnRequest,
    result_rx: watch::Receiver<Option<Result<(), String>>>,
}

pub(super) enum FollowupAdmission {
    StartNow {
        request: Box<RunAgentTurnRequest>,
        result_rx: watch::Receiver<Option<Result<(), String>>>,
    },
    AwaitStart {
        result_rx: watch::Receiver<Option<Result<(), String>>>,
    },
}

#[cfg(test)]
type TerminalPersistenceHook =
    Arc<dyn Fn(&RunnerEvent) -> anyhow::Result<()> + Send + Sync + 'static>;
#[cfg(test)]
type BeforeCleanupHook = Arc<dyn Fn() + Send + Sync + 'static>;
#[cfg(test)]
type CleanupFailureHook = Arc<dyn Fn(&str) -> anyhow::Result<()> + Send + Sync + 'static>;

struct StartTurnOwnerGuard<'a> {
    manager: &'a AgentRuntimeManager,
    control: &'a subagents::AgentControl,
    thread_id: String,
    turn_id: String,
    runtime_control: Arc<AgentThreadControl>,
    runtime_handle: AgentRuntimeHandle,
    terminated_tx: watch::Sender<Option<RunnerAck>>,
    memory_dir: PathBuf,
    session_id: String,
    interrupt_message: bool,
    armed: bool,
    permit: Option<subagents::ExecutionPermit<'a>>,
}

struct FailedStartResources<'a> {
    runtime_handle: AgentRuntimeHandle,
    permit: subagents::ExecutionPermit<'a>,
    terminated_tx: watch::Sender<Option<RunnerAck>>,
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

    fn release_permit(&mut self) {
        drop(self.permit.take());
    }

    fn persist_dropped_owner_terminal(&self, close_requested: bool) -> anyhow::Result<()> {
        let owns_turn = self
            .manager
            .active_turn_matches(&self.thread_id, &self.turn_id)?;
        if !owns_turn {
            anyhow::bail!("cancelled agent turn no longer owns its active runtime generation");
        }

        let events = self.control.status_events(&self.thread_id)?;
        let last = events.last().map(|event| &event.event);
        let needs_interrupted_event = match last {
            Some(RunnerEvent::TurnStarted { turn_id }) if turn_id == &self.turn_id => true,
            Some(RunnerEvent::TurnInterrupted { turn_id, .. }) if turn_id == &self.turn_id => false,
            Some(RunnerEvent::RuntimeTerminated) if close_requested => return Ok(()),
            _ => anyhow::bail!(
                "cancelled agent turn no longer has a recoverable durable terminal projection"
            ),
        };

        if self.interrupt_message {
            let content = if close_requested {
                "[astro:system]\nThe previous agent turn was terminated because its runtime owner was dropped after close was requested."
            } else {
                "[astro:system]\nThe previous agent turn was interrupted because its runtime owner was dropped."
            };
            ensure_interrupted_history_boundary(&self.memory_dir, &self.session_id, content)?;
        }
        if needs_interrupted_event {
            self.manager.record_terminal_event(
                self.control,
                &self.thread_id,
                RunnerEvent::TurnInterrupted {
                    turn_id: self.turn_id.clone(),
                    reason: if close_requested {
                        "start_turn owner dropped after runtime termination was requested".into()
                    } else {
                        "start_turn future cancelled or owner dropped".into()
                    },
                },
            )?;
        }
        if close_requested {
            self.manager.record_terminal_event(
                self.control,
                &self.thread_id,
                RunnerEvent::RuntimeTerminated,
            )?;
        }
        Ok(())
    }
}

impl Drop for StartTurnOwnerGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let close_requested = self.runtime_control.is_closed();
        let durable_result = self.persist_dropped_owner_terminal(close_requested);

        self.manager.run_before_cleanup_hook();
        let active_result = self
            .manager
            .remove_active_if_turn(&self.thread_id, &self.turn_id)
            .and_then(|removed| {
                self.manager.run_cleanup_failure_hook("active")?;
                Ok(removed)
            })
            .map(|_| ());
        let runtime_result = self
            .control
            .remove_runtime_if_same(&self.thread_id, &self.runtime_handle)
            .and_then(|removed| {
                self.manager.run_cleanup_failure_hook("runtime handle")?;
                Ok(removed)
            })
            .map(|_| ());
        self.release_permit();
        self.disarm();
        let completion_result =
            combine_completion_results([durable_result, active_result, runtime_result]);
        match completion_result {
            Ok(()) => self.publish_termination(if close_requested {
                AgentStatusV2::Shutdown
            } else {
                AgentStatusV2::Interrupted
            }),
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
pub(super) struct AckSubscribeHook {
    pub(super) entered: Arc<tokio::sync::Notify>,
    pub(super) release: Arc<tokio::sync::Notify>,
}

#[derive(Default)]
pub struct AgentRuntimeManager {
    active: Mutex<HashMap<String, RuntimeSlot>>,
    subtree_close: Arc<tokio::sync::Mutex<()>>,
    #[cfg(test)]
    ack_subscribe_hook: Mutex<Option<AckSubscribeHook>>,
    #[cfg(test)]
    ack_subscribe_barrier: Mutex<Option<Arc<tokio::sync::Barrier>>>,
    #[cfg(test)]
    terminal_persistence_hook: Mutex<Option<TerminalPersistenceHook>>,
    #[cfg(test)]
    before_terminal_persist_hook: Mutex<Option<AckSubscribeHook>>,
    #[cfg(test)]
    before_cleanup_hook: Mutex<Option<BeforeCleanupHook>>,
    #[cfg(test)]
    cleanup_failure_hook: Mutex<Option<CleanupFailureHook>>,
    #[cfg(test)]
    followup_admission_barrier: Mutex<Option<Arc<tokio::sync::Barrier>>>,
    #[cfg(test)]
    before_followup_start_hook: Mutex<Option<AckSubscribeHook>>,
    #[cfg(test)]
    before_startup_ack_hook: Mutex<Option<AckSubscribeHook>>,
    #[cfg(test)]
    after_startup_permit_hook: Mutex<Option<AckSubscribeHook>>,
    #[cfg(test)]
    start_status_failure: Mutex<Option<String>>,
    #[cfg(test)]
    close_timeout: Mutex<Option<std::time::Duration>>,
}

impl AgentRuntimeManager {
    /// Process-wide active-turn manager. Thread ids are globally unique and
    /// every session under one Agent Tree must observe the same acknowledgements.
    pub fn global() -> Arc<Self> {
        static MANAGER: OnceLock<Arc<AgentRuntimeManager>> = OnceLock::new();
        Arc::clone(MANAGER.get_or_init(|| Arc::new(AgentRuntimeManager::default())))
    }

    pub async fn start_turn(&self, request: RunAgentTurnRequest) -> anyhow::Result<()> {
        let startup_tx = request.startup_tx.clone();
        let unaccepted_cleanup = request.unaccepted_spawn_cleanup.clone();
        let mut result = self.start_turn_inner(request).await;
        if let Some(cleanup) = unaccepted_cleanup.as_ref() {
            if let Err(cleanup_error) = cleanup.run() {
                result = Err(match result {
                    Ok(()) => cleanup_error.context("unaccepted spawn cleanup"),
                    Err(start_error) => anyhow::anyhow!(
                        "{start_error:#}; unaccepted spawn cleanup failed: {cleanup_error:#}"
                    ),
                });
            }
        }
        if let Some(startup_tx) = startup_tx {
            if startup_tx.borrow().is_none() {
                let shared = result
                    .as_ref()
                    .map(|_| ())
                    .map_err(|error| format!("{error:#}"));
                let _ = startup_tx.send(Some(shared));
            }
        }
        result
    }

    async fn start_turn_inner(&self, mut request: RunAgentTurnRequest) -> anyhow::Result<()> {
        self.pause_before_followup_start(&request).await;
        let thread_id = request.thread.thread_id.clone();
        let control = Arc::clone(&request.control);
        let permit = match control.acquire_execution(&thread_id) {
            Ok(permit) => permit,
            Err(error) => {
                self.fail_starting_request(&request, &error);
                return Err(error);
            }
        };
        self.pause_after_startup_permit(&request).await;
        let turn_id = Uuid::new_v4().to_string();
        if let Some(cleanup) = request.unaccepted_spawn_cleanup.as_ref() {
            cleanup.set_turn_id(&turn_id);
        }
        let interrupt = Arc::new(AgentThreadControl::default());
        interrupt.begin_turn();
        let (terminated_tx, terminated_rx) = watch::channel(None);
        let status_events = match self.status_events_for_start(&control, &thread_id) {
            Ok(events) => events,
            Err(error) => {
                self.run_before_cleanup_hook();
                let cleanup = self
                    .remove_active_if_turn(&thread_id, &turn_id)
                    .and_then(|removed| {
                        self.run_cleanup_failure_hook("active")?;
                        Ok(removed)
                    })
                    .map(|_| ());
                drop(permit);
                let combined = combine_completion_results([Err(error), cleanup, Ok(())])
                    .expect_err("setup status failure must remain an error");
                self.fail_starting_request(&request, &combined);
                return Err(combined);
            }
        };
        let prior_turn_was_interrupted = status_events
            .last()
            .is_some_and(|event| matches!(event.event, RunnerEvent::TurnInterrupted { .. }));

        let claim_result = {
            let mut active = self.lock_active()?;
            let running = || {
                RuntimeSlot::Running(Box::new(ActiveAgentTurn {
                    turn_id: turn_id.clone(),
                    interrupt: Arc::clone(&interrupt),
                    terminated: terminated_rx,
                    pending_followup: None,
                }))
            };
            match (active.entry(thread_id.clone()), request.start_token.as_deref()) {
                (std::collections::hash_map::Entry::Vacant(entry), None) => {
                    entry.insert(running());
                    Ok(())
                }
                (std::collections::hash_map::Entry::Occupied(mut entry), Some(token))
                    if matches!(entry.get(), RuntimeSlot::Starting(slot) if slot.token == token) =>
                {
                    entry.insert(running());
                    Ok(())
                }
                (_, Some(token)) => Err(anyhow::anyhow!(
                    "follow-up starting reservation {token:?} no longer owns agent thread {thread_id:?}"
                )),
                (_, None) => Err(anyhow::anyhow!(
                    "agent thread {thread_id:?} already has an active runtime turn"
                )),
            }
        };
        if let Err(error) = claim_result {
            drop(permit);
            self.fail_starting_request(&request, &error);
            return Err(error);
        }

        if let Err(error) = self.record_terminal_event(
            &control,
            &thread_id,
            RunnerEvent::TurnStarted {
                turn_id: turn_id.clone(),
            },
        ) {
            self.run_before_cleanup_hook();
            let active_result = self
                .remove_active_if_turn(&thread_id, &turn_id)
                .and_then(|removed| {
                    self.run_cleanup_failure_hook("active")?;
                    Ok(removed)
                })
                .map(|_| ());
            drop(permit);
            let completion_result = combine_completion_results([
                Err(error.context("persist TurnStarted")),
                active_result,
                Ok(()),
            ]);
            let message = completion_result
                .as_ref()
                .err()
                .map(|error| format!("{error:#}"))
                .unwrap_or_else(|| "persist TurnStarted failed".to_string());
            let _ = terminated_tx.send(Some(RunnerAck::Failed(message)));
            return completion_result;
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
                request.unaccepted_spawn_cleanup.is_none(),
                FailedStartResources {
                    runtime_handle,
                    permit,
                    terminated_tx,
                },
            )?;
            return Err(error);
        }
        let mut owner_guard = StartTurnOwnerGuard {
            manager: self,
            control: control.as_ref(),
            thread_id: thread_id.clone(),
            turn_id: turn_id.clone(),
            runtime_control: Arc::clone(&interrupt),
            runtime_handle,
            terminated_tx,
            memory_dir: request.memory_dir.clone(),
            session_id: request.thread.session_id.clone(),
            interrupt_message: request.runtime.interrupt_message,
            armed: true,
            permit: Some(permit),
        };
        self.pause_before_startup_ack(&request).await;
        if let Some(startup_tx) = request.startup_tx.as_ref() {
            let _ = startup_tx.send(Some(Ok(())));
        }
        if let Some(startup_accept_rx) = request.startup_accept_rx.take() {
            if startup_accept_rx.await.is_err() {
                let cancelled = anyhow::anyhow!(
                    "spawn caller ended before accepting the durable runtime startup"
                );
                self.run_before_cleanup_hook();
                let active_result = self
                    .remove_active_if_turn(&thread_id, &turn_id)
                    .and_then(|removed| {
                        self.run_cleanup_failure_hook("active")?;
                        Ok(removed)
                    })
                    .map(|_| ());
                let runtime_result = control
                    .remove_runtime_if_same(&thread_id, &owner_guard.runtime_handle)
                    .and_then(|removed| {
                        self.run_cleanup_failure_hook("runtime handle")?;
                        Ok(removed)
                    })
                    .map(|_| ());
                owner_guard.release_permit();
                owner_guard.disarm();
                let cleanup_result =
                    combine_completion_results([active_result, runtime_result, Ok(())]);
                let error = match cleanup_result {
                    Ok(()) => cancelled,
                    Err(cleanup_error) => anyhow::anyhow!(
                        "{cancelled:#}; unaccepted startup cleanup failed: {cleanup_error:#}"
                    ),
                };
                owner_guard.publish_failure(&error);
                return Err(error);
            }
            if let Some(cleanup) = request.unaccepted_spawn_cleanup.as_ref() {
                cleanup.accept();
            }
        }

        let result =
            run_request(&request, Arc::clone(&interrupt), prior_turn_was_interrupted).await;
        self.pause_before_terminal_persist().await;
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
        self.run_before_cleanup_hook();
        let mut active_result = self
            .remove_active_and_take_followup(
                &thread_id,
                &turn_id,
                terminal_status != AgentStatusV2::Shutdown,
            )
            .and_then(|(removed, pending)| {
                self.run_cleanup_failure_hook("active")?;
                Ok((removed, pending))
            });
        let runtime_result = control
            .remove_runtime_if_same(&thread_id, &owner_guard.runtime_handle)
            .and_then(|removed| {
                self.run_cleanup_failure_hook("runtime handle")?;
                Ok(removed)
            });
        owner_guard.release_permit();
        owner_guard.disarm();
        let pending_followup = active_result
            .as_mut()
            .ok()
            .and_then(|(_, pending)| pending.take());
        let completion_result = combine_completion_results([
            durable_result,
            active_result.map(|_| ()),
            runtime_result.map(|_| ()),
        ]);
        match &completion_result {
            Ok(()) => owner_guard.publish_termination(terminal_status.clone()),
            Err(error) => owner_guard.publish_failure(error),
        }
        if let Err(error) = completion_result {
            if let Some(pending) = pending_followup.as_ref() {
                self.fail_starting_request(&pending.request, &error);
            }
            return Err(error);
        }

        if let Some(pending) = pending_followup {
            let start_tx = pending.request.followup_start_tx.clone();
            let next_result = if terminal_status == AgentStatusV2::Shutdown {
                Err(anyhow::anyhow!(
                    "cannot start a follow-up after agent runtime shutdown"
                ))
            } else {
                match control.drain_mailbox(&pending.request.thread.canonical_path) {
                    Ok(messages) if messages.is_empty() => {
                        self.complete_starting_request(&pending.request, Ok(()));
                        Ok(())
                    }
                    Ok(_) => Box::pin(self.start_turn(pending.request)).await,
                    Err(error) => {
                        self.fail_starting_request(&pending.request, &error);
                        Err(error)
                    }
                }
            };
            if let Some(start_tx) = start_tx {
                if start_tx.borrow().is_none() {
                    let shared = next_result
                        .as_ref()
                        .map(|_| ())
                        .map_err(|error| format!("{error:#}"));
                    let _ = start_tx.send(Some(shared));
                }
            }
            next_result?;
        }

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
        expect_terminal_ack("interruption", termination, AgentStatusV2::Interrupted)
    }

    pub async fn terminate(&self, thread_id: &str) -> anyhow::Result<()> {
        let (interrupt, terminated) = self.termination_subscription(thread_id)?;
        self.pause_after_ack_subscribe().await;
        interrupt.close();
        let termination = wait_for_termination(terminated, "termination").await?;
        expect_terminal_ack("termination", termination, AgentStatusV2::Shutdown)?;
        Ok(())
    }

    /// Advance one thread close until it is either complete, waiting for a
    /// Starting slot, or has synchronously sent a termination signal.  The
    /// caller can safely transfer its subtree admission guard only after the
    /// `TerminationRequested` result is returned.
    pub(super) async fn begin_close_thread(
        &self,
        control: &subagents::AgentControl,
        thread: &AgentThreadV2,
    ) -> anyhow::Result<CloseThreadStart> {
        let thread_id = thread.thread_id.as_str();
        let current = control.resolve_desktop_target(thread.canonical_path.as_str())?;
        match self.runtime_termination_state(thread_id)? {
            RuntimeTerminationState::Running {
                control: runtime_control,
                terminated,
            } => {
                self.pause_after_ack_subscribe().await;
                runtime_control.close();
                Ok(CloseThreadStart::TerminationRequested(terminated))
            }
            RuntimeTerminationState::Starting => Ok(CloseThreadStart::Starting),
            RuntimeTerminationState::Missing => {
                if current.status == AgentStatusV2::Shutdown
                    && control.runtime_handle(thread_id)?.is_none()
                {
                    return Ok(CloseThreadStart::Complete);
                }

                // No live runner exists to acknowledge shutdown. The durable
                // RuntimeTerminated event is the acknowledgement for this idle
                // generation and atomically closes its spawn edge. Reapplying
                // it to Shutdown also clears a stale runtime handle.
                control.record_runner_event(thread_id, RunnerEvent::RuntimeTerminated)?;
                Ok(CloseThreadStart::Complete)
            }
        }
    }

    pub(super) async fn wait_for_close_ack(
        &self,
        terminated: watch::Receiver<Option<RunnerAck>>,
    ) -> anyhow::Result<()> {
        let termination = wait_for_termination(terminated, "termination").await?;
        expect_terminal_ack("termination", termination, AgentStatusV2::Shutdown)?;
        Ok(())
    }

    pub(super) async fn lock_subtree_close(&self) -> tokio::sync::OwnedMutexGuard<()> {
        Arc::clone(&self.subtree_close).lock_owned().await
    }

    pub fn is_running(&self, thread_id: &str) -> bool {
        self.active
            .lock()
            .map(|active| active.contains_key(thread_id))
            .unwrap_or(false)
    }

    pub fn active_count(&self) -> usize {
        self.active
            .lock()
            .map(|active| active.len())
            .unwrap_or_default()
    }

    /// Atomically decide whether a durable follow-up must start immediately or
    /// be handed off from the current active generation.  The first caller for
    /// an active turn owns the handoff; concurrent callers join its shared
    /// result, so one next turn can consume every ordered mailbox message.
    pub(super) fn request_or_start_followup(
        &self,
        thread_id: &str,
        mut request: RunAgentTurnRequest,
    ) -> anyhow::Result<FollowupAdmission> {
        let mut active = self.lock_active()?;
        let Some(slot) = active.get_mut(thread_id) else {
            let (result_tx, result_rx) = watch::channel(None);
            let token = Uuid::new_v4().to_string();
            request.followup_start_tx = Some(result_tx);
            request.start_token = Some(token.clone());
            active.insert(
                thread_id.to_string(),
                RuntimeSlot::Starting(StartingAgentTurn {
                    token,
                    result_rx: result_rx.clone(),
                }),
            );
            return Ok(FollowupAdmission::StartNow {
                request: Box::new(request),
                result_rx,
            });
        };
        match slot {
            RuntimeSlot::Starting(starting) => Ok(FollowupAdmission::AwaitStart {
                result_rx: starting.result_rx.clone(),
            }),
            RuntimeSlot::Running(turn) => {
                if let Some(handoff) = &turn.pending_followup {
                    return Ok(FollowupAdmission::AwaitStart {
                        result_rx: handoff.result_rx.clone(),
                    });
                }
                let (result_tx, result_rx) = watch::channel(None);
                request.followup_start_tx = Some(result_tx);
                request.start_token = Some(Uuid::new_v4().to_string());
                turn.pending_followup = Some(PendingFollowup {
                    request,
                    result_rx: result_rx.clone(),
                });
                Ok(FollowupAdmission::AwaitStart { result_rx })
            }
        }
    }

    #[cfg(test)]
    pub(super) async fn pause_after_followup_admission(&self) {
        let barrier = self.followup_admission_barrier.lock().unwrap().clone();
        if let Some(barrier) = barrier {
            barrier.wait().await;
        }
    }

    #[cfg(not(test))]
    pub(super) async fn pause_after_followup_admission(&self) {}

    #[cfg(test)]
    async fn pause_before_followup_start(&self, request: &RunAgentTurnRequest) {
        if request.followup_start_tx.is_none() {
            return;
        }
        let hook = self.before_followup_start_hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook.entered.notify_one();
            hook.release.notified().await;
        }
    }

    #[cfg(not(test))]
    async fn pause_before_followup_start(&self, _request: &RunAgentTurnRequest) {}

    #[cfg(test)]
    async fn pause_before_startup_ack(&self, request: &RunAgentTurnRequest) {
        if request.startup_tx.is_none() {
            return;
        }
        let hook = self.before_startup_ack_hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook.entered.notify_one();
            hook.release.notified().await;
        }
    }

    #[cfg(not(test))]
    async fn pause_before_startup_ack(&self, _request: &RunAgentTurnRequest) {}

    #[cfg(test)]
    async fn pause_after_startup_permit(&self, request: &RunAgentTurnRequest) {
        if request.startup_tx.is_none() {
            return;
        }
        let hook = self.after_startup_permit_hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook.entered.notify_one();
            hook.release.notified().await;
        }
    }

    #[cfg(not(test))]
    async fn pause_after_startup_permit(&self, _request: &RunAgentTurnRequest) {}

    fn termination_subscription(
        &self,
        thread_id: &str,
    ) -> anyhow::Result<(Arc<AgentThreadControl>, watch::Receiver<Option<RunnerAck>>)> {
        match self.runtime_termination_state(thread_id)? {
            RuntimeTerminationState::Running {
                control,
                terminated,
            } => Ok((control, terminated)),
            RuntimeTerminationState::Starting => {
                anyhow::bail!("agent thread {thread_id:?} is starting a runtime turn")
            }
            RuntimeTerminationState::Missing => {
                anyhow::bail!("agent thread {thread_id:?} has no active runtime turn")
            }
        }
    }

    fn runtime_termination_state(
        &self,
        thread_id: &str,
    ) -> anyhow::Result<RuntimeTerminationState> {
        let active = self.lock_active()?;
        match active.get(thread_id) {
            Some(RuntimeSlot::Running(turn)) => Ok(RuntimeTerminationState::Running {
                control: Arc::clone(&turn.interrupt),
                terminated: turn.terminated.clone(),
            }),
            Some(RuntimeSlot::Starting(_)) => Ok(RuntimeTerminationState::Starting),
            None => Ok(RuntimeTerminationState::Missing),
        }
    }

    #[cfg(test)]
    async fn pause_after_ack_subscribe(&self) {
        let barrier = self.ack_subscribe_barrier.lock().unwrap().clone();
        if let Some(barrier) = barrier {
            barrier.wait().await;
        }
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
    fn set_ack_subscribe_barrier(&self, barrier: Option<Arc<tokio::sync::Barrier>>) {
        *self.ack_subscribe_barrier.lock().unwrap() = barrier;
    }

    #[cfg(test)]
    fn set_terminal_persistence_hook(&self, hook: Option<TerminalPersistenceHook>) {
        *self.terminal_persistence_hook.lock().unwrap() = hook;
    }

    #[cfg(test)]
    pub(super) fn set_before_terminal_persist_hook(&self, hook: Option<AckSubscribeHook>) {
        *self.before_terminal_persist_hook.lock().unwrap() = hook;
    }

    #[cfg(test)]
    pub(super) fn set_close_timeout(&self, timeout: std::time::Duration) {
        *self.close_timeout.lock().unwrap() = Some(timeout);
    }

    #[cfg(test)]
    pub(super) fn close_timeout(&self) -> std::time::Duration {
        self.close_timeout
            .lock()
            .unwrap()
            .unwrap_or_else(|| std::time::Duration::from_secs(30))
    }

    #[cfg(not(test))]
    pub(super) fn close_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(30)
    }

    #[cfg(test)]
    async fn pause_before_terminal_persist(&self) {
        let hook = self.before_terminal_persist_hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook.entered.notify_one();
            hook.release.notified().await;
        }
    }

    #[cfg(not(test))]
    async fn pause_before_terminal_persist(&self) {}

    #[cfg(test)]
    pub(super) fn set_before_cleanup_hook(&self, hook: Option<BeforeCleanupHook>) {
        *self.before_cleanup_hook.lock().unwrap() = hook;
    }

    #[cfg(test)]
    pub(super) fn set_followup_admission_barrier(
        &self,
        barrier: Option<Arc<tokio::sync::Barrier>>,
    ) {
        *self.followup_admission_barrier.lock().unwrap() = barrier;
    }

    #[cfg(test)]
    pub(super) fn set_before_followup_start_hook(&self, hook: Option<AckSubscribeHook>) {
        *self.before_followup_start_hook.lock().unwrap() = hook;
    }

    #[cfg(test)]
    pub(super) fn set_before_startup_ack_hook(&self, hook: Option<AckSubscribeHook>) {
        *self.before_startup_ack_hook.lock().unwrap() = hook;
    }

    #[cfg(test)]
    pub(super) fn set_after_startup_permit_hook(&self, hook: Option<AckSubscribeHook>) {
        *self.after_startup_permit_hook.lock().unwrap() = hook;
    }

    #[cfg(test)]
    pub(super) fn set_start_status_failure(&self, message: Option<&str>) {
        *self.start_status_failure.lock().unwrap() = message.map(str::to_string);
    }

    #[cfg(test)]
    fn status_events_for_start(
        &self,
        control: &subagents::AgentControl,
        thread_id: &str,
    ) -> anyhow::Result<Vec<subagents::StoredStatusEvent>> {
        if let Some(message) = self.start_status_failure.lock().unwrap().clone() {
            anyhow::bail!(message);
        }
        control.status_events(thread_id)
    }

    #[cfg(not(test))]
    fn status_events_for_start(
        &self,
        control: &subagents::AgentControl,
        thread_id: &str,
    ) -> anyhow::Result<Vec<subagents::StoredStatusEvent>> {
        control.status_events(thread_id)
    }

    #[cfg(test)]
    fn run_before_cleanup_hook(&self) {
        if let Some(hook) = self.before_cleanup_hook.lock().unwrap().clone() {
            hook();
        }
    }

    #[cfg(not(test))]
    fn run_before_cleanup_hook(&self) {}

    #[cfg(test)]
    fn set_cleanup_failure_hook(&self, hook: Option<CleanupFailureHook>) {
        *self.cleanup_failure_hook.lock().unwrap() = hook;
    }

    #[cfg(test)]
    fn run_cleanup_failure_hook(&self, stage: &str) -> anyhow::Result<()> {
        if let Some(hook) = self.cleanup_failure_hook.lock().unwrap().clone() {
            hook(stage)?;
        }
        Ok(())
    }

    #[cfg(not(test))]
    fn run_cleanup_failure_hook(&self, _stage: &str) -> anyhow::Result<()> {
        Ok(())
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
        persist_error_event: bool,
        resources: FailedStartResources<'_>,
    ) -> anyhow::Result<()> {
        let status = AgentStatusV2::Errored {
            message: message.clone(),
        };
        let event_result = if persist_error_event {
            self.record_terminal_event(
                control,
                thread_id,
                RunnerEvent::TurnErrored {
                    turn_id: turn_id.to_string(),
                    message,
                },
            )
            .map(|_| ())
        } else {
            Ok(())
        };
        let active_result = self
            .remove_active_if_turn(thread_id, turn_id)
            .and_then(|removed| {
                self.run_cleanup_failure_hook("active")?;
                Ok(removed)
            })
            .map(|_| ());
        let runtime_result = control
            .remove_runtime_if_same(thread_id, &resources.runtime_handle)
            .and_then(|removed| {
                self.run_cleanup_failure_hook("runtime handle")?;
                Ok(removed)
            })
            .map(|_| ());
        drop(resources.permit);
        let completion_result =
            combine_completion_results([event_result, active_result, runtime_result]);
        match &completion_result {
            Ok(()) => {
                let _ =
                    resources
                        .terminated_tx
                        .send(Some(RunnerAck::Terminated(RunnerTermination {
                            terminal_status: status,
                        })));
            }
            Err(error) => {
                let _ = resources
                    .terminated_tx
                    .send(Some(RunnerAck::Failed(format!("{error:#}"))));
            }
        }
        completion_result
    }

    fn remove_active_if_turn(&self, thread_id: &str, turn_id: &str) -> anyhow::Result<bool> {
        let (removed, pending) = self.remove_active_and_take_followup(thread_id, turn_id, false)?;
        if let Some(pending) = pending {
            if let Some(start_tx) = pending.request.followup_start_tx {
                let _ = start_tx.send(Some(Err(
                    "active runtime ended before follow-up handoff".into()
                )));
            }
        }
        Ok(removed)
    }

    fn remove_active_and_take_followup(
        &self,
        thread_id: &str,
        turn_id: &str,
        reserve_followup: bool,
    ) -> anyhow::Result<(bool, Option<PendingFollowup>)> {
        let mut active = self.lock_active()?;
        let matches = active.get(thread_id).is_some_and(
            |slot| matches!(slot, RuntimeSlot::Running(turn) if turn.turn_id == turn_id),
        );
        let pending = matches
            .then(|| active.remove(thread_id))
            .flatten()
            .and_then(|slot| match slot {
                RuntimeSlot::Running(turn) => turn.pending_followup,
                RuntimeSlot::Starting(_) => None,
            });
        if reserve_followup {
            if let Some(pending) = pending.as_ref() {
                let token = pending
                    .request
                    .start_token
                    .clone()
                    .expect("pending follow-up owns a starting token");
                active.insert(
                    thread_id.to_string(),
                    RuntimeSlot::Starting(StartingAgentTurn {
                        token,
                        result_rx: pending.result_rx.clone(),
                    }),
                );
            }
        }
        Ok((matches, pending))
    }

    fn complete_starting_request(&self, request: &RunAgentTurnRequest, result: Result<(), String>) {
        let Some(token) = request.start_token.as_deref() else {
            return;
        };
        if let Ok(mut active) = self.lock_active() {
            let matches = active.get(&request.thread.thread_id).is_some_and(
                |slot| matches!(slot, RuntimeSlot::Starting(starting) if starting.token == token),
            );
            if matches {
                active.remove(&request.thread.thread_id);
            }
        }
        if let Some(start_tx) = request.followup_start_tx.as_ref() {
            if start_tx.borrow().is_none() {
                let _ = start_tx.send(Some(result));
            }
        }
    }

    fn fail_starting_request(&self, request: &RunAgentTurnRequest, error: &anyhow::Error) {
        self.complete_starting_request(request, Err(format!("{error:#}")));
    }

    fn active_turn_matches(&self, thread_id: &str, turn_id: &str) -> anyhow::Result<bool> {
        Ok(self.lock_active()?.get(thread_id).is_some_and(
            |slot| matches!(slot, RuntimeSlot::Running(turn) if turn.turn_id == turn_id),
        ))
    }

    fn lock_active(
        &self,
    ) -> anyhow::Result<std::sync::MutexGuard<'_, HashMap<String, RuntimeSlot>>> {
        self.active
            .lock()
            .map_err(|_| anyhow::anyhow!("agent runtime manager mutex is poisoned"))
    }
}

fn combine_completion_results(results: [anyhow::Result<()>; 3]) -> anyhow::Result<()> {
    let errors = results
        .into_iter()
        .filter_map(Result::err)
        .map(|error| format!("{error:#}"))
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(errors.join("; "))
    }
}

fn expect_terminal_ack(
    operation: &str,
    termination: RunnerTermination,
    expected: AgentStatusV2,
) -> anyhow::Result<AgentStatusV2> {
    if termination.terminal_status != expected {
        anyhow::bail!(
            "{operation} protocol error: expected {expected:?} acknowledgement, received {:?}",
            termination.terminal_status
        );
    }
    Ok(termination.terminal_status)
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
    prior_turn_was_interrupted: bool,
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
    // Runner events choose the semantic boundary kind only. Whether a boundary
    // is needed is derived idempotently from the hydrated and durable history,
    // so a failed repair remains retryable after it records TurnErrored.
    if prior_turn_was_interrupted {
        session.ensure_assistant_interrupted_boundary().await?;
    } else {
        session.ensure_assistant_error_boundary().await?;
    }
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
    let prepared_system_prompt = if request.consume_mailbox {
        let turn = session.prepare_mailbox_turn().await?;
        let system_prompt = match turn {
            TurnResult::Continue { system_prompt, .. } => system_prompt,
            TurnResult::BudgetExhausted => anyhow::bail!("conversation turn budget exhausted"),
            TurnResult::Interrupted => anyhow::bail!("follow-up turn interrupted while preparing"),
            other => anyhow::bail!("unexpected follow-up preparation result: {other:?}"),
        };
        Some(system_prompt)
    } else {
        None
    };
    let session = Arc::new(tokio::sync::Mutex::new(session));
    if let Some(started) = request.followup_start_tx.as_ref() {
        let _ = started.send(Some(Ok(())));
    }
    let result = if let Some(system_prompt) = prepared_system_prompt {
        crate::exec::background::run_background_prepared_turn_controlled_with_chat(
            Arc::clone(&session),
            targets,
            system_prompt,
            Some(Arc::clone(&interrupt)),
            request.chat_override.clone(),
        )
        .await
    } else {
        crate::exec::background::run_background_multi_turn_controlled_with_chat(
            Arc::clone(&session),
            targets,
            vec![TurnInput::UserInput {
                content: request.runtime.model_request.message.clone(),
                image_data_urls: Vec::new(),
            }],
            Some(Arc::clone(&interrupt)),
            request.chat_override.clone(),
        )
        .await
    };
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
        AgentControl, AgentGraphStore, AgentPath, AgentRuntimeHandle, AgentStatusV2,
        AgentThreadControl, Limits, RunnerEvent, SpawnAgentV2Request, SpawnRuntimeV2Request,
    };

    use tokio::sync::watch;

    use crate::streaming::ChatOverride;

    use super::{
        sandbox_profile, wait_for_termination, AckSubscribeHook, ActiveAgentTurn,
        AgentRuntimeManager, CloseThreadStart, RunAgentTurnRequest, RuntimeSlot,
        StartTurnOwnerGuard, StartingAgentTurn,
    };

    #[test]
    fn custom_parent_sandbox_profile_is_preserved_for_child_session() {
        assert_eq!(sandbox_profile(Some("locked")).as_deref(), Some("locked"));
        assert_eq!(
            sandbox_profile(Some("workspace_write")).as_deref(),
            Some(types::WORKSPACE_PROFILE)
        );
    }

    #[tokio::test]
    async fn close_start_reports_starting_as_a_typed_state_without_mutating_durable_status() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = AgentRuntimeManager::default();
        let (_result_tx, result_rx) = watch::channel(None);
        manager.active.lock().unwrap().insert(
            thread.thread_id.clone(),
            RuntimeSlot::Starting(StartingAgentTurn {
                token: "starting-token".into(),
                result_rx,
            }),
        );

        assert!(matches!(
            manager
                .begin_close_thread(control.as_ref(), &thread)
                .await
                .unwrap(),
            CloseThreadStart::Starting
        ));
        assert_eq!(
            control
                .resolve_desktop_target(thread.canonical_path.as_str())
                .unwrap()
                .status,
            AgentStatusV2::PendingInit
        );
    }

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
            consume_mailbox: false,
            startup_tx: None,
            startup_accept_rx: None,
            unaccepted_spawn_cleanup: None,
            followup_start_tx: None,
            start_token: None,
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
    async fn followup_turn_persists_sequence_marker_before_acknowledging_mailbox() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let memory_dir = dir.path().join("memory");
        control
            .enqueue_message(
                &AgentPath::root(),
                subagents::MessageAgentV2Request {
                    target: thread.canonical_path.to_string(),
                    message: "first follow-up".into(),
                },
                false,
            )
            .unwrap();
        control
            .enqueue_message(
                &AgentPath::root(),
                subagents::MessageAgentV2Request {
                    target: thread.canonical_path.to_string(),
                    message: "second follow-up".into(),
                },
                false,
            )
            .unwrap();
        let mut run = request(
            Arc::clone(&control),
            thread.clone(),
            memory_dir.clone(),
            scripted_chat("done"),
        );
        run.consume_mailbox = true;

        AgentRuntimeManager::default()
            .start_turn(run)
            .await
            .unwrap();

        assert!(control
            .drain_mailbox(&thread.canonical_path)
            .unwrap()
            .is_empty());
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        let user_messages = sessions
            .get_messages(&thread.session_id)
            .unwrap()
            .into_iter()
            .filter(|message| message.role == "user")
            .collect::<Vec<_>>();
        assert_eq!(user_messages.len(), 1);
        assert_eq!(
            user_messages[0].content.as_deref(),
            Some("first follow-up\n\nsecond follow-up")
        );
        assert!(user_messages[0].finish_reason.as_deref().is_some_and(
            |reason| reason.starts_with(crate::exec::subagents::MAILBOX_FINISH_PREFIX)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn followup_retry_after_ack_failure_reuses_marker_without_duplicate_user() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let memory_dir = dir.path().join("memory");
        control
            .enqueue_message(
                &AgentPath::root(),
                subagents::MessageAgentV2Request {
                    target: thread.canonical_path.to_string(),
                    message: "retry-safe follow-up".into(),
                },
                false,
            )
            .unwrap();
        let graph = rusqlite::Connection::open(dir.path().join("agents.db")).unwrap();
        graph
            .execute_batch(
                "CREATE TRIGGER fail_mailbox_ack
                 BEFORE UPDATE OF delivery_state ON agent_mailbox
                 WHEN NEW.delivery_state = 'delivered'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected mailbox ack failure');
                 END;",
            )
            .unwrap();
        let mut first = request(
            Arc::clone(&control),
            thread.clone(),
            memory_dir.clone(),
            scripted_chat("must not sample"),
        );
        first.consume_mailbox = true;
        let first_error = AgentRuntimeManager::default()
            .start_turn(first)
            .await
            .unwrap_err();
        assert!(format!("{first_error:#}").contains("injected mailbox ack failure"));
        graph
            .execute_batch("DROP TRIGGER fail_mailbox_ack;")
            .unwrap();

        let retry_thread = control
            .resolve_target(&AgentPath::root(), "worker")
            .unwrap();
        let mut retry = request(
            Arc::clone(&control),
            retry_thread,
            memory_dir.clone(),
            scripted_chat("done"),
        );
        retry.consume_mailbox = true;
        AgentRuntimeManager::default()
            .start_turn(retry)
            .await
            .unwrap();

        assert!(control
            .drain_mailbox(&thread.canonical_path)
            .unwrap()
            .is_empty());
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        let user_messages = sessions
            .get_messages(&thread.session_id)
            .unwrap()
            .into_iter()
            .filter(|message| message.role == "user")
            .collect::<Vec<_>>();
        assert_eq!(user_messages.len(), 1);
        assert_eq!(
            user_messages[0].content.as_deref(),
            Some("retry-safe follow-up")
        );
        assert!(user_messages[0].finish_reason.as_deref().is_some_and(
            |reason| reason.starts_with(crate::exec::subagents::MAILBOX_FINISH_PREFIX)
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn turn_started_persistence_failure_is_shared_with_all_waiters_after_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = Arc::new(AgentRuntimeManager::default());
        let persistence_entered = Arc::new(std::sync::Barrier::new(2));
        let persistence_release = Arc::new(std::sync::Barrier::new(2));
        manager.set_terminal_persistence_hook(Some(Arc::new({
            let persistence_entered = Arc::clone(&persistence_entered);
            let persistence_release = Arc::clone(&persistence_release);
            move |event| {
                if matches!(event, RunnerEvent::TurnStarted { .. }) {
                    persistence_entered.wait();
                    persistence_release.wait();
                    anyhow::bail!("injected TurnStarted persistence failure");
                }
                Ok(())
            }
        })));
        let start = tokio::spawn({
            let manager = Arc::clone(&manager);
            let control = Arc::clone(&control);
            let thread = thread.clone();
            let memory_dir = dir.path().join("memory");
            async move {
                manager
                    .start_turn(request(control, thread, memory_dir, pending_chat()))
                    .await
            }
        });
        tokio::task::spawn_blocking({
            let persistence_entered = Arc::clone(&persistence_entered);
            move || persistence_entered.wait()
        })
        .await
        .unwrap();

        assert!(manager.is_running(&thread.thread_id));
        let (_, observer_one) = manager.termination_subscription(&thread.thread_id).unwrap();
        let (_, observer_two) = manager.termination_subscription(&thread.thread_id).unwrap();
        let subscribe_hook = AckSubscribeHook {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        manager.set_ack_subscribe_hook(Some(subscribe_hook.clone()));
        let interrupt = tokio::spawn({
            let manager = Arc::clone(&manager);
            let thread_id = thread.thread_id.clone();
            async move { manager.interrupt(&thread_id).await }
        });
        subscribe_hook.entered.notified().await;
        subscribe_hook.release.notify_one();
        let terminate = tokio::spawn({
            let manager = Arc::clone(&manager);
            let thread_id = thread.thread_id.clone();
            async move { manager.terminate(&thread_id).await }
        });
        subscribe_hook.entered.notified().await;
        subscribe_hook.release.notify_one();
        let observer_one =
            tokio::spawn(async move { wait_for_termination(observer_one, "first observer").await });
        let observer_two =
            tokio::spawn(
                async move { wait_for_termination(observer_two, "second observer").await },
            );

        tokio::task::spawn_blocking({
            let persistence_release = Arc::clone(&persistence_release);
            move || persistence_release.wait()
        })
        .await
        .unwrap();

        let start_error = start.await.unwrap().unwrap_err();
        let interrupt_error = interrupt.await.unwrap().unwrap_err();
        let terminate_error = terminate.await.unwrap().unwrap_err();
        let observer_one_error = observer_one.await.unwrap().unwrap_err();
        let observer_two_error = observer_two.await.unwrap().unwrap_err();
        for error in [
            &start_error,
            &interrupt_error,
            &terminate_error,
            &observer_one_error,
            &observer_two_error,
        ] {
            assert!(
                format!("{error:#}").contains("injected TurnStarted persistence failure"),
                "unexpected error: {error:#}"
            );
        }
        assert!(!manager.is_running(&thread.thread_id));
        assert!(control.runtime_handle(&thread.thread_id).unwrap().is_none());
        assert!(control.status_events(&thread.thread_id).unwrap().is_empty());
        let permit = control.acquire_execution(&thread.thread_id).unwrap();
        drop(permit);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn recovered_running_turn_inserts_interrupted_boundary_before_follow_up() {
        let dir = tempfile::tempdir().unwrap();
        let graph_path = dir.path().join("agents.db");
        let store = AgentGraphStore::open(graph_path.clone()).unwrap();
        let initial_control = AgentControl::open(
            "root".into(),
            store,
            Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 1,
            },
        )
        .unwrap();
        let reservation = initial_control
            .reserve_spawn(&AgentPath::root(), "worker")
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().unwrap();
        initial_control
            .record_runner_event(
                &thread.thread_id,
                RunnerEvent::TurnStarted {
                    turn_id: "crashed-turn".into(),
                },
            )
            .unwrap();
        drop(initial_control);

        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions
            .ensure_session(&thread.session_id, "tauri")
            .unwrap();
        sessions
            .append_message(session::NewMessage {
                content: Some("unfinished request"),
                ..session::NewMessage::empty(&thread.session_id, "user")
            })
            .unwrap();

        let recovered = crate::exec::agent_control_directory::AgentControlDirectory::global()
            .open_root_at("root", &graph_path)
            .unwrap();
        let recovered_thread = recovered
            .resolve_target(&AgentPath::root(), "worker")
            .unwrap();
        assert_eq!(recovered_thread.status, AgentStatusV2::Interrupted);
        let captured_roles = Arc::new(std::sync::Mutex::new(Vec::new()));
        AgentRuntimeManager::default()
            .start_turn(request(
                Arc::clone(&recovered),
                recovered_thread,
                memory_dir,
                role_capturing_chat(Arc::clone(&captured_roles)),
            ))
            .await
            .unwrap();

        assert_eq!(
            captured_roles.lock().unwrap().as_slice(),
            &["user", "assistant", "user"]
        );
        let roles = sessions
            .get_messages(&thread.session_id)
            .unwrap()
            .into_iter()
            .map(|message| message.role)
            .collect::<Vec<_>>();
        assert_eq!(roles, ["user", "assistant", "user", "assistant"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_recovered_boundary_is_retried_after_turn_errored() {
        let dir = tempfile::tempdir().unwrap();
        let graph_path = dir.path().join("agents.db");
        let store = AgentGraphStore::open(graph_path.clone()).unwrap();
        let initial_control = AgentControl::open(
            "root".into(),
            store,
            Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 1,
            },
        )
        .unwrap();
        let reservation = initial_control
            .reserve_spawn(&AgentPath::root(), "worker")
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().unwrap();
        initial_control
            .record_runner_event(
                &thread.thread_id,
                RunnerEvent::TurnStarted {
                    turn_id: "crashed-turn".into(),
                },
            )
            .unwrap();
        drop(initial_control);

        let memory_dir = dir.path().join("memory");
        let state_path = memory_dir.join("sessions/state.db");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions
            .ensure_session(&thread.session_id, "tauri")
            .unwrap();
        sessions
            .append_message(session::NewMessage {
                content: Some("unfinished request"),
                ..session::NewMessage::empty(&thread.session_id, "user")
            })
            .unwrap();
        let raw = rusqlite::Connection::open(&state_path).unwrap();
        raw.execute_batch(&format!(
            "CREATE TRIGGER fail_interrupted_boundary
             BEFORE INSERT ON messages
             WHEN NEW.session_id = '{}' AND NEW.role = 'assistant'
                  AND NEW.finish_reason = 'interrupted'
             BEGIN
               SELECT RAISE(ABORT, 'injected interrupted boundary failure');
             END;",
            thread.session_id.replace('\'', "''")
        ))
        .unwrap();

        let recovered = crate::exec::agent_control_directory::AgentControlDirectory::global()
            .open_root_at("root", &graph_path)
            .unwrap();
        let manager = AgentRuntimeManager::default();
        let recovered_thread = recovered
            .resolve_target(&AgentPath::root(), "worker")
            .unwrap();
        let first_error = manager
            .start_turn(request(
                Arc::clone(&recovered),
                recovered_thread,
                memory_dir.clone(),
                scripted_chat("unreachable"),
            ))
            .await
            .unwrap_err();
        assert!(first_error
            .to_string()
            .contains("injected interrupted boundary failure"));
        assert!(matches!(
            recovered
                .status_events(&thread.thread_id)
                .unwrap()
                .last()
                .map(|event| &event.event),
            Some(RunnerEvent::TurnErrored { .. })
        ));
        assert!(!manager.is_running(&thread.thread_id));
        assert!(recovered
            .runtime_handle(&thread.thread_id)
            .unwrap()
            .is_none());
        let permit = recovered.acquire_execution(&thread.thread_id).unwrap();
        drop(permit);

        raw.execute_batch("DROP TRIGGER fail_interrupted_boundary;")
            .unwrap();
        let captured_roles = Arc::new(std::sync::Mutex::new(Vec::new()));
        let retry_thread = recovered
            .resolve_target(&AgentPath::root(), "worker")
            .unwrap();
        manager
            .start_turn(request(
                Arc::clone(&recovered),
                retry_thread,
                memory_dir,
                role_capturing_chat(Arc::clone(&captured_roles)),
            ))
            .await
            .unwrap();

        assert_eq!(
            captured_roles.lock().unwrap().as_slice(),
            &["user", "assistant", "user"]
        );
        let roles = sessions
            .get_messages(&thread.session_id)
            .unwrap()
            .into_iter()
            .map(|message| message.role)
            .collect::<Vec<_>>();
        assert_eq!(roles, ["user", "assistant", "user", "assistant"]);
        assert!(roles.windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn success_ack_is_published_only_after_runtime_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = Arc::new(AgentRuntimeManager::default());
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let ack_before_cleanup = Arc::new(AtomicBool::new(false));
        let active_before_cleanup = Arc::new(AtomicBool::new(false));
        let handle_before_cleanup = Arc::new(AtomicBool::new(false));
        let permit_before_cleanup = Arc::new(AtomicBool::new(false));
        let run = request(
            Arc::clone(&control),
            thread.clone(),
            dir.path().join("memory"),
            gated_scripted_chat(Arc::clone(&entered), Arc::clone(&release), "finished"),
        );

        let (run_result, observed_ack) = tokio::join!(manager.start_turn(run), async {
            entered.notified().await;
            let (_, observer) = manager.termination_subscription(&thread.thread_id).unwrap();
            let observer_for_hook = observer.clone();
            let manager_for_hook = Arc::clone(&manager);
            let control_for_hook = Arc::clone(&control);
            let thread_id = thread.thread_id.clone();
            let ack_before_cleanup_for_hook = Arc::clone(&ack_before_cleanup);
            let active_before_cleanup_for_hook = Arc::clone(&active_before_cleanup);
            let handle_before_cleanup_for_hook = Arc::clone(&handle_before_cleanup);
            let permit_before_cleanup_for_hook = Arc::clone(&permit_before_cleanup);
            manager.set_before_cleanup_hook(Some(Arc::new(move || {
                ack_before_cleanup_for_hook
                    .store(observer_for_hook.borrow().is_some(), Ordering::SeqCst);
                active_before_cleanup_for_hook
                    .store(manager_for_hook.is_running(&thread_id), Ordering::SeqCst);
                handle_before_cleanup_for_hook.store(
                    control_for_hook
                        .runtime_handle(&thread_id)
                        .unwrap()
                        .is_some(),
                    Ordering::SeqCst,
                );
                permit_before_cleanup_for_hook.store(
                    control_for_hook.acquire_execution(&thread_id).is_ok(),
                    Ordering::SeqCst,
                );
            })));
            release.notify_one();
            wait_for_termination(observer, "cleanup ordering observer").await
        });

        run_result.unwrap();
        assert_eq!(
            observed_ack.unwrap().terminal_status,
            AgentStatusV2::Completed {
                last_message: "finished".into()
            }
        );
        assert!(!ack_before_cleanup.load(Ordering::SeqCst));
        assert!(active_before_cleanup.load(Ordering::SeqCst));
        assert!(handle_before_cleanup.load(Ordering::SeqCst));
        assert!(!permit_before_cleanup.load(Ordering::SeqCst));
        assert!(!manager.is_running(&thread.thread_id));
        assert!(control.runtime_handle(&thread.thread_id).unwrap().is_none());
        let permit = control.acquire_execution(&thread.thread_id).unwrap();
        drop(permit);
        manager.set_before_cleanup_hook(None);
        manager
            .start_turn(request(
                Arc::clone(&control),
                thread,
                dir.path().join("memory"),
                scripted_chat("follow-up completed"),
            ))
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cleanup_failures_publish_one_shared_failed_ack_after_best_effort_cleanup() {
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

        let (run_result, (observed_ack, second_observer)) =
            tokio::join!(manager.start_turn(run), async {
                entered.notified().await;
                let (_, observer) = manager.termination_subscription(&thread.thread_id).unwrap();
                let second_observer = observer.clone();
                manager.set_cleanup_failure_hook(Some(Arc::new(|stage| {
                    anyhow::bail!("injected {stage} cleanup failure")
                })));
                release.notify_one();
                (
                    wait_for_termination(observer, "cleanup failure observer").await,
                    second_observer,
                )
            });

        let run_error = run_result.unwrap_err().to_string();
        let ack_error = observed_ack.unwrap_err().to_string();
        let second_error = wait_for_termination(second_observer, "second cleanup failure observer")
            .await
            .unwrap_err()
            .to_string();
        for error in [&run_error, &ack_error, &second_error] {
            assert!(error.contains("injected active cleanup failure"));
            assert!(error.contains("injected runtime handle cleanup failure"));
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
            AgentStatusV2::Completed {
                last_message: "finished".into()
            }
        );
        manager.set_cleanup_failure_hook(None);
        manager
            .start_turn(request(
                Arc::clone(&control),
                thread,
                dir.path().join("memory"),
                scripted_chat("follow-up completed"),
            ))
            .await
            .unwrap();
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn terminate_owner_abort_race_durably_shutdowns_and_closes_edge() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = Arc::new(AgentRuntimeManager::default());
        let provider_ready = Arc::new(tokio::sync::Barrier::new(2));
        let terminal_entered = Arc::new(tokio::sync::Notify::new());
        manager.set_before_terminal_persist_hook(Some(AckSubscribeHook {
            entered: Arc::clone(&terminal_entered),
            release: Arc::new(tokio::sync::Notify::new()),
        }));
        let owner = tokio::spawn({
            let manager = Arc::clone(&manager);
            let request = request(
                Arc::clone(&control),
                thread.clone(),
                dir.path().join("memory"),
                barrier_pending_chat(Arc::clone(&provider_ready)),
            );
            async move { manager.start_turn(request).await }
        });

        provider_ready.wait().await;
        let (runtime_control, observer_one) =
            manager.termination_subscription(&thread.thread_id).unwrap();
        let observer_two = observer_one.clone();
        let terminate = tokio::spawn({
            let manager = Arc::clone(&manager);
            let thread_id = thread.thread_id.clone();
            async move { manager.terminate(&thread_id).await }
        });
        terminal_entered.notified().await;
        assert!(runtime_control.is_closed());

        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());
        terminate.await.unwrap().unwrap();
        for (observer, operation) in [
            (observer_one, "terminate owner-abort observer"),
            (observer_two, "second terminate owner-abort observer"),
        ] {
            assert_eq!(
                wait_for_termination(observer, operation)
                    .await
                    .unwrap()
                    .terminal_status,
                AgentStatusV2::Shutdown
            );
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
            AgentStatusV2::Shutdown
        );
        let edge_state: String = rusqlite::Connection::open(dir.path().join("agents.db"))
            .unwrap()
            .query_row(
                "SELECT edge_state FROM agent_spawn_edges WHERE child_thread_id = ?1",
                [&thread.thread_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(edge_state, "closed");
        assert!(matches!(
            control
                .status_events(&thread.thread_id)
                .unwrap()
                .last()
                .map(|event| &event.event),
            Some(RunnerEvent::RuntimeTerminated)
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn terminate_owner_abort_race_reports_runtime_terminated_failure() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = Arc::new(AgentRuntimeManager::default());
        let provider_ready = Arc::new(tokio::sync::Barrier::new(2));
        let terminal_entered = Arc::new(tokio::sync::Notify::new());
        manager.set_before_terminal_persist_hook(Some(AckSubscribeHook {
            entered: Arc::clone(&terminal_entered),
            release: Arc::new(tokio::sync::Notify::new()),
        }));
        manager.set_terminal_persistence_hook(Some(Arc::new(|event| {
            if matches!(event, RunnerEvent::RuntimeTerminated) {
                anyhow::bail!("injected owner-drop RuntimeTerminated failure");
            }
            Ok(())
        })));
        let owner = tokio::spawn({
            let manager = Arc::clone(&manager);
            let request = request(
                Arc::clone(&control),
                thread.clone(),
                dir.path().join("memory"),
                barrier_pending_chat(Arc::clone(&provider_ready)),
            );
            async move { manager.start_turn(request).await }
        });

        provider_ready.wait().await;
        let terminate = tokio::spawn({
            let manager = Arc::clone(&manager);
            let thread_id = thread.thread_id.clone();
            async move { manager.terminate(&thread_id).await }
        });
        terminal_entered.notified().await;
        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());
        assert!(terminate
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("owner-drop RuntimeTerminated failure"));

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
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn interrupt_rejects_shutdown_ack_as_protocol_error() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = Arc::new(AgentRuntimeManager::default());
        let provider_ready = Arc::new(tokio::sync::Barrier::new(2));
        let owner = tokio::spawn({
            let manager = Arc::clone(&manager);
            let request = request(
                Arc::clone(&control),
                thread.clone(),
                dir.path().join("memory"),
                barrier_pending_chat(Arc::clone(&provider_ready)),
            );
            async move { manager.start_turn(request).await }
        });

        provider_ready.wait().await;
        let (runtime_control, _) = manager.termination_subscription(&thread.thread_id).unwrap();
        let subscribe_entered = Arc::new(tokio::sync::Notify::new());
        let subscribe_release = Arc::new(tokio::sync::Notify::new());
        manager.set_ack_subscribe_hook(Some(AckSubscribeHook {
            entered: Arc::clone(&subscribe_entered),
            release: Arc::clone(&subscribe_release),
        }));
        let interrupt = tokio::spawn({
            let manager = Arc::clone(&manager);
            let thread_id = thread.thread_id.clone();
            async move { manager.interrupt(&thread_id).await }
        });
        subscribe_entered.notified().await;
        runtime_control.close();
        subscribe_release.notify_one();

        owner.await.unwrap().unwrap();
        let error = interrupt.await.unwrap().unwrap_err();
        assert!(error.to_string().contains("interruption protocol error"));
        assert!(error.to_string().contains("Shutdown"));
        assert_eq!(
            control
                .resolve_target(&AgentPath::root(), "worker")
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn aborted_owner_cleanup_failures_publish_one_shared_failed_ack() {
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
        manager.set_cleanup_failure_hook(Some(Arc::new(|stage| {
            anyhow::bail!("injected owner-drop {stage} cleanup failure")
        })));
        let (_, termination) = manager.termination_subscription(&thread.thread_id).unwrap();
        let second_observer = termination.clone();
        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());

        for (observer, operation) in [
            (termination, "owner cleanup failure"),
            (second_observer, "second owner cleanup failure"),
        ] {
            let error = wait_for_termination(observer, operation)
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains("injected owner-drop active cleanup failure"));
            assert!(error.contains("injected owner-drop runtime handle cleanup failure"));
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
            AgentStatusV2::Interrupted
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_owner_drop_preserves_replacement_generation_and_runtime_handle() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = AgentRuntimeManager::default();
        let stale_turn_id = "stale-turn".to_string();
        control
            .record_runner_event(
                &thread.thread_id,
                RunnerEvent::TurnStarted {
                    turn_id: stale_turn_id.clone(),
                },
            )
            .unwrap();
        let permit = control.acquire_execution(&thread.thread_id).unwrap();
        let stale_handle = AgentRuntimeHandle {
            interrupt: Arc::new(|| {}),
            terminate: Arc::new(|| {}),
        };
        let replacement_handle = AgentRuntimeHandle {
            interrupt: Arc::new(|| {}),
            terminate: Arc::new(|| {}),
        };
        control
            .register_runtime(&thread.thread_id, stale_handle.clone())
            .unwrap();
        control.remove_runtime(&thread.thread_id).unwrap();
        control
            .register_runtime(&thread.thread_id, replacement_handle.clone())
            .unwrap();
        let replacement_control = Arc::new(AgentThreadControl::default());
        let (_replacement_tx, replacement_rx) = watch::channel(None);
        manager.lock_active().unwrap().insert(
            thread.thread_id.clone(),
            RuntimeSlot::Running(Box::new(ActiveAgentTurn {
                turn_id: "replacement-turn".into(),
                interrupt: Arc::clone(&replacement_control),
                terminated: replacement_rx,
                pending_followup: None,
            })),
        );
        let (stale_tx, stale_rx) = watch::channel(None);

        drop(StartTurnOwnerGuard {
            manager: &manager,
            control: control.as_ref(),
            thread_id: thread.thread_id.clone(),
            turn_id: stale_turn_id,
            runtime_control: Arc::new(AgentThreadControl::default()),
            runtime_handle: stale_handle,
            terminated_tx: stale_tx,
            memory_dir: dir.path().join("memory"),
            session_id: thread.session_id.clone(),
            interrupt_message: true,
            armed: true,
            permit: Some(permit),
        });

        assert!(wait_for_termination(stale_rx, "stale owner")
            .await
            .unwrap_err()
            .to_string()
            .contains("no longer owns its active runtime generation"));
        assert!(matches!(
            manager
                .lock_active()
                .unwrap()
                .get(&thread.thread_id),
            Some(RuntimeSlot::Running(turn)) if turn.turn_id == "replacement-turn"
        ));
        let current = control.runtime_handle(&thread.thread_id).unwrap().unwrap();
        assert!(Arc::ptr_eq(
            &current.terminate,
            &replacement_handle.terminate
        ));
        assert!(Arc::ptr_eq(
            &current.interrupt,
            &replacement_handle.interrupt
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_starting_token_cannot_claim_or_remove_replacement_reservation() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session(&thread.session_id, "test").unwrap();
        let manager = AgentRuntimeManager::default();
        let admission = manager
            .request_or_start_followup(
                &thread.thread_id,
                request(
                    Arc::clone(&control),
                    thread.clone(),
                    memory_dir.clone(),
                    scripted_chat("replacement completed"),
                ),
            )
            .unwrap();
        let (owner, result_rx) = match admission {
            super::FollowupAdmission::StartNow { request, result_rx } => (request, result_rx),
            super::FollowupAdmission::AwaitStart { .. } => panic!("first admission must own start"),
        };
        let owner_token = owner.start_token.clone().unwrap();
        assert!(!manager
            .remove_active_if_turn(&thread.thread_id, "stale-running-turn")
            .unwrap());

        let mut stale = request(
            Arc::clone(&control),
            thread.clone(),
            memory_dir,
            scripted_chat("must not run"),
        );
        let (stale_tx, _stale_rx) = watch::channel(None);
        stale.followup_start_tx = Some(stale_tx);
        stale.start_token = Some("stale-token".into());
        let error = manager.start_turn(stale).await.unwrap_err();
        assert!(format!("{error:#}").contains("no longer owns"));
        assert!(matches!(
            manager.lock_active().unwrap().get(&thread.thread_id),
            Some(RuntimeSlot::Starting(starting)) if starting.token == owner_token
        ));

        manager.start_turn(*owner).await.unwrap();
        assert_eq!(result_rx.borrow().clone(), Some(Ok(())));
        assert!(!manager.is_running(&thread.thread_id));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn starting_admission_failure_removes_matching_slot_and_shares_error() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = AgentRuntimeManager::default();
        let permit = control.acquire_execution(&thread.thread_id).unwrap();
        let admission = manager
            .request_or_start_followup(
                &thread.thread_id,
                request(
                    Arc::clone(&control),
                    thread.clone(),
                    dir.path().join("memory"),
                    scripted_chat("must not run"),
                ),
            )
            .unwrap();
        let (owner, mut result_rx) = match admission {
            super::FollowupAdmission::StartNow { request, result_rx } => (request, result_rx),
            super::FollowupAdmission::AwaitStart { .. } => panic!("first admission must own start"),
        };
        let error = manager.start_turn(*owner).await.unwrap_err();
        assert!(format!("{error:#}").contains("active execution"));
        if result_rx.borrow().is_none() {
            result_rx.changed().await.unwrap();
        }
        assert!(
            matches!(result_rx.borrow().as_ref(), Some(Err(message)) if message.contains("active execution"))
        );
        assert!(!manager.is_running(&thread.thread_id));
        drop(permit);
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn concurrent_terminate_callers_share_one_shutdown_ack() {
        let dir = tempfile::tempdir().unwrap();
        let (control, thread) = setup(&dir, "worker");
        let manager = Arc::new(AgentRuntimeManager::default());
        let provider_ready = Arc::new(tokio::sync::Barrier::new(2));
        let owner = tokio::spawn({
            let manager = Arc::clone(&manager);
            let request = request(
                Arc::clone(&control),
                thread.clone(),
                dir.path().join("memory"),
                barrier_pending_chat(Arc::clone(&provider_ready)),
            );
            async move { manager.start_turn(request).await }
        });

        provider_ready.wait().await;
        let subscribe_barrier = Arc::new(tokio::sync::Barrier::new(3));
        manager.set_ack_subscribe_barrier(Some(Arc::clone(&subscribe_barrier)));
        let first = tokio::spawn({
            let manager = Arc::clone(&manager);
            let thread_id = thread.thread_id.clone();
            async move { manager.terminate(&thread_id).await }
        });
        let second = tokio::spawn({
            let manager = Arc::clone(&manager);
            let thread_id = thread.thread_id.clone();
            async move { manager.terminate(&thread_id).await }
        });
        subscribe_barrier.wait().await;

        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();
        owner.await.unwrap().unwrap();
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
                .filter(|event| matches!(event.event, RunnerEvent::RuntimeTerminated))
                .count(),
            1
        );
        assert!(manager
            .terminate(&thread.thread_id)
            .await
            .unwrap_err()
            .to_string()
            .contains("no active runtime turn"));
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
