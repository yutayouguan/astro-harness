//! One long-lived Codex-style Thread event connection for the desktop shell.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Weak};
use std::time::Duration;

use agent_protocol::TurnItem;
use proto::astro_service_client::AstroServiceClient;
use serde::Serialize;
use server::WORKSPACE_EVENT_THREAD_ID;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, watch, Mutex, RwLock};
use tracing::debug;

use super::grpc::{default_grpc_address, endpoint_url};
use crate::commands::chat::{
    ChatStreamEvent, ContextUsageItemDto, ContextUsageSegmentDto, MediaAssetDto,
};

const SNAPSHOT_EVENT: &str = "thread_snapshot";
const SESSION_EVENT: &str = "session_event";
pub(crate) const THREAD_EVENTS_READY_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionEventDto {
    pub session_id: Option<String>,
    pub agent_id: String,
    pub ts_ms: i64,
    pub memory_updated: Option<MemoryUpdatedDto>,
    pub pending_changed: Option<PendingChangedDto>,
    pub session_metadata_changed: Option<SessionMetadataChangedDto>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryUpdatedDto {
    pub source: String,
    pub target: String,
    pub summary: String,
    pub live_written: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingChangedDto {
    pub pending_count: u32,
    pub reason: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionMetadataChangedDto {
    pub title: String,
}

pub(crate) fn now_ts_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub(crate) fn emit_session_event(app: &AppHandle, event: SessionEventDto) {
    let _ = app.emit(SESSION_EVENT, event);
}

#[derive(Default)]
struct ActiveState {
    threads: HashSet<String>,
    background_pending: HashMap<String, HashSet<String>>,
    background_subscriptions: HashMap<String, u64>,
    turn_epochs: HashMap<String, HashMap<String, u64>>,
    activations: HashMap<String, u64>,
    awaiting_submissions: HashMap<String, HashSet<u64>>,
    deferred_terminals: HashMap<String, HashMap<String, DeferredTerminal>>,
    delivered_agent_text: HashMap<String, HashMap<String, String>>,
    delivered_reasoning: HashMap<String, HashMap<String, String>>,
    pending_terminal_errors: HashMap<String, HashMap<String, String>>,
    delivered_extensions: HashMap<(String, String), String>,
    next_activation: u64,
}

struct DeferredTerminal {
    awaiting_activation: u64,
    background_turn: bool,
    events: Vec<ChatStreamEvent>,
}

struct RecoveredExtension {
    turn_id: String,
    extension: proto::ThreadExtension,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalUnsubscribeRequest {
    thread_id: String,
    activation: u64,
}

type TerminalGateRegistry = Arc<std::sync::Mutex<HashMap<String, Weak<Mutex<()>>>>>;

struct TerminalGateLease {
    thread_id: String,
    gate: Arc<Mutex<()>>,
    registry: TerminalGateRegistry,
}

impl std::ops::Deref for TerminalGateLease {
    type Target = Mutex<()>;

    fn deref(&self) -> &Self::Target {
        &self.gate
    }
}

impl Drop for TerminalGateLease {
    fn drop(&mut self) {
        let Ok(mut registry) = self.registry.lock() else {
            return;
        };
        let is_current = registry
            .get(&self.thread_id)
            .is_some_and(|current| Weak::ptr_eq(current, &Arc::downgrade(&self.gate)));
        if is_current && Arc::strong_count(&self.gate) == 1 {
            registry.remove(&self.thread_id);
        }
    }
}

struct TerminalSubscriptionCleanup {
    owners: RwLock<HashMap<String, u64>>,
    gates: TerminalGateRegistry,
    requests: mpsc::UnboundedSender<TerminalUnsubscribeRequest>,
    receiver: std::sync::Mutex<Option<mpsc::UnboundedReceiver<TerminalUnsubscribeRequest>>>,
}

impl TerminalSubscriptionCleanup {
    fn new() -> Self {
        let (requests, receiver) = mpsc::unbounded_channel();
        Self {
            owners: RwLock::new(HashMap::new()),
            gates: Arc::new(std::sync::Mutex::new(HashMap::new())),
            requests,
            receiver: std::sync::Mutex::new(Some(receiver)),
        }
    }

    fn gate(&self, thread_id: &str) -> TerminalGateLease {
        let mut registry = self
            .gates
            .lock()
            .expect("terminal subscription gate lock poisoned");
        registry.retain(|_, gate| gate.strong_count() > 0);
        let gate = registry
            .get(thread_id)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let gate = Arc::new(Mutex::new(()));
                registry.insert(thread_id.into(), Arc::downgrade(&gate));
                gate
            });
        drop(registry);
        TerminalGateLease {
            thread_id: thread_id.into(),
            gate,
            registry: Arc::clone(&self.gates),
        }
    }

    fn take_requests(&self) -> mpsc::UnboundedReceiver<TerminalUnsubscribeRequest> {
        self.receiver
            .lock()
            .expect("terminal unsubscribe receiver lock poisoned")
            .take()
            .expect("terminal unsubscribe receiver already taken")
    }
}

/// Process-wide connection state shared by chat commands and the event pump.
pub struct ThreadEventsBridge {
    connection_id: String,
    ready: watch::Sender<bool>,
    active_threads: RwLock<ActiveState>,
    terminal_cleanup: TerminalSubscriptionCleanup,
}

impl Default for ThreadEventsBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadEventsBridge {
    pub fn new() -> Self {
        let (ready, _) = watch::channel(false);
        Self {
            connection_id: uuid::Uuid::new_v4().to_string(),
            ready,
            active_threads: RwLock::new(ActiveState::default()),
            terminal_cleanup: TerminalSubscriptionCleanup::new(),
        }
    }

    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub async fn wait_ready(&self) {
        let mut ready = self.ready.subscribe();
        loop {
            if *ready.borrow_and_update() {
                return;
            }
            if ready.changed().await.is_err() {
                return;
            }
        }
    }

    pub async fn wait_ready_for(&self, timeout: Duration) -> Result<(), String> {
        tokio::time::timeout(timeout, self.wait_ready())
            .await
            .map_err(|_| "thread event backend did not become ready in time".to_string())
    }

    fn set_ready(&self, value: bool) {
        self.ready.send_replace(value);
    }

    fn mark_recovering(&self) {
        self.set_ready(false);
    }

    fn mark_recovered(&self) {
        self.set_ready(true);
    }

    fn complete_recovery(&self, result: Result<(), String>) -> Result<(), String> {
        result?;
        self.mark_recovered();
        Ok(())
    }

    #[cfg(test)]
    fn is_ready(&self) -> bool {
        *self.ready.borrow()
    }

    pub async fn activate(&self, thread_id: impl Into<String>) -> u64 {
        let thread_id = thread_id.into();
        let gate = self.terminal_cleanup.gate(&thread_id);
        let _owner_guard = gate.lock().await;
        let mut state = self.active_threads.write().await;
        state.next_activation = state.next_activation.wrapping_add(1).max(1);
        let activation = state.next_activation;
        state.threads.insert(thread_id.clone());
        state.background_subscriptions.remove(&thread_id);
        state.deferred_terminals.remove(&thread_id);
        state
            .awaiting_submissions
            .entry(thread_id.clone())
            .or_default()
            .insert(activation);
        state.activations.insert(thread_id.clone(), activation);
        drop(state);
        self.terminal_cleanup
            .owners
            .write()
            .await
            .insert(thread_id, activation);
        activation
    }

    /// Retire a failed activation and let the generation-owned cleanup worker unsubscribe it.
    /// A stale failure only removes its own bookkeeping and cannot touch the current subscriber.
    pub async fn fail_activation(&self, thread_id: &str, activation: u64) -> bool {
        let mut state = self.active_threads.write().await;
        if state.activations.get(thread_id).copied() != Some(activation) {
            Self::remove_turn_epoch(&mut state, thread_id, activation);
            Self::remove_deferred_activation(&mut state, thread_id, activation);
            return false;
        }
        Self::clear_thread(&mut state, thread_id);
        drop(state);
        // Keep terminal_cleanup.owners intact until the queued RPC succeeds. A newer activation
        // overwrites the owner first, making this request stale without unsubscribing the new turn.
        self.queue_terminal_unsubscribe(thread_id, activation);
        true
    }

    /// Bind a turn observed from snapshot/live delivery without rewriting an existing epoch.
    pub async fn bind_observed_turn(&self, thread_id: &str, turn_id: &str) {
        if turn_id.is_empty() {
            return;
        }
        let mut state = self.active_threads.write().await;
        if let Some(activation) = state.activations.get(thread_id).copied() {
            let uniquely_pending = state
                .awaiting_submissions
                .get(thread_id)
                .is_some_and(|pending| pending.len() == 1 && pending.contains(&activation));
            if uniquely_pending {
                state
                    .turn_epochs
                    .entry(thread_id.into())
                    .or_default()
                    .entry(turn_id.into())
                    .or_insert(activation);
            }
        }
    }

    /// Bind the authoritative SubmitTurn response, which may steer an existing turn id into
    /// the current activation epoch.
    pub async fn bind_submitted_turn_if_current(
        &self,
        thread_id: &str,
        activation: u64,
        turn_id: &str,
    ) -> Vec<ChatStreamEvent> {
        if turn_id.is_empty() {
            return Vec::new();
        }
        let mut state = self.active_threads.write().await;
        let is_current = state.activations.get(thread_id).copied() == Some(activation);
        let was_pending = state
            .awaiting_submissions
            .get(thread_id)
            .is_some_and(|pending| pending.contains(&activation));
        Self::remove_pending_submission(&mut state, thread_id, activation);
        if !was_pending {
            Self::remove_deferred_activation(&mut state, thread_id, activation);
            return Vec::new();
        }
        if !is_current {
            if state.threads.contains(thread_id) {
                state
                    .turn_epochs
                    .entry(thread_id.into())
                    .or_default()
                    .entry(turn_id.into())
                    .or_insert(activation);
            }
            Self::remove_deferred_activation(&mut state, thread_id, activation);
            return Vec::new();
        }
        state
            .turn_epochs
            .entry(thread_id.into())
            .or_default()
            .insert(turn_id.into(), activation);

        let deferred = state
            .deferred_terminals
            .remove(thread_id)
            .and_then(|mut terminals| terminals.remove(turn_id))
            .filter(|terminal| terminal.awaiting_activation == activation);
        let Some(deferred) = deferred else {
            return Vec::new();
        };
        if deferred.background_turn {
            state
                .background_pending
                .entry(thread_id.into())
                .or_default()
                .insert(turn_id.into());
        }
        Self::clear_thread(&mut state, thread_id);
        drop(state);
        self.queue_terminal_unsubscribe(thread_id, activation);
        deferred.events
    }

    async fn active_activations(&self) -> Vec<(String, u64)> {
        let state = self.active_threads.read().await;
        state
            .threads
            .iter()
            .filter_map(|thread_id| {
                state
                    .activations
                    .get(thread_id)
                    .copied()
                    .map(|activation| (thread_id.clone(), activation))
            })
            .collect()
    }

    async fn background_resume_threads(&self) -> Vec<String> {
        let mut threads = self
            .active_threads
            .read()
            .await
            .background_pending
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        threads.sort();
        threads
    }

    async fn background_resume_targets(&self) -> Vec<(String, u64)> {
        let threads = self.background_resume_threads().await;
        let mut targets = Vec::new();
        for thread_id in threads {
            let gate = self.terminal_cleanup.gate(&thread_id);
            let _owner_guard = gate.lock().await;
            let mut state = self.active_threads.write().await;
            if state.threads.contains(&thread_id)
                || !state.background_pending.contains_key(&thread_id)
            {
                continue;
            }
            let token = match state.background_subscriptions.get(&thread_id).copied() {
                Some(token) => token,
                None => {
                    state.next_activation = state.next_activation.wrapping_add(1).max(1);
                    let token = state.next_activation;
                    state
                        .background_subscriptions
                        .insert(thread_id.clone(), token);
                    token
                }
            };
            drop(state);
            self.terminal_cleanup
                .owners
                .write()
                .await
                .insert(thread_id.clone(), token);
            targets.push((thread_id, token));
        }
        targets
    }

    async fn complete_background_turn(&self, thread_id: &str, turn_id: &str) -> bool {
        let mut state = self.active_threads.write().await;
        let removed = state
            .background_pending
            .get_mut(thread_id)
            .is_some_and(|turns| turns.remove(turn_id));
        if !removed {
            return false;
        }
        let empty = state
            .background_pending
            .get(thread_id)
            .is_some_and(HashSet::is_empty);
        if !empty {
            return true;
        }
        state.background_pending.remove(thread_id);
        let subscription = state.background_subscriptions.remove(thread_id);
        let active = state.threads.contains(thread_id);
        drop(state);
        if !active {
            if let Some(subscription) = subscription {
                self.queue_terminal_unsubscribe(thread_id, subscription);
            }
        }
        true
    }

    async fn record_delivered_projection(
        &self,
        thread_id: &str,
        turn_id: &str,
        events: &[ChatStreamEvent],
    ) {
        if turn_id.is_empty() {
            return;
        }
        let mut state = self.active_threads.write().await;
        if !state.threads.contains(thread_id) {
            return;
        }
        for event in events {
            match event {
                ChatStreamEvent::Token { content } => state
                    .delivered_agent_text
                    .entry(thread_id.into())
                    .or_default()
                    .entry(turn_id.into())
                    .or_default()
                    .push_str(content),
                ChatStreamEvent::Reasoning { content } => state
                    .delivered_reasoning
                    .entry(thread_id.into())
                    .or_default()
                    .entry(turn_id.into())
                    .or_default()
                    .push_str(content),
                ChatStreamEvent::Error { message } => {
                    state
                        .pending_terminal_errors
                        .entry(thread_id.into())
                        .or_default()
                        .insert(turn_id.into(), message.clone());
                }
                _ => {}
            }
        }
    }

    /// Snapshot agent messages are full text while live messages are deltas. Emit only the
    /// suffix that the current UI listener has not already received.
    async fn recover_snapshot_projection(
        &self,
        thread_id: &str,
        turn_id: &str,
        events: Vec<ChatStreamEvent>,
    ) -> Vec<ChatStreamEvent> {
        let mut state = self.active_threads.write().await;
        let mut recovered = Vec::with_capacity(events.len());
        for event in events {
            match event {
                ChatStreamEvent::Token { content } => {
                    let delivered = state
                        .delivered_agent_text
                        .entry(thread_id.into())
                        .or_default()
                        .entry(turn_id.into())
                        .or_default();
                    if content.starts_with(delivered.as_str()) {
                        let missing = content[delivered.len()..].to_string();
                        *delivered = content;
                        if !missing.is_empty() {
                            recovered.push(ChatStreamEvent::Token { content: missing });
                        }
                    } else {
                        *delivered = content.clone();
                        recovered.push(ChatStreamEvent::TextReconcile { content });
                    }
                }
                ChatStreamEvent::Reasoning { content } => {
                    let delivered = state
                        .delivered_reasoning
                        .entry(thread_id.into())
                        .or_default()
                        .entry(turn_id.into())
                        .or_default();
                    if content.starts_with(delivered.as_str()) {
                        let missing = content[delivered.len()..].to_string();
                        *delivered = content;
                        if !missing.is_empty() {
                            recovered.push(ChatStreamEvent::Reasoning { content: missing });
                        }
                    } else {
                        *delivered = content.clone();
                        recovered.push(ChatStreamEvent::ReasoningReconcile { content });
                    }
                }
                other => recovered.push(other),
            }
        }
        recovered
    }

    async fn accept_extension(&self, thread_id: &str, item_id: &str, payload_json: &str) -> bool {
        let key = (thread_id.into(), item_id.into());
        let payload_json = payload_json.to_string();
        let previous = self
            .active_threads
            .write()
            .await
            .delivered_extensions
            .insert(key, payload_json.clone());
        previous.as_deref() != Some(payload_json.as_str())
    }

    async fn dedup_terminal_projection(
        &self,
        thread_id: &str,
        turn_id: &str,
        events: Vec<ChatStreamEvent>,
    ) -> Vec<ChatStreamEvent> {
        let mut state = self.active_threads.write().await;
        let pending = state
            .pending_terminal_errors
            .get_mut(thread_id)
            .and_then(|errors| errors.remove(turn_id));
        if state
            .pending_terminal_errors
            .get(thread_id)
            .is_some_and(HashMap::is_empty)
        {
            state.pending_terminal_errors.remove(thread_id);
        }
        let mut suppressed = false;
        events
            .into_iter()
            .filter(|event| {
                if suppressed {
                    return true;
                }
                let ChatStreamEvent::Error { message } = event else {
                    return true;
                };
                if pending.as_deref() == Some(message.as_str()) {
                    suppressed = true;
                    return false;
                }
                true
            })
            .collect()
    }

    #[cfg(test)]
    async fn is_active(&self, thread_id: &str) -> bool {
        self.active_threads.read().await.threads.contains(thread_id)
    }

    /// Accept each terminal once, or defer it while a newer SubmitTurn ack can still steer the
    /// same turn id into the current activation.
    #[cfg(test)]
    async fn accept_terminal(
        &self,
        thread_id: &str,
        turn_id: &str,
        events: Vec<ChatStreamEvent>,
    ) -> Vec<ChatStreamEvent> {
        self.accept_terminal_with_background(thread_id, turn_id, events, false)
            .await
    }

    async fn accept_terminal_with_background(
        &self,
        thread_id: &str,
        turn_id: &str,
        events: Vec<ChatStreamEvent>,
        background_turn: bool,
    ) -> Vec<ChatStreamEvent> {
        let mut state = self.active_threads.write().await;
        if state
            .deferred_terminals
            .get(thread_id)
            .is_some_and(|terminals| terminals.contains_key(turn_id))
        {
            return Vec::new();
        }
        let Some(turn_epoch) = state
            .turn_epochs
            .get(thread_id)
            .and_then(|turns| turns.get(turn_id))
            .copied()
        else {
            return Vec::new();
        };
        let current_activation = state.activations.get(thread_id).copied();
        if current_activation == Some(turn_epoch) {
            if background_turn {
                state
                    .background_pending
                    .entry(thread_id.into())
                    .or_default()
                    .insert(turn_id.into());
            }
            Self::clear_thread(&mut state, thread_id);
            drop(state);
            self.queue_terminal_unsubscribe(thread_id, turn_epoch);
            return events;
        }

        if let Some(current_activation) = current_activation.filter(|activation| {
            state
                .awaiting_submissions
                .get(thread_id)
                .is_some_and(|pending| pending.contains(activation))
        }) {
            Self::remove_turn(&mut state, thread_id, turn_id);
            state
                .deferred_terminals
                .entry(thread_id.into())
                .or_default()
                .insert(
                    turn_id.into(),
                    DeferredTerminal {
                        awaiting_activation: current_activation,
                        background_turn,
                        events,
                    },
                );
            return Vec::new();
        }

        Self::remove_turn(&mut state, thread_id, turn_id);
        Vec::new()
    }

    fn remove_turn(state: &mut ActiveState, thread_id: &str, turn_id: &str) {
        if let Some(turns) = state.turn_epochs.get_mut(thread_id) {
            turns.remove(turn_id);
        }
        if state
            .turn_epochs
            .get(thread_id)
            .is_some_and(HashMap::is_empty)
        {
            state.turn_epochs.remove(thread_id);
        }
    }

    fn clear_thread(state: &mut ActiveState, thread_id: &str) {
        state.threads.remove(thread_id);
        state.turn_epochs.remove(thread_id);
        state.activations.remove(thread_id);
        state.awaiting_submissions.remove(thread_id);
        state.deferred_terminals.remove(thread_id);
        state.delivered_agent_text.remove(thread_id);
        state.delivered_reasoning.remove(thread_id);
        state.pending_terminal_errors.remove(thread_id);
    }

    fn remove_turn_epoch(state: &mut ActiveState, thread_id: &str, activation: u64) {
        if let Some(turns) = state.turn_epochs.get_mut(thread_id) {
            turns.retain(|_, epoch| *epoch != activation);
            if turns.is_empty() {
                state.turn_epochs.remove(thread_id);
            }
        }
    }

    fn remove_deferred_activation(state: &mut ActiveState, thread_id: &str, activation: u64) {
        Self::remove_pending_submission(state, thread_id, activation);
        if let Some(terminals) = state.deferred_terminals.get_mut(thread_id) {
            terminals.retain(|_, terminal| terminal.awaiting_activation != activation);
            if terminals.is_empty() {
                state.deferred_terminals.remove(thread_id);
            }
        }
    }

    fn remove_pending_submission(state: &mut ActiveState, thread_id: &str, activation: u64) {
        if let Some(pending) = state.awaiting_submissions.get_mut(thread_id) {
            pending.remove(&activation);
            if pending.is_empty() {
                state.awaiting_submissions.remove(thread_id);
            }
        }
    }

    fn queue_terminal_unsubscribe(&self, thread_id: &str, activation: u64) {
        let _ = self
            .terminal_cleanup
            .requests
            .send(TerminalUnsubscribeRequest {
                thread_id: thread_id.into(),
                activation,
            });
    }

    fn take_terminal_unsubscribe_requests(
        &self,
    ) -> mpsc::UnboundedReceiver<TerminalUnsubscribeRequest> {
        self.terminal_cleanup.take_requests()
    }

    async fn clear_terminal_owner_if(&self, thread_id: &str, activation: u64) {
        let mut owners = self.terminal_cleanup.owners.write().await;
        if owners.get(thread_id).copied() == Some(activation) {
            owners.remove(thread_id);
        }
    }

    async fn run_terminal_unsubscribe_if_owned<F, Fut>(
        &self,
        request: TerminalUnsubscribeRequest,
        unsubscribe: F,
    ) -> Result<bool, String>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<(), String>>,
    {
        let gate = self.terminal_cleanup.gate(&request.thread_id);
        let _owner_guard = gate.lock().await;
        if self
            .terminal_cleanup
            .owners
            .read()
            .await
            .get(&request.thread_id)
            .copied()
            != Some(request.activation)
        {
            return Ok(false);
        }
        unsubscribe(request.thread_id.clone()).await?;
        self.clear_terminal_owner_if(&request.thread_id, request.activation)
            .await;
        Ok(true)
    }

    /// Linearize recovery ResumeThread with activation-owned terminal cleanup.
    ///
    /// The gate remains held across the RPC: if cleanup wins first, the owner/current checks
    /// skip the stale resume; if resume wins first, cleanup waits and unsubscribes afterwards.
    async fn run_resume_if_owned<F, Fut, T>(
        &self,
        thread_id: String,
        activation: u64,
        resume: F,
    ) -> Result<Option<T>, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        let gate = self.terminal_cleanup.gate(&thread_id);
        let _owner_guard = gate.lock().await;
        let is_current = self
            .active_threads
            .read()
            .await
            .activations
            .get(&thread_id)
            .copied()
            == Some(activation);
        let is_owner = self
            .terminal_cleanup
            .owners
            .read()
            .await
            .get(&thread_id)
            .copied()
            == Some(activation);
        if !is_current || !is_owner {
            return Ok(None);
        }
        resume().await.map(Some)
    }

    async fn run_background_resume_if_owned<F, Fut, T>(
        &self,
        thread_id: String,
        subscription: u64,
        resume: F,
    ) -> Result<Option<T>, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        let gate = self.terminal_cleanup.gate(&thread_id);
        let _owner_guard = gate.lock().await;
        let state = self.active_threads.read().await;
        let is_pending = state.background_pending.contains_key(&thread_id);
        let is_current =
            state.background_subscriptions.get(&thread_id).copied() == Some(subscription);
        let is_active = state.threads.contains(&thread_id);
        drop(state);
        let is_owner = self
            .terminal_cleanup
            .owners
            .read()
            .await
            .get(&thread_id)
            .copied()
            == Some(subscription);
        if !is_pending || !is_current || is_active || !is_owner {
            return Ok(None);
        }
        resume().await.map(Some)
    }
}

#[derive(Debug)]
struct RetryBackoff {
    next_ms: u64,
}

impl Default for RetryBackoff {
    fn default() -> Self {
        Self { next_ms: 500 }
    }
}

impl RetryBackoff {
    fn next_delay(&mut self) -> Duration {
        let delay = Duration::from_millis(self.next_ms);
        self.next_ms = self.next_ms.saturating_mul(2).min(15_000);
        delay
    }

    fn reset(&mut self) {
        self.next_ms = 500;
    }

    fn after_attempt(&mut self, saw_live_event: bool) -> Duration {
        if saw_live_event {
            self.reset();
        }
        self.next_delay()
    }
}

pub(crate) fn submission_failure_events(
    is_current_activation: bool,
    message: impl Into<String>,
) -> Vec<ChatStreamEvent> {
    if !is_current_activation {
        return Vec::new();
    }
    vec![
        ChatStreamEvent::Error {
            message: message.into(),
        },
        ChatStreamEvent::RunFinished {
            run_id: String::new(),
            outcome_type: "error".into(),
            interrupts_json: "[]".into(),
        },
        ChatStreamEvent::Done,
    ]
}

pub fn accepted_turn_id(response: proto::SubmitTurnResponse) -> Result<String, String> {
    if response.disposition == "not_submitted" {
        Err(if response.reason.trim().is_empty() {
            "turn was not submitted".into()
        } else {
            response.reason
        })
    } else {
        Ok(response.turn_id)
    }
}

/// Register the shared bridge and start its reconnecting connection loop.
pub fn start_bridge(app: &AppHandle) {
    let bridge = Arc::new(ThreadEventsBridge::new());
    let terminal_unsubscribes = bridge.take_terminal_unsubscribe_requests();
    app.manage(Arc::clone(&bridge));
    let subscribe_app = app.clone();
    let subscribe_bridge = Arc::clone(&bridge);
    tauri::async_runtime::spawn(async move {
        run_subscribe_loop(subscribe_app, subscribe_bridge).await;
    });
    tauri::async_runtime::spawn(async move {
        run_terminal_unsubscribe_loop(bridge, terminal_unsubscribes).await;
    });
}

async fn run_terminal_unsubscribe_loop(
    bridge: Arc<ThreadEventsBridge>,
    mut requests: mpsc::UnboundedReceiver<TerminalUnsubscribeRequest>,
) {
    while let Some(request) = requests.recv().await {
        let bridge = Arc::clone(&bridge);
        tauri::async_runtime::spawn(async move {
            let mut backoff = RetryBackoff::default();
            loop {
                let connection_id = bridge.connection_id().to_string();
                let result = bridge
                    .run_terminal_unsubscribe_if_owned(request.clone(), move |thread_id| {
                        unsubscribe_terminal_thread(connection_id, thread_id)
                    })
                    .await;
                match result {
                    Ok(_) => break,
                    Err(error) => {
                        debug!(
                            thread_id = %request.thread_id,
                            activation = request.activation,
                            %error,
                            "terminal thread unsubscribe failed"
                        );
                        tokio::time::sleep(backoff.next_delay()).await;
                    }
                }
            }
        });
    }
}

async fn unsubscribe_terminal_thread(
    connection_id: String,
    thread_id: String,
) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(5), async move {
        let endpoint = endpoint_url(&default_grpc_address());
        let mut client = AstroServiceClient::connect(endpoint)
            .await
            .map_err(|error| error.to_string())?;
        match client
            .unsubscribe_thread(proto::UnsubscribeThreadRequest {
                connection_id,
                thread_id,
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(error)
                if matches!(
                    error.code(),
                    tonic::Code::NotFound | tonic::Code::FailedPrecondition
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.to_string()),
        }
    })
    .await
    .map_err(|_| "terminal thread unsubscribe timed out".to_string())?
}

async fn run_subscribe_loop(app: AppHandle, bridge: Arc<ThreadEventsBridge>) {
    let mut backoff = RetryBackoff::default();
    loop {
        bridge.mark_recovering();
        let attempt = subscribe_once(&app, &bridge).await;
        debug!(error = %attempt.error, saw_live_event = attempt.saw_live_event, "thread event stream ended");
        bridge.mark_recovering();
        tokio::time::sleep(backoff.after_attempt(attempt.saw_live_event)).await;
    }
}

struct ConnectionAttempt {
    saw_live_event: bool,
    error: String,
}

async fn subscribe_once(app: &AppHandle, bridge: &ThreadEventsBridge) -> ConnectionAttempt {
    let mut saw_live_event = false;
    let result = subscribe_connection(app, bridge, &mut saw_live_event).await;
    ConnectionAttempt {
        saw_live_event,
        error: result
            .err()
            .unwrap_or_else(|| "thread event stream closed".into()),
    }
}

async fn subscribe_connection(
    app: &AppHandle,
    bridge: &ThreadEventsBridge,
    saw_live_event: &mut bool,
) -> Result<(), String> {
    let endpoint = endpoint_url(&default_grpc_address());
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|error| error.to_string())?;
    let mut stream = client
        .subscribe_thread_events(proto::SubscribeThreadEventsRequest {
            connection_id: bridge.connection_id().into(),
        })
        .await
        .map_err(|error| error.to_string())?
        .into_inner();

    // Reading begins before Resume RPCs. This prevents the server's bounded transport from
    // classifying a reconnecting desktop as a slow consumer while snapshots are rebuilt.
    let (live_tx, mut live_rx) = tokio::sync::mpsc::channel(128);
    let (boundary_tx, mut boundary_rx) = tokio::sync::mpsc::channel(1);
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                biased;
                message = stream.message() => {
                    match message {
                        Ok(Some(event)) => {
                            if live_tx.send(RecoveryIngress::Event(Ok(event))).await.is_err() {
                                return;
                            }
                        }
                        Ok(None) => {
                            let _ = live_tx
                                .send(RecoveryIngress::Event(Err("thread event stream closed".into())))
                                .await;
                            return;
                        }
                        Err(error) => {
                            let _ = live_tx
                                .send(RecoveryIngress::Event(Err(error.to_string())))
                                .await;
                            return;
                        }
                    }
                }
                boundary = boundary_rx.recv() => {
                    if boundary.is_none() || live_tx.send(RecoveryIngress::Boundary).await.is_err() {
                        return;
                    }
                }
            }
        }
    });

    // Workspace-wide pending changes have no active chat owner. Subscribe every accepted
    // desktop connection before recovering chat Threads so those durable extensions are live.
    let mut snapshots = Vec::new();
    let workspace = client
        .resume_thread(proto::ResumeThreadRequest {
            connection_id: bridge.connection_id().into(),
            thread_id: WORKSPACE_EVENT_THREAD_ID.into(),
            include_turns: true,
        })
        .await
        .map_err(|error| format!("failed to resume workspace event thread: {error}"))?
        .into_inner();
    if let Some(snapshot) = workspace.thread {
        snapshots.push(snapshot);
    }

    // Generation barrier: every durable snapshot is emitted before any buffered event from
    // this accepted stream. Public readiness remains false until recovery completes, so a new
    // SubmitTurn cannot race an old terminal snapshot.
    for (thread_id, activation) in bridge.active_activations().await {
        let connection_id = bridge.connection_id().to_string();
        match bridge
            .run_resume_if_owned(thread_id.clone(), activation, || async {
                client
                    .resume_thread(proto::ResumeThreadRequest {
                        connection_id,
                        thread_id: thread_id.clone(),
                        include_turns: true,
                    })
                    .await
                    .map(|response| response.into_inner())
                    .map_err(|error| error.to_string())
            })
            .await
        {
            Ok(Some(response)) => {
                if let Some(snapshot) = response.thread {
                    snapshots.push(snapshot);
                }
            }
            Ok(None) => {}
            Err(error) => {
                return bridge.complete_recovery(Err(format!(
                    "failed to resume active thread {thread_id}: {error}"
                )));
            }
        }
    }
    for (thread_id, subscription) in bridge.background_resume_targets().await {
        let connection_id = bridge.connection_id().to_string();
        match bridge
            .run_background_resume_if_owned(thread_id.clone(), subscription, || async {
                client
                    .resume_thread(proto::ResumeThreadRequest {
                        connection_id,
                        thread_id: thread_id.clone(),
                        include_turns: true,
                    })
                    .await
                    .map(|response| response.into_inner())
                    .map_err(|error| error.to_string())
            })
            .await
        {
            Ok(Some(response)) => {
                if let Some(snapshot) = response.thread {
                    snapshots.push(snapshot);
                }
            }
            Ok(None) => {}
            Err(error) => {
                return bridge.complete_recovery(Err(format!(
                    "failed to resume background thread {thread_id}: {error}"
                )));
            }
        }
    }

    boundary_tx
        .send(())
        .await
        .map_err(|_| "thread event reader stopped before recovery boundary".to_string())?;
    let buffered = receive_recovery_batch(&mut live_rx).await?;
    for delivery in reconnect_delivery_order(snapshots, buffered) {
        match delivery {
            ReconnectDelivery::Snapshot(snapshot) => {
                let thread_id = snapshot.thread_id.clone();
                emit_snapshot(app, &snapshot);
                let recovered_extensions = recover_snapshot_extensions(bridge, &snapshot).await;
                if thread_id == WORKSPACE_EVENT_THREAD_ID {
                    for recovered in recovered_extensions {
                        let extension = recovered.extension;
                        if let Some(session_event) =
                            extension_to_session_event(&thread_id, extension.clone())
                        {
                            emit_session_event(app, session_event);
                        }
                        emit_chat_events(app, &thread_id, map_extension_to_chat(extension));
                    }
                    continue;
                }
                let reconciled = reconcile_snapshot(&snapshot);
                if let Some(turn_id) = reconciled.active_turn_id.as_deref() {
                    bridge.bind_observed_turn(&thread_id, turn_id).await;
                }
                let projection_turn_id = reconciled
                    .active_turn_id
                    .as_deref()
                    .or(reconciled.terminal_turn_id.as_deref())
                    .unwrap_or_default();
                let recovered = bridge
                    .recover_snapshot_projection(
                        &thread_id,
                        projection_turn_id,
                        reconciled.terminal,
                    )
                    .await;
                if reconciled.keep_active {
                    emit_chat_events(app, &thread_id, recovered);
                } else {
                    let recovered = bridge
                        .dedup_terminal_projection(&thread_id, projection_turn_id, recovered)
                        .await;
                    let terminal = bridge
                        .accept_terminal_with_background(
                            &thread_id,
                            projection_turn_id,
                            recovered,
                            snapshot.turns.iter().any(|turn| {
                                turn.id == projection_turn_id && turn.status == "completed"
                            }),
                        )
                        .await;
                    emit_chat_events(app, &thread_id, terminal);
                }
                for recovered in recovered_extensions {
                    if recovered.extension.namespace == "astro.background_complete" {
                        bridge
                            .complete_background_turn(&thread_id, &recovered.turn_id)
                            .await;
                    }
                    let extension = recovered.extension;
                    if let Some(session_event) =
                        extension_to_session_event(&thread_id, extension.clone())
                    {
                        emit_session_event(app, session_event);
                    }
                    emit_chat_events(app, &thread_id, map_extension_to_chat(extension));
                }
            }
            ReconnectDelivery::Live(event) => {
                let event = event?;
                *saw_live_event = true;
                process_live_event(app, bridge, event).await;
            }
        }
    }
    bridge.complete_recovery(Ok(()))?;

    while let Some(ingress) = live_rx.recv().await {
        let RecoveryIngress::Event(event) = ingress else {
            continue;
        };
        let event = event?;
        *saw_live_event = true;
        process_live_event(app, bridge, event).await;
    }
    Err("thread event reader stopped".into())
}

async fn process_live_event(
    app: &AppHandle,
    bridge: &ThreadEventsBridge,
    event: proto::ThreadEvent,
) {
    let thread_id = event.thread_id.clone();
    let turn_id = event.turn_id.clone();
    if let Some(proto::thread_event::Payload::TurnStarted(started)) = event.payload.as_ref() {
        bridge
            .bind_observed_turn(&thread_id, &started.turn_id)
            .await;
    }
    if let Some(proto::thread_event::Payload::Extension(extension)) = event.payload.as_ref() {
        if !bridge
            .accept_extension(&thread_id, &extension.item_id, &extension.payload_json)
            .await
        {
            return;
        }
        if let Some(session_event) = extension_to_session_event(&thread_id, extension.clone()) {
            emit_session_event(app, session_event);
        }
        if extension.namespace == "astro.background_complete" {
            bridge.complete_background_turn(&thread_id, &turn_id).await;
        }
    }
    let background_turn = matches!(
        event.payload.as_ref(),
        Some(proto::thread_event::Payload::TurnComplete(complete)) if !complete.has_error
    );
    let terminal = matches!(
        event.payload,
        Some(proto::thread_event::Payload::TurnComplete(_))
            | Some(proto::thread_event::Payload::TurnAborted(_))
    );
    let events = map_thread_event(event);
    let events = if terminal {
        let events = bridge
            .dedup_terminal_projection(&thread_id, &turn_id, events)
            .await;
        bridge
            .accept_terminal_with_background(&thread_id, &turn_id, events, background_turn)
            .await
    } else {
        bridge
            .record_delivered_projection(&thread_id, &turn_id, &events)
            .await;
        events
    };
    emit_chat_events(app, &thread_id, events);
}

enum ReconnectDelivery {
    Snapshot(proto::ThreadSnapshot),
    Live(Result<proto::ThreadEvent, String>),
}

enum RecoveryIngress {
    Event(Result<proto::ThreadEvent, String>),
    Boundary,
}

async fn receive_recovery_batch(
    live_rx: &mut tokio::sync::mpsc::Receiver<RecoveryIngress>,
) -> Result<Vec<Result<proto::ThreadEvent, String>>, String> {
    let mut buffered = Vec::new();
    loop {
        match live_rx.recv().await {
            Some(RecoveryIngress::Event(Ok(event))) => buffered.push(Ok(event)),
            Some(RecoveryIngress::Event(Err(error))) => return Err(error),
            Some(RecoveryIngress::Boundary) => return Ok(buffered),
            None => return Err("thread event reader stopped before recovery boundary".into()),
        }
    }
}

fn reconnect_delivery_order(
    snapshots: Vec<proto::ThreadSnapshot>,
    buffered_live: Vec<Result<proto::ThreadEvent, String>>,
) -> Vec<ReconnectDelivery> {
    snapshots
        .into_iter()
        .map(ReconnectDelivery::Snapshot)
        .chain(buffered_live.into_iter().map(ReconnectDelivery::Live))
        .collect()
}

pub(crate) fn emit_chat_events(app: &AppHandle, thread_id: &str, events: Vec<ChatStreamEvent>) {
    let event_name = format!("chat_stream_{thread_id}");
    for event in events {
        let is_done = matches!(event, ChatStreamEvent::Done);
        let _ = app.emit(&event_name, event);
        if is_done {
            crate::commands::evolution_run::spawn_maybe_auto_evolution(app.clone());
            crate::commands::evolution_run::spawn_maybe_curator(app.clone());
        }
    }
}

fn map_thread_event(event: proto::ThreadEvent) -> Vec<ChatStreamEvent> {
    use proto::thread_event::Payload;
    let thread_id = event.thread_id;
    let turn_id = event.turn_id;
    match event.payload {
        Some(Payload::TurnStarted(started)) => vec![ChatStreamEvent::RunStarted {
            thread_id,
            run_id: started.turn_id,
        }],
        Some(Payload::ItemStarted(item)) => map_item_event(item, true),
        Some(Payload::ItemCompleted(item)) => map_item_event(item, false),
        Some(Payload::AgentMessageDelta(delta)) => {
            vec![ChatStreamEvent::Token { content: delta.delta }]
        }
        Some(Payload::ReasoningDelta(delta)) => {
            vec![ChatStreamEvent::Reasoning { content: delta.delta }]
        }
        Some(Payload::PlanDelta(delta)) => vec![activity(delta.item_id, "plan_delta", delta.delta)],
        Some(Payload::ExecOutputDelta(delta)) => {
            vec![activity(delta.item_id, "exec_output_delta", delta.delta)]
        }
        Some(Payload::PatchDelta(delta)) => {
            vec![activity(delta.item_id, "patch_delta", delta.delta)]
        }
        Some(Payload::ControlRequest(control)) => map_control_request(turn_id, control),
        Some(Payload::TokenCount(tokens)) => vec![ChatStreamEvent::Usage {
            prompt_tokens: tokens.input_tokens.min(u32::MAX.into()) as u32,
            completion_tokens: tokens.output_tokens.min(u32::MAX.into()) as u32,
            total_tokens: tokens.total_tokens.min(u32::MAX.into()) as u32,
        }],
        Some(Payload::Error(error)) => vec![ChatStreamEvent::Error {
            message: error.message,
        }],
        Some(Payload::Warning(warning)) => vec![activity(
            turn_id,
            "warning",
            serde_json::json!({"message":warning.message,"error_type":warning.error_type})
                .to_string(),
        )],
        Some(Payload::TurnComplete(complete)) => {
            let mut events = Vec::new();
            if complete.has_error {
                if let Some(error) = complete.error {
                    events.push(ChatStreamEvent::Error {
                        message: error.message,
                    });
                }
            }
            events.extend(terminal_events(
                turn_id,
                if complete.has_error { "error" } else { "success" },
                "[]".into(),
            ));
            events
        }
        Some(Payload::TurnAborted(aborted)) => terminal_events(
            turn_id,
            "interrupt",
            serde_json::json!([{"id":"","reason":aborted.reason,"message":"","tool_call_id":"","response_schema_json":"","expires_at":"","metadata_json":""}]).to_string(),
        ),
        Some(Payload::Extension(extension)) => map_extension_to_chat(extension),
        Some(Payload::ShutdownComplete(_)) => vec![activity(
            turn_id,
            "shutdown_complete",
            serde_json::json!({"shutdown_complete":true}).to_string(),
        )],
        None => vec![ChatStreamEvent::Error {
            message: "thread event has no payload".into(),
        }],
    }
}

fn terminal_events(
    run_id: String,
    outcome_type: &str,
    interrupts_json: String,
) -> Vec<ChatStreamEvent> {
    vec![
        ChatStreamEvent::RunFinished {
            run_id,
            outcome_type: outcome_type.into(),
            interrupts_json,
        },
        ChatStreamEvent::Done,
    ]
}

fn activity(
    message_id: impl Into<String>,
    activity_type: &str,
    content_json: impl Into<String>,
) -> ChatStreamEvent {
    ChatStreamEvent::Activity {
        message_id: message_id.into(),
        activity_type: activity_type.into(),
        content_json: content_json.into(),
        replace: false,
    }
}

fn map_control_request(
    run_id: String,
    control: proto::ThreadControlRequest,
) -> Vec<ChatStreamEvent> {
    let payload = serde_json::from_str::<serde_json::Value>(&control.payload_json)
        .unwrap_or(serde_json::Value::Null);
    if control.kind == "dynamic_tool_call" {
        return vec![ChatStreamEvent::ToolCallDelta {
            index: payload
                .get("index")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default()
                .min(u32::MAX.into()) as u32,
            id: control.item_id,
            name: payload
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .into(),
            arguments: payload
                .get("delta")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .into(),
        }];
    }
    if control.kind == "dynamic_tool_response" {
        return Vec::new();
    }
    if !matches!(
        control.kind.as_str(),
        "exec_approval"
            | "apply_patch_approval"
            | "request_permissions"
            | "request_user_input"
            | "elicitation"
    ) {
        return vec![activity(
            control.item_id,
            &control.kind,
            control.payload_json,
        )];
    }
    vec![ChatStreamEvent::RunFinished {
        run_id,
        outcome_type: "hitl_waiting".into(),
        interrupts_json: serde_json::json!([{
            "id": control.request_id,
            "reason": payload.get("reason").and_then(serde_json::Value::as_str).unwrap_or(&control.kind),
            "message": payload.get("message").and_then(serde_json::Value::as_str).unwrap_or_default(),
            "tool_call_id": control.item_id,
            "response_schema_json": payload.get("response_schema").cloned().unwrap_or_default().to_string(),
            "expires_at": payload.get("expires_at").and_then(serde_json::Value::as_str).unwrap_or_default(),
            "metadata_json": serde_json::json!({"kind":control.kind,"operations":payload.get("operations").cloned().unwrap_or_default(),"payload":payload}).to_string(),
        }])
        .to_string(),
    }]
}

fn map_item_event(item_event: proto::ThreadItemEvent, started: bool) -> Vec<ChatStreamEvent> {
    let Some(item) = item_event.item else {
        return vec![ChatStreamEvent::Error {
            message: "thread item event has no item".into(),
        }];
    };
    match serde_json::from_str::<TurnItem>(&item.payload_json) {
        Ok(TurnItem::CommandExecution(tool))
        | Ok(TurnItem::DynamicToolCall(tool))
        | Ok(TurnItem::McpToolCall(tool))
        | Ok(TurnItem::CollabAgentToolCall(tool))
        | Ok(TurnItem::WebSearch(tool))
        | Ok(TurnItem::ImageView(tool))
        | Ok(TurnItem::ImageGeneration(tool))
        | Ok(TurnItem::FileChange(tool)) => vec![ChatStreamEvent::ToolCall {
            id: tool.id,
            name: tool.name,
            arguments_json: tool.arguments.to_string(),
            result: tool
                .output
                .map(|value| match value {
                    serde_json::Value::String(text) => text,
                    other => other.to_string(),
                })
                .unwrap_or_default(),
            phase: if started { "started" } else { "completed" }.into(),
            media: tool.media.into_iter().map(media_asset_dto).collect(),
        }],
        Ok(TurnItem::AgentMessage(text)) if started => {
            vec![ChatStreamEvent::Token {
                content: text.content,
            }]
        }
        Ok(TurnItem::Reasoning(text)) if started => {
            vec![ChatStreamEvent::Reasoning {
                content: text.content,
            }]
        }
        Ok(TurnItem::AgentMessage(_)) | Ok(TurnItem::Reasoning(_)) => Vec::new(),
        Ok(TurnItem::HookPrompt(text)) => vec![ChatStreamEvent::Hook {
            name: "hook_prompt".into(),
            detail: text.content,
            outcome: if started { "started" } else { "completed" }.into(),
        }],
        Ok(TurnItem::Extension(extension)) if extension.namespace == "astro.memory" => {
            vec![memory_update_from_value(&extension.payload)]
        }
        Ok(_) => vec![activity(item.id, &item.item_type, item.payload_json)],
        Err(error) => vec![ChatStreamEvent::Error {
            message: format!("invalid thread item: {error}"),
        }],
    }
}

fn media_asset_dto(asset: types::MediaAsset) -> MediaAssetDto {
    let (ref_kind, ref_value) = match asset.reference {
        types::MediaRef::WorkspacePath(path) => ("workspace_path", path),
        types::MediaRef::DataUrl(url) => ("data_url", url),
        types::MediaRef::RemoteUri(uri) => ("remote_uri", uri),
    };
    let kind = match asset.kind {
        types::MediaKind::Image => "image",
        types::MediaKind::Audio => "audio",
        types::MediaKind::Video => "video",
        types::MediaKind::File => "file",
    };
    MediaAssetDto {
        kind: kind.into(),
        mime_type: asset.mime_type,
        ref_kind: ref_kind.into(),
        ref_value,
        label: asset.label,
        id: asset.id,
    }
}

fn map_extension_to_chat(extension: proto::ThreadExtension) -> Vec<ChatStreamEvent> {
    match extension.namespace.as_str() {
        "astro.thread_settings"
        | "astro.thread_rollback"
        | "astro.background_complete"
        | "astro.pending"
        | "astro.session_metadata" => Vec::new(),
        "astro.context_usage" => vec![context_usage_event(&extension.payload_json)],
        "astro.memory" => {
            match serde_json::from_str::<serde_json::Value>(&extension.payload_json) {
                Ok(payload) => vec![memory_update_from_value(&payload)],
                Err(error) => vec![ChatStreamEvent::Error {
                    message: format!("invalid memory update payload: {error}"),
                }],
            }
        }
        _ => vec![activity(
            extension.item_id,
            &extension.namespace,
            extension.payload_json,
        )],
    }
}

fn memory_update_from_value(payload: &serde_json::Value) -> ChatStreamEvent {
    ChatStreamEvent::MemoryUpdate {
        operation: payload
            .get("source")
            .and_then(serde_json::Value::as_str)
            .or_else(|| payload.get("op").and_then(serde_json::Value::as_str))
            .unwrap_or("memory")
            .into(),
        content: payload
            .get("summary")
            .and_then(serde_json::Value::as_str)
            .or_else(|| payload.get("content").and_then(serde_json::Value::as_str))
            .unwrap_or_default()
            .into(),
    }
}

fn context_usage_event(payload: &str) -> ChatStreamEvent {
    let value = serde_json::from_str::<serde_json::Value>(payload).unwrap_or_default();
    ChatStreamEvent::ContextUsage {
        context_window: json_u32(&value, "context_window"),
        total_tokens: json_u32(&value, "total_tokens"),
        segments: value
            .get("segments")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .map(|segment| ContextUsageSegmentDto {
                id: json_str(segment, "id"),
                tokens: json_u32(segment, "tokens"),
                count: json_u32(segment, "count"),
                items: segment
                    .get("items")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|item| ContextUsageItemDto {
                        id: json_str(item, "id"),
                        label: json_str(item, "label"),
                        tokens: json_u32(item, "tokens"),
                    })
                    .collect(),
            })
            .collect(),
        updated_at: value
            .get("updated_at")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default(),
        recommend_compact: value
            .get("recommend_compact")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or_default(),
    }
}

fn json_str(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .into()
}

fn json_u32(value: &serde_json::Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
        .min(u32::MAX.into()) as u32
}

fn extension_to_session_event(
    thread_id: &str,
    extension: proto::ThreadExtension,
) -> Option<SessionEventDto> {
    let value = serde_json::from_str::<serde_json::Value>(&extension.payload_json).ok()?;
    let (memory_updated, pending_changed, session_metadata_changed) =
        match extension.namespace.as_str() {
            "astro.memory" => (
                Some(MemoryUpdatedDto {
                    source: json_str(&value, "source"),
                    target: json_str(&value, "target"),
                    summary: json_str(&value, "summary"),
                    live_written: value
                        .get("live_written")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or_default(),
                }),
                None,
                None,
            ),
            "astro.pending" => (
                None,
                Some(PendingChangedDto {
                    pending_count: json_u32(&value, "pending_count"),
                    reason: json_str(&value, "reason"),
                }),
                None,
            ),
            "astro.session_metadata" => (
                None,
                None,
                Some(SessionMetadataChangedDto {
                    title: json_str(&value, "title"),
                }),
            ),
            _ => return None,
        };
    Some(SessionEventDto {
        session_id: Some(thread_id.into()),
        agent_id: String::new(),
        ts_ms: now_ts_ms(),
        memory_updated,
        pending_changed,
        session_metadata_changed,
    })
}

struct SnapshotReconcile {
    terminal: Vec<ChatStreamEvent>,
    terminal_turn_id: Option<String>,
    active_turn_id: Option<String>,
    keep_active: bool,
}

fn reconcile_snapshot(snapshot: &proto::ThreadSnapshot) -> SnapshotReconcile {
    if snapshot.has_active_turn {
        if let Some(turn) = snapshot.active_turn.as_ref() {
            if turn.status == "in_progress" {
                return SnapshotReconcile {
                    terminal: snapshot_turn_recovery_events(turn),
                    terminal_turn_id: None,
                    active_turn_id: Some(turn.id.clone()),
                    keep_active: true,
                };
            }
        }
    }
    let terminal_turn = snapshot
        .active_turn
        .as_ref()
        .filter(|turn| matches!(turn.status.as_str(), "completed" | "failed" | "aborted"))
        .or_else(|| {
            snapshot
                .turns
                .iter()
                .rev()
                .find(|turn| matches!(turn.status.as_str(), "completed" | "failed" | "aborted"))
        });
    if let Some(turn) = terminal_turn {
        let outcome = match turn.status.as_str() {
            "failed" => "error",
            "aborted" => "interrupt",
            _ => "success",
        };
        let mut terminal = snapshot_turn_recovery_events(turn);
        if turn.status == "failed" && turn.has_error {
            if let Some(error) = turn.error.as_ref() {
                terminal.push(ChatStreamEvent::Error {
                    message: error.message.clone(),
                });
            }
        }
        terminal.extend(terminal_events(turn.id.clone(), outcome, "[]".into()));
        return SnapshotReconcile {
            terminal,
            terminal_turn_id: Some(turn.id.clone()),
            active_turn_id: None,
            keep_active: false,
        };
    }
    SnapshotReconcile {
        terminal: Vec::new(),
        terminal_turn_id: None,
        active_turn_id: None,
        keep_active: true,
    }
}

fn snapshot_turn_recovery_events(turn: &proto::ThreadTurn) -> Vec<ChatStreamEvent> {
    let mut events = Vec::new();
    let mut item_agent_messages = Vec::new();
    let mut item_reasoning = Vec::new();
    for item in &turn.items {
        match serde_json::from_str(&item.payload_json) {
            Ok(TurnItem::AgentMessage(message)) => {
                item_agent_messages.push(message.content);
                continue;
            }
            Ok(TurnItem::Reasoning(reasoning)) => {
                item_reasoning.push(reasoning.content);
                continue;
            }
            Ok(TurnItem::Extension(_)) => continue,
            _ => {}
        }
        events.extend(map_item_event(
            proto::ThreadItemEvent {
                item: Some(item.clone()),
            },
            false,
        ));
    }
    if !item_reasoning.is_empty() {
        events.push(ChatStreamEvent::Reasoning {
            content: item_reasoning.concat(),
        });
    }
    // The thread history keeps every assistant item while TurnComplete stores only the final
    // assistant message. Rebuild the same concatenation that live deltas produced, using the
    // terminal field only when that final item was not persisted.
    let final_item_matches_terminal = item_agent_messages
        .last()
        .is_some_and(|message| message == &turn.last_agent_message);
    let has_agent_message_item = !item_agent_messages.is_empty();
    let mut message = item_agent_messages.concat();
    if message.is_empty() || (!turn.last_agent_message.is_empty() && !final_item_matches_terminal) {
        message.push_str(&turn.last_agent_message);
    }
    if has_agent_message_item || !message.is_empty() {
        events.push(ChatStreamEvent::Token { content: message });
    }
    events
}

fn snapshot_extensions(snapshot: &proto::ThreadSnapshot) -> Vec<RecoveredExtension> {
    snapshot
        .turns
        .iter()
        .chain(snapshot.active_turn.iter())
        .flat_map(|turn| turn.items.iter().map(move |item| (turn, item)))
        .filter_map(|(turn, item)| {
            let TurnItem::Extension(extension) =
                serde_json::from_str::<TurnItem>(&item.payload_json).ok()?
            else {
                return None;
            };
            Some(RecoveredExtension {
                turn_id: turn.id.clone(),
                extension: proto::ThreadExtension {
                    item_id: extension.id,
                    namespace: extension.namespace,
                    payload_json: extension.payload.to_string(),
                },
            })
        })
        .collect()
}

async fn recover_snapshot_extensions(
    bridge: &ThreadEventsBridge,
    snapshot: &proto::ThreadSnapshot,
) -> Vec<RecoveredExtension> {
    let mut accepted = Vec::new();
    for recovered in snapshot_extensions(snapshot) {
        let extension = &recovered.extension;
        if bridge
            .accept_extension(
                &snapshot.thread_id,
                &extension.item_id,
                &extension.payload_json,
            )
            .await
        {
            if extension.namespace == "astro.background_complete" {
                bridge
                    .complete_background_turn(&snapshot.thread_id, &recovered.turn_id)
                    .await;
            }
            accepted.push(recovered);
        }
    }
    accepted
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadSnapshotDto<'a> {
    thread_id: &'a str,
    status: &'a str,
    turns: Vec<ThreadTurnDto<'a>>,
    active_turn: Option<ThreadTurnDto<'a>>,
    has_active_turn: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadTurnDto<'a> {
    id: &'a str,
    status: &'a str,
    items: Vec<ThreadItemDto<'a>>,
    last_agent_message: &'a str,
    has_error: bool,
    error: Option<ThreadErrorDto<'a>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadItemDto<'a> {
    id: &'a str,
    item_type: &'a str,
    status: &'a str,
    payload_json: &'a str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadErrorDto<'a> {
    message: &'a str,
    error_type: &'a str,
}

fn turn_dto(turn: &proto::ThreadTurn) -> ThreadTurnDto<'_> {
    ThreadTurnDto {
        id: &turn.id,
        status: &turn.status,
        items: turn
            .items
            .iter()
            .map(|item| ThreadItemDto {
                id: &item.id,
                item_type: &item.item_type,
                status: &item.status,
                payload_json: &item.payload_json,
            })
            .collect(),
        last_agent_message: &turn.last_agent_message,
        has_error: turn.has_error,
        error: turn.error.as_ref().map(|error| ThreadErrorDto {
            message: &error.message,
            error_type: &error.error_type,
        }),
    }
}

fn snapshot_dto(snapshot: &proto::ThreadSnapshot) -> ThreadSnapshotDto<'_> {
    ThreadSnapshotDto {
        thread_id: &snapshot.thread_id,
        status: &snapshot.status,
        turns: snapshot.turns.iter().map(turn_dto).collect(),
        active_turn: snapshot.active_turn.as_ref().map(turn_dto),
        has_active_turn: snapshot.has_active_turn,
    }
}

fn emit_snapshot(app: &AppHandle, snapshot: &proto::ThreadSnapshot) {
    let _ = app.emit(SNAPSHOT_EVENT, snapshot_dto(snapshot));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn terminal_event(thread_id: &str, turn_id: &str) -> proto::ThreadEvent {
        proto::ThreadEvent {
            thread_id: thread_id.into(),
            turn_id: turn_id.into(),
            payload: Some(proto::thread_event::Payload::TurnComplete(
                proto::ThreadTurnComplete {
                    last_agent_message: "done".into(),
                    error: None,
                    has_error: false,
                },
            )),
        }
    }

    fn terminal_projection(turn_id: &str) -> Vec<ChatStreamEvent> {
        map_thread_event(terminal_event("session-1", turn_id))
    }

    fn agent_message_item(id: &str, content: &str) -> proto::ThreadItem {
        proto::ThreadItem {
            id: id.into(),
            item_type: "agent_message".into(),
            status: "completed".into(),
            payload_json: serde_json::to_string(&TurnItem::AgentMessage(
                agent_protocol::TextItem {
                    id: id.into(),
                    content: content.into(),
                },
            ))
            .unwrap(),
        }
    }

    fn reasoning_item(id: &str, content: &str) -> proto::ThreadItem {
        proto::ThreadItem {
            id: id.into(),
            item_type: "reasoning".into(),
            status: "in_progress".into(),
            payload_json: serde_json::to_string(&TurnItem::Reasoning(agent_protocol::TextItem {
                id: id.into(),
                content: content.into(),
            }))
            .unwrap(),
        }
    }

    async fn deferred_terminal_count(bridge: &ThreadEventsBridge, thread_id: &str) -> usize {
        bridge
            .active_threads
            .read()
            .await
            .deferred_terminals
            .get(thread_id)
            .map(HashMap::len)
            .unwrap_or_default()
    }

    #[test]
    fn terminal_thread_event_maps_to_run_finished_then_done() {
        let mapped = map_thread_event(terminal_event("session-1", "turn-1"));
        assert!(matches!(
            mapped.as_slice(),
            [
                ChatStreamEvent::RunFinished { outcome_type, .. },
                ChatStreamEvent::Done
            ] if outcome_type == "success"
        ));
    }

    #[tokio::test]
    async fn core_error_then_failed_terminal_projects_one_error() {
        let error = proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::Error(proto::ThreadError {
                message: "boom".into(),
                error_type: "provider".into(),
            })),
        };
        let terminal = proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::TurnComplete(
                proto::ThreadTurnComplete {
                    last_agent_message: String::new(),
                    error: Some(proto::ThreadError {
                        message: "boom".into(),
                        error_type: "provider".into(),
                    }),
                    has_error: true,
                },
            )),
        };
        let bridge = ThreadEventsBridge::new();
        let activation = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await;
        let mut projected = map_thread_event(error);
        bridge
            .record_delivered_projection("session-1", "turn-1", &projected)
            .await;
        projected.extend(
            bridge
                .dedup_terminal_projection("session-1", "turn-1", map_thread_event(terminal))
                .await,
        );
        assert_eq!(
            projected
                .iter()
                .filter(|event| matches!(event, ChatStreamEvent::Error { .. }))
                .count(),
            1
        );
        assert!(matches!(
            projected.as_slice(),
            [
                ChatStreamEvent::Error { .. },
                ChatStreamEvent::RunFinished { outcome_type, .. },
                ChatStreamEvent::Done
            ] if outcome_type == "error"
        ));
    }

    #[test]
    fn terminal_only_error_is_preserved_before_error_outcome() {
        let terminal = proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::TurnComplete(
                proto::ThreadTurnComplete {
                    last_agent_message: String::new(),
                    error: Some(proto::ThreadError {
                        message: "terminal failure".into(),
                        error_type: "provider".into(),
                    }),
                    has_error: true,
                },
            )),
        };

        assert!(matches!(
            map_thread_event(terminal).as_slice(),
            [
                ChatStreamEvent::Error { message },
                ChatStreamEvent::RunFinished { outcome_type, .. },
                ChatStreamEvent::Done
            ] if message == "terminal failure" && outcome_type == "error"
        ));
    }

    #[tokio::test]
    async fn distinct_standalone_and_terminal_errors_are_both_projected() {
        let standalone = proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::Error(proto::ThreadError {
                message: "stream failure".into(),
                error_type: "stream".into(),
            })),
        };
        let terminal = proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::TurnComplete(
                proto::ThreadTurnComplete {
                    last_agent_message: String::new(),
                    error: Some(proto::ThreadError {
                        message: "final failure".into(),
                        error_type: "provider".into(),
                    }),
                    has_error: true,
                },
            )),
        };
        let bridge = ThreadEventsBridge::new();
        let activation = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await;
        let mut projected = map_thread_event(standalone);
        bridge
            .record_delivered_projection("session-1", "turn-1", &projected)
            .await;
        projected.extend(
            bridge
                .dedup_terminal_projection("session-1", "turn-1", map_thread_event(terminal))
                .await,
        );

        assert_eq!(
            projected
                .iter()
                .filter(|event| matches!(event, ChatStreamEvent::Error { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn memory_extension_maps_to_existing_session_event_shape() {
        let event = proto::ThreadExtension {
            item_id: "memory-1".into(),
            namespace: "astro.memory".into(),
            payload_json: serde_json::json!({
                "source":"review",
                "target":"memory",
                "summary":"updated",
                "live_written":true
            })
            .to_string(),
        };
        let mapped = extension_to_session_event("session-1", event).expect("memory event");
        let memory = mapped.memory_updated.expect("memory payload");
        assert_eq!(mapped.session_id.as_deref(), Some("session-1"));
        assert_eq!(memory.source, "review");
        assert!(memory.live_written);
    }

    #[test]
    fn memory_extension_chat_adapter_reads_new_schema_and_keeps_legacy_fallback() {
        let current = map_extension_to_chat(proto::ThreadExtension {
            item_id: "memory-current".into(),
            namespace: "astro.memory".into(),
            payload_json: serde_json::json!({
                "source":"review",
                "target":"memory",
                "summary":"updated",
                "live_written":true
            })
            .to_string(),
        });
        assert!(matches!(
            current.as_slice(),
            [ChatStreamEvent::MemoryUpdate { operation, content }]
                if operation == "review" && content == "updated"
        ));

        let legacy = map_extension_to_chat(proto::ThreadExtension {
            item_id: "memory-legacy".into(),
            namespace: "astro.memory".into(),
            payload_json: serde_json::json!({"op":"memory","content":"legacy"}).to_string(),
        });
        assert!(matches!(
            legacy.as_slice(),
            [ChatStreamEvent::MemoryUpdate { operation, content }]
                if operation == "memory" && content == "legacy"
        ));
    }

    #[tokio::test]
    async fn snapshot_extension_recovery_is_idempotent_by_stable_item_id() {
        let extension = TurnItem::Extension(agent_protocol::ExtensionItem {
            id: "turn-1:memory:review".into(),
            namespace: "astro.memory".into(),
            payload: serde_json::json!({
                "source":"review",
                "target":"memory",
                "summary":"updated",
                "live_written":true
            }),
        });
        let mut snapshot = proto::ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "idle".into(),
            turns: vec![proto::ThreadTurn {
                id: "turn-1".into(),
                status: "completed".into(),
                items: vec![proto::ThreadItem {
                    id: "turn-1:memory:review".into(),
                    item_type: "extension".into(),
                    status: "completed".into(),
                    payload_json: serde_json::to_string(&extension).unwrap(),
                }],
                last_agent_message: String::new(),
                error: None,
                has_error: false,
            }],
            active_turn: None,
            has_active_turn: false,
        };
        let bridge = ThreadEventsBridge::new();
        let first = recover_snapshot_extensions(&bridge, &snapshot).await;
        let second = recover_snapshot_extensions(&bridge, &snapshot).await;
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].extension.namespace, "astro.memory");
        assert!(
            second.is_empty(),
            "unchanged snapshot must not replay twice"
        );

        snapshot.turns[0].items[0].payload_json =
            serde_json::to_string(&TurnItem::Extension(agent_protocol::ExtensionItem {
                id: "turn-1:memory:review".into(),
                namespace: "astro.memory".into(),
                payload: serde_json::json!({
                    "source":"review",
                    "target":"memory",
                    "summary":"updated again",
                    "live_written":true
                }),
            }))
            .unwrap();
        assert_eq!(
            recover_snapshot_extensions(&bridge, &snapshot).await.len(),
            1,
            "same stable item id with new state must still be delivered"
        );
    }

    #[test]
    fn terminal_snapshot_synthesizes_terminal_before_buffered_live_events() {
        let snapshot = proto::ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "idle".into(),
            turns: vec![proto::ThreadTurn {
                id: "turn-1".into(),
                status: "completed".into(),
                items: vec![
                    proto::ThreadItem {
                        id: "message-1".into(),
                        item_type: "agent_message".into(),
                        status: "completed".into(),
                        payload_json: serde_json::to_string(&TurnItem::AgentMessage(
                            agent_protocol::TextItem {
                                id: "message-1".into(),
                                content: "almost ".into(),
                            },
                        ))
                        .unwrap(),
                    },
                    proto::ThreadItem {
                        id: "message-2".into(),
                        item_type: "agent_message".into(),
                        status: "completed".into(),
                        payload_json: serde_json::to_string(&TurnItem::AgentMessage(
                            agent_protocol::TextItem {
                                id: "message-2".into(),
                                content: "done".into(),
                            },
                        ))
                        .unwrap(),
                    },
                ],
                last_agent_message: "done".into(),
                error: None,
                has_error: false,
            }],
            active_turn: None,
            has_active_turn: false,
        };
        let outcome = reconcile_snapshot(&snapshot);
        assert!(matches!(
            outcome.terminal.as_slice(),
            [
                ChatStreamEvent::Token { content },
                ChatStreamEvent::RunFinished { outcome_type, .. },
                ChatStreamEvent::Done
            ] if content == "almost done" && outcome_type == "success"
        ));
        assert!(!outcome.keep_active);
    }

    #[test]
    fn running_snapshot_keeps_thread_active_without_terminal_projection() {
        let snapshot = proto::ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "running".into(),
            turns: vec![],
            active_turn: Some(proto::ThreadTurn {
                id: "turn-1".into(),
                status: "in_progress".into(),
                items: vec![],
                last_agent_message: String::new(),
                error: None,
                has_error: false,
            }),
            has_active_turn: true,
        };
        let outcome = reconcile_snapshot(&snapshot);
        assert!(outcome.keep_active);
        assert!(outcome.terminal.is_empty());
    }

    #[tokio::test]
    async fn running_snapshot_recovers_agent_text_before_buffered_terminal() {
        let snapshot = proto::ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "running".into(),
            turns: vec![],
            active_turn: Some(proto::ThreadTurn {
                id: "turn-1".into(),
                status: "in_progress".into(),
                items: vec![agent_message_item("message-1", "done")],
                last_agent_message: String::new(),
                error: None,
                has_error: false,
            }),
            has_active_turn: true,
        };
        let bridge = ThreadEventsBridge::new();
        let activation = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await;

        let reconciled = reconcile_snapshot(&snapshot);
        let mut projected = bridge
            .recover_snapshot_projection("session-1", "turn-1", reconciled.terminal)
            .await;
        projected.extend(
            bridge
                .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
                .await,
        );

        assert!(matches!(
            projected.as_slice(),
            [
                ChatStreamEvent::Token { content },
                ChatStreamEvent::RunFinished { .. },
                ChatStreamEvent::Done
            ] if content == "done"
        ));
    }

    #[test]
    fn reconnect_generation_delivers_every_snapshot_before_buffered_live() {
        let snapshots = vec![proto::ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "running".into(),
            turns: vec![],
            active_turn: None,
            has_active_turn: false,
        }];
        let live = vec![Ok(proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::AgentMessageDelta(
                proto::ThreadDelta {
                    item_id: "message-1".into(),
                    delta: "late".into(),
                },
            )),
        })];
        let ordered = reconnect_delivery_order(snapshots, live);
        assert!(matches!(
            ordered.first(),
            Some(ReconnectDelivery::Snapshot(_))
        ));
        assert!(matches!(ordered.get(1), Some(ReconnectDelivery::Live(_))));
    }

    #[tokio::test]
    async fn duplicate_terminal_is_suppressed_and_removes_active_thread() {
        let bridge = ThreadEventsBridge::new();
        let activation = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await;
        assert!(!bridge
            .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
            .await
            .is_empty());
        assert!(bridge
            .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
            .await
            .is_empty());
        assert!(!bridge.is_active("session-1").await);
    }

    #[tokio::test]
    async fn successful_terminal_moves_thread_to_background_resume_set_until_marker() {
        let bridge = ThreadEventsBridge::new();
        let activation = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await;

        assert!(!bridge
            .accept_terminal_with_background(
                "session-1",
                "turn-1",
                terminal_projection("turn-1"),
                true,
            )
            .await
            .is_empty());
        assert!(!bridge.is_active("session-1").await);
        assert_eq!(
            bridge.background_resume_threads().await,
            vec!["session-1".to_string()]
        );

        assert!(bridge.complete_background_turn("session-1", "turn-1").await);
        assert!(bridge.background_resume_threads().await.is_empty());
    }

    #[tokio::test]
    async fn offline_review_and_marker_recover_once_then_release_background_subscription() {
        let bridge = ThreadEventsBridge::new();
        let mut cleanup = bridge.take_terminal_unsubscribe_requests();
        let activation = bridge.activate("session-offline").await;
        bridge
            .bind_submitted_turn_if_current("session-offline", activation, "turn-offline")
            .await;
        bridge
            .accept_terminal_with_background(
                "session-offline",
                "turn-offline",
                terminal_projection("turn-offline"),
                true,
            )
            .await;
        let terminal_cleanup = cleanup.recv().await.expect("terminal cleanup");
        assert!(bridge
            .run_terminal_unsubscribe_if_owned(terminal_cleanup, |_| async { Ok(()) })
            .await
            .unwrap());
        let background_targets = bridge.background_resume_targets().await;
        let [(thread_id, subscription)] = background_targets.as_slice() else {
            panic!("one background Resume target expected");
        };
        let subscriber = Arc::new(AtomicBool::new(false));
        let resumed = Arc::clone(&subscriber);
        assert_eq!(
            bridge
                .run_background_resume_if_owned(
                    thread_id.clone(),
                    *subscription,
                    move || async move {
                        resumed.store(true, Ordering::SeqCst);
                        Ok::<_, String>(())
                    },
                )
                .await
                .unwrap(),
            Some(())
        );

        let review = TurnItem::Extension(agent_protocol::ExtensionItem {
            id: "turn-offline:memory:review".into(),
            namespace: "astro.memory".into(),
            payload: serde_json::json!({
                "source":"review",
                "target":"memory",
                "summary":"offline review",
                "live_written":true
            }),
        });
        let marker = TurnItem::Extension(agent_protocol::ExtensionItem {
            id: "turn-offline:background_complete".into(),
            namespace: "astro.background_complete".into(),
            payload: serde_json::json!({}),
        });
        let mut snapshot = proto::ThreadSnapshot {
            thread_id: "session-offline".into(),
            status: "idle".into(),
            turns: vec![proto::ThreadTurn {
                id: "turn-offline".into(),
                status: "completed".into(),
                items: vec![proto::ThreadItem {
                    id: "turn-offline:memory:review".into(),
                    item_type: "extension".into(),
                    status: "completed".into(),
                    payload_json: serde_json::to_string(&review).unwrap(),
                }],
                last_agent_message: "done".into(),
                error: None,
                has_error: false,
            }],
            active_turn: None,
            has_active_turn: false,
        };

        let recovered = recover_snapshot_extensions(&bridge, &snapshot).await;
        let session_events = recovered
            .iter()
            .filter_map(|extension| {
                extension_to_session_event("session-offline", extension.extension.clone())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            session_events.len(),
            1,
            "offline review must reach session_event"
        );
        assert_eq!(
            session_events[0]
                .memory_updated
                .as_ref()
                .expect("memory event")
                .summary,
            "offline review"
        );
        assert_eq!(
            bridge.background_resume_threads().await,
            vec!["session-offline".to_string()],
            "without the completion marker the resumed formal subscription stays live"
        );
        assert!(subscriber.load(Ordering::SeqCst));
        assert!(cleanup.try_recv().is_err());

        snapshot.turns[0].items.push(proto::ThreadItem {
            id: "turn-offline:background_complete".into(),
            item_type: "extension".into(),
            status: "completed".into(),
            payload_json: serde_json::to_string(&marker).unwrap(),
        });
        let marker_recovery = recover_snapshot_extensions(&bridge, &snapshot).await;
        assert_eq!(marker_recovery.len(), 1);
        assert_eq!(
            marker_recovery[0].extension.namespace,
            "astro.background_complete"
        );
        assert!(bridge.background_resume_threads().await.is_empty());
        let background_cleanup = cleanup.recv().await.expect("background cleanup");
        let unsubscribed = Arc::clone(&subscriber);
        assert!(bridge
            .run_terminal_unsubscribe_if_owned(background_cleanup, move |_| async move {
                unsubscribed.store(false, Ordering::SeqCst);
                Ok(())
            })
            .await
            .unwrap());
        assert!(!subscriber.load(Ordering::SeqCst));
        assert!(recover_snapshot_extensions(&bridge, &snapshot)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn accepted_terminal_queues_and_executes_exact_unsubscribe() {
        let bridge = ThreadEventsBridge::new();
        let mut requests = bridge.take_terminal_unsubscribe_requests();
        let activation = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await;
        assert!(!bridge
            .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
            .await
            .is_empty());

        let request = requests.recv().await.expect("terminal unsubscribe");
        assert_eq!(request.thread_id, "session-1");
        assert_eq!(request.activation, activation);
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        assert!(bridge
            .run_terminal_unsubscribe_if_owned(request, move |thread_id| async move {
                assert_eq!(thread_id, "session-1");
                counted.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .await
            .unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn recovery_skips_captured_activation_after_failure_cleanup_wins() {
        let bridge = Arc::new(ThreadEventsBridge::new());
        let mut cleanup = bridge.take_terminal_unsubscribe_requests();
        let activation = bridge.activate("session-1").await;
        let captured = bridge.active_activations().await;
        let [(thread_id, captured_activation)] = captured.as_slice() else {
            panic!("one captured activation expected");
        };
        let thread_id = thread_id.clone();
        let captured_activation = *captured_activation;
        let subscriber = Arc::new(AtomicBool::new(true));
        let paused = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());

        let resume_bridge = Arc::clone(&bridge);
        let resume_subscriber = Arc::clone(&subscriber);
        let resume_paused = Arc::clone(&paused);
        let resume_release = Arc::clone(&release);
        let resume = tokio::spawn(async move {
            resume_paused.notify_one();
            resume_release.notified().await;
            resume_bridge
                .run_resume_if_owned(thread_id, captured_activation, move || async move {
                    resume_subscriber.store(true, Ordering::SeqCst);
                    Ok::<_, String>(())
                })
                .await
        });
        paused.notified().await;

        assert!(bridge.fail_activation("session-1", activation).await);
        let request = cleanup.recv().await.expect("failed activation cleanup");
        let cleanup_subscriber = Arc::clone(&subscriber);
        assert!(bridge
            .run_terminal_unsubscribe_if_owned(request, move |_| async move {
                cleanup_subscriber.store(false, Ordering::SeqCst);
                Ok(())
            })
            .await
            .unwrap());

        release.notify_one();
        assert_eq!(resume.await.unwrap().unwrap(), None);
        assert!(!subscriber.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn failure_cleanup_runs_after_in_flight_recovery_resume() {
        let bridge = Arc::new(ThreadEventsBridge::new());
        let mut cleanup = bridge.take_terminal_unsubscribe_requests();
        let activation = bridge.activate("session-1").await;
        let subscriber = Arc::new(AtomicBool::new(true));
        let resume_entered = Arc::new(tokio::sync::Notify::new());
        let resume_release = Arc::new(tokio::sync::Notify::new());

        let resume_bridge = Arc::clone(&bridge);
        let resume_subscriber = Arc::clone(&subscriber);
        let entered = Arc::clone(&resume_entered);
        let release = Arc::clone(&resume_release);
        let resume = tokio::spawn(async move {
            resume_bridge
                .run_resume_if_owned("session-1".into(), activation, move || async move {
                    entered.notify_one();
                    release.notified().await;
                    resume_subscriber.store(true, Ordering::SeqCst);
                    Ok::<_, String>(())
                })
                .await
        });
        resume_entered.notified().await;

        assert!(bridge.fail_activation("session-1", activation).await);
        let request = cleanup.recv().await.expect("failed activation cleanup");
        let cleanup_bridge = Arc::clone(&bridge);
        let cleanup_subscriber = Arc::clone(&subscriber);
        let mut cleanup_task = tokio::spawn(async move {
            cleanup_bridge
                .run_terminal_unsubscribe_if_owned(request, move |_| async move {
                    cleanup_subscriber.store(false, Ordering::SeqCst);
                    Ok(())
                })
                .await
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut cleanup_task)
                .await
                .is_err()
        );

        resume_release.notify_one();
        assert_eq!(resume.await.unwrap().unwrap(), Some(()));
        assert!(cleanup_task.await.unwrap().unwrap());
        assert!(!subscriber.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn captured_recovery_cannot_resume_over_a_new_activation_owner() {
        let bridge = ThreadEventsBridge::new();
        let old = bridge.activate("session-1").await;
        let captured = bridge.active_activations().await;
        let [(thread_id, captured_activation)] = captured.as_slice() else {
            panic!("one captured activation expected");
        };
        assert_eq!(*captured_activation, old);
        let thread_id = thread_id.clone();

        let current = bridge.activate("session-1").await;
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        assert_eq!(
            bridge
                .run_resume_if_owned(thread_id, old, move || async move {
                    counted.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, String>(())
                })
                .await
                .unwrap(),
            None
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            bridge
                .active_threads
                .read()
                .await
                .activations
                .get("session-1"),
            Some(&current)
        );
        assert_eq!(
            bridge.terminal_cleanup.owners.read().await.get("session-1"),
            Some(&current)
        );
    }

    #[tokio::test]
    async fn new_activation_waits_for_in_flight_recovery_and_then_owns_subscription() {
        let bridge = Arc::new(ThreadEventsBridge::new());
        let old = bridge.activate("session-1").await;
        let resume_entered = Arc::new(tokio::sync::Notify::new());
        let resume_release = Arc::new(tokio::sync::Notify::new());

        let resume_bridge = Arc::clone(&bridge);
        let entered = Arc::clone(&resume_entered);
        let release = Arc::clone(&resume_release);
        let resume = tokio::spawn(async move {
            resume_bridge
                .run_resume_if_owned("session-1".into(), old, move || async move {
                    entered.notify_one();
                    release.notified().await;
                    Ok::<_, String>(())
                })
                .await
        });
        resume_entered.notified().await;

        let activation_bridge = Arc::clone(&bridge);
        let mut activation =
            tokio::spawn(async move { activation_bridge.activate("session-1").await });
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut activation)
                .await
                .is_err()
        );

        resume_release.notify_one();
        assert_eq!(resume.await.unwrap().unwrap(), Some(()));
        let current = activation.await.unwrap();
        assert_ne!(current, old);
        assert_eq!(
            bridge
                .active_threads
                .read()
                .await
                .activations
                .get("session-1"),
            Some(&current)
        );
        assert_eq!(
            bridge.terminal_cleanup.owners.read().await.get("session-1"),
            Some(&current)
        );
    }

    #[tokio::test]
    async fn stale_terminal_unsubscribe_cannot_remove_new_activation() {
        let bridge = ThreadEventsBridge::new();
        let mut requests = bridge.take_terminal_unsubscribe_requests();
        let old = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", old, "turn-old")
            .await;
        bridge
            .accept_terminal("session-1", "turn-old", terminal_projection("turn-old"))
            .await;
        let stale = requests.recv().await.expect("old terminal unsubscribe");

        let current = bridge.activate("session-1").await;
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        assert!(!bridge
            .run_terminal_unsubscribe_if_owned(stale, move |_| async move {
                counted.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .await
            .unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(bridge.is_active("session-1").await);
        assert_eq!(
            bridge
                .active_threads
                .read()
                .await
                .activations
                .get("session-1"),
            Some(&current)
        );
    }

    #[tokio::test]
    async fn terminal_subscription_gate_registry_prunes_unique_threads() {
        let bridge = ThreadEventsBridge::new();
        for index in 0..1_000 {
            let gate = bridge
                .terminal_cleanup
                .gate(&format!("unique-session-{index}"));
            let guard = gate.lock().await;
            drop(guard);
            drop(gate);
        }

        assert!(bridge
            .terminal_cleanup
            .gates
            .lock()
            .expect("terminal gate registry")
            .is_empty());
    }

    #[tokio::test]
    async fn steered_terminal_unsubscribe_is_owned_by_current_activation() {
        let bridge = ThreadEventsBridge::new();
        let mut requests = bridge.take_terminal_unsubscribe_requests();
        let old = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", old, "turn-1")
            .await;
        let current = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", current, "turn-1")
            .await;
        assert!(!bridge
            .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
            .await
            .is_empty());

        let request = requests.recv().await.expect("steered unsubscribe");
        assert_eq!(request.activation, current);
    }

    #[tokio::test]
    async fn late_ack_after_accepted_terminal_cannot_recreate_thread_state() {
        let bridge = ThreadEventsBridge::new();
        let activation = bridge.activate("session-1").await;
        bridge.bind_observed_turn("session-1", "turn-1").await;
        assert!(!bridge
            .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
            .await
            .is_empty());

        assert!(bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await
            .is_empty());
        let state = bridge.active_threads.read().await;
        assert!(!state.threads.contains("session-1"));
        assert!(!state.turn_epochs.contains_key("session-1"));
        assert!(!state.activations.contains_key("session-1"));
        assert!(!state.awaiting_submissions.contains_key("session-1"));
        assert!(!state.deferred_terminals.contains_key("session-1"));
        assert!(!state.delivered_agent_text.contains_key("session-1"));
        assert!(!state.delivered_reasoning.contains_key("session-1"));
        assert!(!state.pending_terminal_errors.contains_key("session-1"));
    }

    #[tokio::test]
    async fn stale_submit_failure_cannot_remove_a_newer_activation() {
        let bridge = ThreadEventsBridge::new();
        let mut cleanup = bridge.take_terminal_unsubscribe_requests();
        let old = bridge.activate("session-1").await;
        let current = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", current, "turn-current")
            .await;

        assert!(!bridge.fail_activation("session-1", old).await);
        assert!(cleanup.try_recv().is_err());
        assert!(bridge.is_active("session-1").await);
        assert_eq!(
            bridge
                .active_threads
                .read()
                .await
                .activations
                .get("session-1"),
            Some(&current)
        );
    }

    #[tokio::test]
    async fn current_replacement_failure_queues_activation_owned_unsubscribe() {
        let bridge = ThreadEventsBridge::new();
        let mut cleanup = bridge.take_terminal_unsubscribe_requests();
        let _old = bridge.activate("session-1").await;
        let current = bridge.activate("session-1").await;

        assert!(bridge.fail_activation("session-1", current).await);
        assert!(!bridge.is_active("session-1").await);
        let request = cleanup.recv().await.expect("current failure cleanup");
        assert_eq!(request.activation, current);
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        assert!(bridge
            .run_terminal_unsubscribe_if_owned(request, move |_| async move {
                counted.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .await
            .unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!bridge
            .terminal_cleanup
            .owners
            .read()
            .await
            .contains_key("session-1"));
    }

    #[tokio::test]
    async fn old_turn_terminal_cannot_retire_a_new_activation() {
        let bridge = ThreadEventsBridge::new();
        let old = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", old, "turn-old")
            .await;
        let current = bridge.activate("session-1").await;

        // A reconnect snapshot can observe the old active turn again. Rebinding it must not
        // promote that turn into the new activation epoch.
        bridge.bind_observed_turn("session-1", "turn-old").await;

        assert!(bridge
            .accept_terminal("session-1", "turn-old", terminal_projection("turn-old"))
            .await
            .is_empty());
        assert!(bridge.is_active("session-1").await);

        bridge
            .bind_submitted_turn_if_current("session-1", current, "turn-current")
            .await;
        assert!(!bridge
            .accept_terminal(
                "session-1",
                "turn-current",
                terminal_projection("turn-current")
            )
            .await
            .is_empty());
        assert!(!bridge.is_active("session-1").await);
    }

    #[tokio::test]
    async fn authoritative_steered_submit_rebinds_same_turn_to_current_activation() {
        let bridge = ThreadEventsBridge::new();
        let old = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", old, "turn-1")
            .await;

        let current = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", current, "turn-1")
            .await;

        assert!(!bridge
            .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
            .await
            .is_empty());
        assert!(!bridge.is_active("session-1").await);
    }

    #[tokio::test]
    async fn late_old_ack_and_turn_started_cannot_bind_the_current_activation() {
        let bridge = ThreadEventsBridge::new();
        let old = bridge.activate("session-1").await;
        let current = bridge.activate("session-1").await;

        assert!(bridge
            .bind_submitted_turn_if_current("session-1", old, "turn-old")
            .await
            .is_empty());
        bridge.bind_observed_turn("session-1", "turn-old").await;
        assert!(bridge
            .accept_terminal("session-1", "turn-old", terminal_projection("turn-old"))
            .await
            .is_empty());
        assert!(bridge.is_active("session-1").await);

        bridge
            .bind_submitted_turn_if_current("session-1", current, "turn-current")
            .await;
        assert!(!bridge
            .accept_terminal(
                "session-1",
                "turn-current",
                terminal_projection("turn-current")
            )
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn first_turn_started_still_binds_the_only_pending_activation() {
        let bridge = ThreadEventsBridge::new();
        bridge.activate("session-1").await;
        bridge.bind_observed_turn("session-1", "turn-1").await;

        assert!(!bridge
            .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
            .await
            .is_empty());
        assert!(!bridge.is_active("session-1").await);
    }

    #[tokio::test]
    async fn pre_ack_terminal_is_released_by_authoritative_steered_binding() {
        let bridge = ThreadEventsBridge::new();
        let old = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", old, "turn-1")
            .await;

        let current = bridge.activate("session-1").await;
        assert!(bridge
            .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
            .await
            .is_empty());
        assert!(bridge.is_active("session-1").await);

        let released = bridge
            .bind_submitted_turn_if_current("session-1", current, "turn-1")
            .await;
        assert!(matches!(
            released.as_slice(),
            [
                ChatStreamEvent::RunFinished {
                    run_id,
                    outcome_type,
                    ..
                },
                ChatStreamEvent::Done
            ] if run_id == "turn-1" && outcome_type == "success"
        ));
        assert!(!bridge.is_active("session-1").await);
    }

    #[tokio::test]
    async fn submission_failure_clears_its_deferred_terminal() {
        let bridge = ThreadEventsBridge::new();
        let old = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", old, "turn-1")
            .await;
        let failed = bridge.activate("session-1").await;
        assert!(bridge
            .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
            .await
            .is_empty());
        assert_eq!(deferred_terminal_count(&bridge, "session-1").await, 1);

        assert!(bridge.fail_activation("session-1", failed).await);
        assert_eq!(deferred_terminal_count(&bridge, "session-1").await, 0);

        let next = bridge.activate("session-1").await;
        assert!(bridge
            .bind_submitted_turn_if_current("session-1", next, "turn-1")
            .await
            .is_empty());
        assert!(bridge.is_active("session-1").await);
    }

    #[tokio::test]
    async fn replacement_activation_clears_older_deferred_terminal() {
        let bridge = ThreadEventsBridge::new();
        let old = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", old, "turn-1")
            .await;
        let replaced = bridge.activate("session-1").await;
        assert!(bridge
            .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
            .await
            .is_empty());
        assert_eq!(deferred_terminal_count(&bridge, "session-1").await, 1);

        let current = bridge.activate("session-1").await;
        assert_eq!(deferred_terminal_count(&bridge, "session-1").await, 0);
        assert!(bridge
            .bind_submitted_turn_if_current("session-1", replaced, "turn-1")
            .await
            .is_empty());
        assert!(bridge
            .bind_submitted_turn_if_current("session-1", current, "turn-1")
            .await
            .is_empty());
        assert!(bridge.is_active("session-1").await);
    }

    #[tokio::test]
    async fn start_chat_registers_new_epoch_before_blocked_ready_wait() {
        let bridge = ThreadEventsBridge::new();
        let old = bridge.activate("session-1").await;
        bridge.mark_recovering();

        // The invocation is registered synchronously even though its RPC must wait for recovery.
        let current = bridge.activate("session-1").await;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), bridge.wait_ready())
                .await
                .is_err()
        );

        let old_is_current = bridge.fail_activation("session-1", old).await;
        assert!(submission_failure_events(old_is_current, "old failure").is_empty());
        assert!(bridge.is_active("session-1").await);

        assert!(bridge.fail_activation("session-1", current).await);
        assert!(!bridge.is_active("session-1").await);

        // Keep the command integration honest: activation must happen before the task can block
        // on readiness, otherwise the state assertions above do not describe `start_chat`.
        let source = include_str!("../commands/chat.rs");
        let activation = source
            .find("let activation = bridge.activate(sid2.clone()).await;")
            .expect("start_chat activation marker");
        let spawn = source
            .find("tauri::async_runtime::spawn(async move {")
            .expect("start_chat spawn marker");
        let wait_ready = source
            .find(".wait_ready_for(THREAD_EVENTS_READY_TIMEOUT)")
            .expect("start_chat readiness marker");
        assert!(activation < spawn && spawn < wait_ready);
    }

    #[tokio::test]
    async fn ready_state_cannot_lose_a_wakeup() {
        let bridge = std::sync::Arc::new(ThreadEventsBridge::new());
        bridge.set_ready(true);
        tokio::time::timeout(std::time::Duration::from_millis(50), bridge.wait_ready())
            .await
            .expect("already-ready state must return immediately");
    }

    #[tokio::test]
    async fn reconnect_is_not_publicly_ready_until_snapshot_barrier_finishes() {
        let bridge = std::sync::Arc::new(ThreadEventsBridge::new());
        bridge.mark_recovering();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), bridge.wait_ready())
                .await
                .is_err()
        );
        bridge.mark_recovered();
        tokio::time::timeout(std::time::Duration::from_millis(50), bridge.wait_ready())
            .await
            .expect("completed recovery must release submitters");
    }

    #[tokio::test]
    async fn completed_snapshot_only_recovers_missing_agent_text_before_done() {
        let bridge = ThreadEventsBridge::new();
        let activation = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await;
        bridge
            .record_delivered_projection(
                "session-1",
                "turn-1",
                &[
                    ChatStreamEvent::Token {
                        content: "hello".into(),
                    },
                    ChatStreamEvent::Error {
                        message: "boom".into(),
                    },
                ],
            )
            .await;

        let recovered = bridge
            .recover_snapshot_projection(
                "session-1",
                "turn-1",
                vec![
                    ChatStreamEvent::Token {
                        content: "hello world".into(),
                    },
                    ChatStreamEvent::Error {
                        message: "boom".into(),
                    },
                    ChatStreamEvent::RunFinished {
                        run_id: "turn-1".into(),
                        outcome_type: "success".into(),
                        interrupts_json: "[]".into(),
                    },
                    ChatStreamEvent::Done,
                ],
            )
            .await;
        let recovered = bridge
            .dedup_terminal_projection("session-1", "turn-1", recovered)
            .await;

        assert!(matches!(
            recovered.as_slice(),
            [
                ChatStreamEvent::Token { content },
                ChatStreamEvent::RunFinished { .. },
                ChatStreamEvent::Done
            ] if content == " world"
        ));
    }

    #[tokio::test]
    async fn divergent_snapshot_uses_canonical_text_reconciliation() {
        let bridge = ThreadEventsBridge::new();
        let activation = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await;
        bridge
            .record_delivered_projection(
                "session-1",
                "turn-1",
                &[ChatStreamEvent::Token {
                    content: "hel world".into(),
                }],
            )
            .await;

        let recovered = bridge
            .recover_snapshot_projection(
                "session-1",
                "turn-1",
                vec![ChatStreamEvent::Token {
                    content: "hello world".into(),
                }],
            )
            .await;

        assert!(matches!(
            recovered.as_slice(),
            [ChatStreamEvent::TextReconcile { content }] if content == "hello world"
        ));
    }

    #[tokio::test]
    async fn completed_empty_agent_message_clears_delivered_draft_before_terminal() {
        let bridge = ThreadEventsBridge::new();
        let activation = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await;
        bridge
            .record_delivered_projection(
                "session-1",
                "turn-1",
                &[ChatStreamEvent::Token {
                    content: "draft".into(),
                }],
            )
            .await;
        let snapshot = proto::ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "completed".into(),
            turns: vec![proto::ThreadTurn {
                id: "turn-1".into(),
                status: "completed".into(),
                items: vec![agent_message_item("message-1", "")],
                last_agent_message: String::new(),
                error: None,
                has_error: false,
            }],
            active_turn: None,
            has_active_turn: false,
        };

        let reconciled = reconcile_snapshot(&snapshot);
        let recovered = bridge
            .recover_snapshot_projection("session-1", "turn-1", reconciled.terminal)
            .await;

        assert!(matches!(
            recovered.as_slice(),
            [
                ChatStreamEvent::TextReconcile { content },
                ChatStreamEvent::RunFinished { .. },
                ChatStreamEvent::Done
            ] if content.is_empty()
        ));
    }

    #[tokio::test]
    async fn running_snapshot_recovers_only_missing_reasoning_before_buffered_live() {
        let bridge = ThreadEventsBridge::new();
        let activation = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await;
        bridge
            .record_delivered_projection(
                "session-1",
                "turn-1",
                &[ChatStreamEvent::Reasoning {
                    content: "seen".into(),
                }],
            )
            .await;
        let snapshot = proto::ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "running".into(),
            turns: vec![],
            active_turn: Some(proto::ThreadTurn {
                id: "turn-1".into(),
                status: "in_progress".into(),
                items: vec![reasoning_item("reasoning-1", "seenlost")],
                last_agent_message: String::new(),
                error: None,
                has_error: false,
            }),
            has_active_turn: true,
        };

        let reconciled = reconcile_snapshot(&snapshot);
        let mut projected = bridge
            .recover_snapshot_projection("session-1", "turn-1", reconciled.terminal)
            .await;
        projected.extend(map_thread_event(proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::ReasoningDelta(
                proto::ThreadDelta {
                    item_id: "reasoning-1".into(),
                    delta: "next".into(),
                },
            )),
        }));

        assert!(matches!(
            projected.as_slice(),
            [
                ChatStreamEvent::Reasoning { content: missing },
                ChatStreamEvent::Reasoning { content: live }
            ] if missing == "lost" && live == "next"
        ));
    }

    #[tokio::test]
    async fn running_snapshot_recovers_full_reasoning_after_total_disconnect() {
        let bridge = ThreadEventsBridge::new();
        let activation = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await;

        let recovered = bridge
            .recover_snapshot_projection(
                "session-1",
                "turn-1",
                snapshot_turn_recovery_events(&proto::ThreadTurn {
                    id: "turn-1".into(),
                    status: "in_progress".into(),
                    items: vec![reasoning_item("reasoning-1", "full")],
                    last_agent_message: String::new(),
                    error: None,
                    has_error: false,
                }),
            )
            .await;

        assert!(matches!(
            recovered.as_slice(),
            [ChatStreamEvent::Reasoning { content }] if content == "full"
        ));
    }

    #[tokio::test]
    async fn divergent_snapshot_uses_canonical_reasoning_reconciliation() {
        let bridge = ThreadEventsBridge::new();
        let activation = bridge.activate("session-1").await;
        bridge
            .bind_submitted_turn_if_current("session-1", activation, "turn-1")
            .await;
        bridge
            .record_delivered_projection(
                "session-1",
                "turn-1",
                &[ChatStreamEvent::Reasoning {
                    content: "hel world".into(),
                }],
            )
            .await;

        let recovered = bridge
            .recover_snapshot_projection(
                "session-1",
                "turn-1",
                vec![ChatStreamEvent::Reasoning {
                    content: "hello world".into(),
                }],
            )
            .await;
        let serialized = serde_json::to_value(&recovered).unwrap();

        assert_eq!(serialized[0]["type"], "reasoning_reconcile");
        assert_eq!(serialized[0]["content"], "hello world");
    }

    #[tokio::test]
    async fn recovery_boundary_drains_events_queued_during_snapshot_rpc() {
        let bridge = ThreadEventsBridge::new();
        bridge.mark_recovering();
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        tx.send(RecoveryIngress::Event(Ok(proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::AgentMessageDelta(
                proto::ThreadDelta {
                    item_id: "message-1".into(),
                    delta: "before-ready".into(),
                },
            )),
        })))
        .await
        .unwrap();
        tx.send(RecoveryIngress::Boundary).await.unwrap();
        tx.send(RecoveryIngress::Event(Ok(proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::AgentMessageDelta(
                proto::ThreadDelta {
                    item_id: "message-1".into(),
                    delta: "after-ready".into(),
                },
            )),
        })))
        .await
        .unwrap();

        let recovery = receive_recovery_batch(&mut rx).await.unwrap();
        assert!(!bridge.is_ready());
        assert_eq!(recovery.len(), 1);
        assert!(matches!(
            rx.recv().await,
            Some(RecoveryIngress::Event(Ok(_)))
        ));
        bridge.complete_recovery(Ok(())).unwrap();
        assert!(bridge.is_ready());
    }

    #[tokio::test]
    async fn ready_wait_timeout_only_retires_its_own_activation() {
        let bridge = ThreadEventsBridge::new();
        let mut cleanup = bridge.take_terminal_unsubscribe_requests();
        bridge.mark_recovering();
        let timed_out = bridge.activate("session-1").await;
        assert!(bridge
            .wait_ready_for(Duration::from_millis(1))
            .await
            .is_err());

        let current = bridge.activate("session-1").await;
        let timed_out_is_current = bridge.fail_activation("session-1", timed_out).await;
        assert!(submission_failure_events(timed_out_is_current, "backend unavailable").is_empty());
        assert!(cleanup.try_recv().is_err());
        assert!(bridge.is_active("session-1").await);
        assert!(bridge.fail_activation("session-1", current).await);
    }

    #[tokio::test]
    async fn current_ready_timeout_emits_one_error_terminal_sequence() {
        let bridge = ThreadEventsBridge::new();
        let mut cleanup = bridge.take_terminal_unsubscribe_requests();
        bridge.mark_recovering();
        let activation = bridge.activate("session-1").await;
        let error = bridge
            .wait_ready_for(Duration::from_millis(1))
            .await
            .unwrap_err();
        let is_current = bridge.fail_activation("session-1", activation).await;
        let projected = submission_failure_events(is_current, error);

        assert!(matches!(
            projected.as_slice(),
            [
                ChatStreamEvent::Error { .. },
                ChatStreamEvent::RunFinished { outcome_type, .. },
                ChatStreamEvent::Done
            ] if outcome_type == "error"
        ));
        assert!(!bridge.is_active("session-1").await);
        assert_eq!(
            cleanup
                .recv()
                .await
                .expect("ready timeout cleanup")
                .activation,
            activation
        );
    }

    #[tokio::test]
    async fn failed_resume_cannot_publish_connection_as_ready() {
        let bridge = ThreadEventsBridge::new();
        bridge.mark_recovering();
        assert!(bridge
            .complete_recovery(Err("resume failed".to_string()))
            .is_err());
        assert!(!bridge.is_ready());
        bridge.complete_recovery(Ok(())).unwrap();
        assert!(bridge.is_ready());
    }

    #[test]
    fn emitted_snapshot_keeps_turn_items_and_error_shape() {
        let snapshot = proto::ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "errored".into(),
            turns: vec![proto::ThreadTurn {
                id: "turn-1".into(),
                status: "failed".into(),
                items: vec![proto::ThreadItem {
                    id: "tool-1".into(),
                    item_type: "command_execution".into(),
                    status: "failed".into(),
                    payload_json: r#"{"type":"command_execution"}"#.into(),
                }],
                last_agent_message: String::new(),
                error: Some(proto::ThreadError {
                    message: "boom".into(),
                    error_type: "provider".into(),
                }),
                has_error: true,
            }],
            active_turn: None,
            has_active_turn: false,
        };
        let value = serde_json::to_value(snapshot_dto(&snapshot)).unwrap();
        assert_eq!(value["turns"][0]["items"][0]["id"], "tool-1");
        assert_eq!(value["turns"][0]["error"]["errorType"], "provider");
    }

    #[test]
    fn reconnect_backoff_starts_at_500ms_and_caps_at_15s() {
        let mut backoff = RetryBackoff::default();
        assert_eq!(
            backoff.after_attempt(false),
            std::time::Duration::from_millis(500)
        );
        assert_eq!(
            backoff.after_attempt(false),
            std::time::Duration::from_secs(1)
        );
        assert_eq!(
            backoff.after_attempt(false),
            std::time::Duration::from_secs(2)
        );
        for _ in 0..10 {
            backoff.after_attempt(false);
        }
        assert_eq!(
            backoff.after_attempt(false),
            std::time::Duration::from_secs(15)
        );
        assert_eq!(
            backoff.after_attempt(true),
            std::time::Duration::from_millis(500)
        );
        assert_eq!(
            backoff.after_attempt(false),
            std::time::Duration::from_secs(1)
        );
    }

    #[test]
    fn not_submitted_response_is_an_error() {
        let response = proto::SubmitTurnResponse {
            submission_id: "submission-1".into(),
            turn_id: String::new(),
            disposition: "not_submitted".into(),
            reason: "thread is busy".into(),
        };
        assert_eq!(accepted_turn_id(response).unwrap_err(), "thread is busy");
    }

    #[test]
    fn started_response_returns_turn_id() {
        let response = proto::SubmitTurnResponse {
            submission_id: "submission-1".into(),
            turn_id: "turn-1".into(),
            disposition: "started".into(),
            reason: String::new(),
        };
        assert_eq!(accepted_turn_id(response).unwrap(), "turn-1");
    }

    #[test]
    fn stale_submission_failure_has_no_terminal_projection() {
        assert!(submission_failure_events(false, "old failure").is_empty());
        assert!(matches!(
            submission_failure_events(true, "current failure").as_slice(),
            [
                ChatStreamEvent::Error { .. },
                ChatStreamEvent::RunFinished { outcome_type, .. },
                ChatStreamEvent::Done
            ] if outcome_type == "error"
        ));
    }
}
