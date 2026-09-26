//! One long-lived Codex-style Thread event connection for the desktop shell.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use agent_protocol::TurnItem;
use proto::astro_service_client::AstroServiceClient;
use serde::{Deserialize, Serialize};
use server::WORKSPACE_EVENT_THREAD_ID;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, oneshot, watch, Mutex, Notify, RwLock};
use tracing::debug;

use super::grpc::{default_grpc_address, endpoint_url};
use crate::commands::chat::{
    ChatStreamEvent, ContextUsageItemDto, ContextUsageSegmentDto, MediaAssetDto,
};

const SNAPSHOT_EVENT: &str = "thread_snapshot";
const SESSION_EVENT: &str = "session_event";
const SESSION_STATUS_CHANGED_EVENT: &str = "session_status_changed";
const DESKTOP_PET_ACTIVITY_CHANGED_EVENT: &str = "desktop_pet_activity_changed";
const REALTIME_CONVERSATION_EVENT: &str = "realtime_conversation_event";
pub(crate) const THREAD_EVENTS_READY_TIMEOUT: Duration = Duration::from_secs(15);
const PROVISIONAL_EVENT_BUFFER_CAPACITY: usize = 128;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionStatusChangedDto {
    pub session_id: String,
    /// `idle` | `active` | `systemError`
    pub status: String,
    /// Active-only flags: `waitingOnApproval` | `waitingOnUserInput`.
    pub active_flags: Vec<String>,
    pub error: Option<String>,
    pub ts_ms: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RealtimeConversationEventDto {
    session_id: String,
    kind: String,
    payload: serde_json::Value,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopPetActivityChangedDto {
    session_id: String,
    state: String,
    ts_ms: i64,
}

fn session_status_registry() -> &'static std::sync::Mutex<HashMap<String, SessionStatusChangedDto>>
{
    static REGISTRY: OnceLock<std::sync::Mutex<HashMap<String, SessionStatusChangedDto>>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn next_session_status_ts_ms() -> i64 {
    static LAST_TS_MS: AtomicI64 = AtomicI64::new(0);
    let now = now_ts_ms();
    let mut previous = LAST_TS_MS.load(Ordering::Relaxed);
    loop {
        let next = now.max(previous.saturating_add(1));
        match LAST_TS_MS.compare_exchange_weak(previous, next, Ordering::SeqCst, Ordering::Relaxed)
        {
            Ok(_) => return next,
            Err(current) => previous = current,
        }
    }
}

pub(crate) fn session_status_snapshot() -> Vec<SessionStatusChangedDto> {
    let mut statuses = session_status_registry()
        .lock()
        .map(|statuses| statuses.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    statuses.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    statuses
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionEventDto {
    pub session_id: Option<String>,
    pub agent_id: String,
    pub ts_ms: i64,
    pub memory_updated: Option<MemoryUpdatedDto>,
    pub pending_changed: Option<PendingChangedDto>,
    pub session_metadata_changed: Option<SessionMetadataChangedDto>,
    pub agent_thread_changed: Option<AgentThreadChangedDto>,
    pub resync_required: Option<SessionResyncRequiredDto>,
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

/// Codex V2 Agent Thread activity with the complete current projection expected
/// by the existing Desktop Agent Tree listener.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentThreadChangedDto {
    activity_sequence: u64,
    root_thread_id: String,
    thread_id: String,
    parent_thread_id: String,
    canonical_path: String,
    task_name: String,
    agent_type: String,
    session_id: String,
    status_kind: String,
    status_payload_json: String,
    activity_kind: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionResyncRequiredDto {
    reason: String,
}

/// Session-event projection for durable Thread extensions. Local commands keep
/// using the smaller [`SessionEventDto`], while backend-derived events retain
/// the replay/generation fields consumed by the V2 Agent Tree listener.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionSessionEventDto {
    session_id: Option<String>,
    agent_id: String,
    ts_ms: i64,
    event_id: u64,
    stream_id: String,
    memory_updated: Option<MemoryUpdatedDto>,
    pending_changed: Option<PendingChangedDto>,
    session_metadata_changed: Option<SessionMetadataChangedDto>,
    agent_thread_changed: Option<AgentThreadChangedDto>,
    resync_required: Option<SessionResyncRequiredDto>,
}

pub(crate) fn now_ts_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub(crate) fn emit_session_event<T>(app: &AppHandle, event: T)
where
    T: Serialize + Clone,
{
    let _ = app.emit(SESSION_EVENT, event);
}

pub(crate) fn emit_session_status(
    app: &AppHandle,
    session_id: impl Into<String>,
    status: &str,
    active_flags: Vec<String>,
    error: Option<String>,
) {
    let changed = SessionStatusChangedDto {
        session_id: session_id.into(),
        status: status.into(),
        active_flags,
        error,
        ts_ms: next_session_status_ts_ms(),
    };
    if let Ok(mut statuses) = session_status_registry().lock() {
        statuses.insert(changed.session_id.clone(), changed.clone());
    }
    let _ = app.emit(SESSION_STATUS_CHANGED_EVENT, changed);
}

#[derive(Default)]
struct ActiveState {
    threads: HashSet<String>,
    background_pending: HashMap<String, HashSet<String>>,
    background_subscriptions: HashMap<String, u64>,
    completed_background_turns: HashMap<String, HashSet<String>>,
    turn_epochs: HashMap<String, HashMap<String, u64>>,
    activations: HashMap<String, u64>,
    awaiting_submissions: HashMap<String, HashSet<u64>>,
    deferred_terminals: HashMap<String, HashMap<String, DeferredTerminal>>,
    provisional_nonterminal_events: HashMap<String, HashMap<(String, u64), Vec<ChatStreamEvent>>>,
    provisional_delivery_pending: HashSet<(String, u64)>,
    delivered_agent_text: HashMap<String, HashMap<String, String>>,
    delivered_reasoning: HashMap<String, HashMap<String, String>>,
    delivered_async_messages: HashMap<String, HashSet<String>>,
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
    provisional_delivery_notify: Notify,
    terminal_cleanup: TerminalSubscriptionCleanup,
}

pub(crate) type ManagedThreadEventsBridge = Arc<ThreadEventsBridge>;

pub(crate) fn managed_bridge(app: &AppHandle) -> tauri::State<'_, ManagedThreadEventsBridge> {
    app.state::<ManagedThreadEventsBridge>()
}

#[cfg(test)]
fn managed_bridge_state_type_id() -> std::any::TypeId {
    std::any::TypeId::of::<ManagedThreadEventsBridge>()
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
            provisional_delivery_notify: Notify::new(),
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
        state.provisional_nonterminal_events.remove(&thread_id);
        state
            .provisional_delivery_pending
            .retain(|(pending_thread_id, _)| pending_thread_id != &thread_id);
        state
            .awaiting_submissions
            .entry(thread_id.clone())
            .or_default()
            .insert(activation);
        state.activations.insert(thread_id.clone(), activation);
        drop(state);
        self.provisional_delivery_notify.notify_waiters();
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
            drop(state);
            self.provisional_delivery_notify.notify_waiters();
            return false;
        }
        Self::clear_thread(&mut state, thread_id);
        drop(state);
        self.provisional_delivery_notify.notify_waiters();
        // Keep terminal_cleanup.owners intact until the queued RPC succeeds. A newer activation
        // overwrites the owner first, making this request stale without unsubscribing the new turn.
        self.queue_terminal_unsubscribe(thread_id, activation);
        true
    }

    /// Forget every local recovery target for an explicitly released session. The current
    /// generation remains the cleanup owner until its Unsubscribe RPC succeeds; a reused thread
    /// id overwrites that owner under the same gate, making the queued cleanup safely stale.
    #[cfg(test)]
    pub(crate) async fn forget_thread(&self, thread_id: &str) {
        let gate = self.terminal_cleanup.gate(thread_id);
        let _owner_guard = gate.lock().await;
        self.forget_thread_locked(thread_id).await;
    }

    async fn forget_thread_locked(&self, thread_id: &str) {
        let mut state = self.active_threads.write().await;
        Self::clear_thread(&mut state, thread_id);
        state.background_pending.remove(thread_id);
        state.background_subscriptions.remove(thread_id);
        state.completed_background_turns.remove(thread_id);
        state
            .delivered_extensions
            .retain(|(delivered_thread_id, _), _| delivered_thread_id != thread_id);
        drop(state);
        self.provisional_delivery_notify.notify_waiters();
        if let Some(owner) = self
            .terminal_cleanup
            .owners
            .read()
            .await
            .get(thread_id)
            .copied()
        {
            self.queue_terminal_unsubscribe(thread_id, owner);
        }
    }

    /// Linearize an explicit lifecycle RPC with activation of a reused logical thread id.
    /// `activate` uses the same per-thread gate, so no replacement can exist until the backend
    /// release boundary returns (successfully or otherwise).
    pub(crate) async fn forget_thread_through<F, Fut, T>(&self, thread_id: &str, operation: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let gate = self.terminal_cleanup.gate(thread_id);
        let _owner_guard = gate.lock().await;
        self.forget_thread_locked(thread_id).await;
        operation().await
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
            drop(state);
            self.provisional_delivery_notify.notify_waiters();
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
            drop(state);
            self.provisional_delivery_notify.notify_waiters();
            return Vec::new();
        }
        let buffered_events =
            Self::take_provisional_nonterminal_events(&mut state, thread_id, turn_id, activation);
        let mut superseded_turns = state
            .turn_epochs
            .get(thread_id)
            .into_iter()
            .flat_map(|turns| turns.iter())
            .filter(|(provisional_turn_id, epoch)| {
                **epoch == activation && provisional_turn_id.as_str() != turn_id
            })
            .map(|(provisional_turn_id, _)| provisional_turn_id.clone())
            .collect::<HashSet<_>>();
        Self::remove_turn_epoch(&mut state, thread_id, activation);
        state
            .turn_epochs
            .entry(thread_id.into())
            .or_default()
            .insert(turn_id.into(), activation);

        let deferred = state
            .deferred_terminals
            .remove(thread_id)
            .and_then(|mut terminals| {
                let matching = terminals.remove(turn_id);
                superseded_turns.extend(terminals.into_keys());
                matching
            })
            .filter(|terminal| terminal.awaiting_activation == activation);
        for superseded_turn_id in superseded_turns {
            Self::take_completed_background_turn(&mut state, thread_id, &superseded_turn_id);
        }
        Self::record_projection_state(&mut state, thread_id, turn_id, &buffered_events);
        let Some(deferred) = deferred else {
            if buffered_events.is_empty() {
                drop(state);
                self.provisional_delivery_notify.notify_waiters();
            } else {
                state
                    .provisional_delivery_pending
                    .insert((thread_id.into(), activation));
            }
            return buffered_events;
        };
        let terminal_events =
            Self::dedup_terminal_projection_state(&mut state, thread_id, turn_id, deferred.events);
        let background_completed =
            Self::take_completed_background_turn(&mut state, thread_id, turn_id);
        if deferred.background_turn && !background_completed {
            state
                .background_pending
                .entry(thread_id.into())
                .or_default()
                .insert(turn_id.into());
        }
        Self::clear_thread(&mut state, thread_id);
        drop(state);
        self.provisional_delivery_notify.notify_waiters();
        self.queue_terminal_unsubscribe(thread_id, activation);
        let mut released = buffered_events;
        released.extend(terminal_events);
        released
    }

    /// Release live projection after the command has emitted an ACK-drained provisional batch.
    /// This keeps the event pump behind the batch until the UI observes it in FIFO order.
    async fn finish_provisional_delivery(&self, thread_id: &str, activation: u64) {
        let mut state = self.active_threads.write().await;
        state
            .provisional_delivery_pending
            .remove(&(thread_id.into(), activation));
        drop(state);
        self.provisional_delivery_notify.notify_waiters();
    }

    /// Own barrier cleanup independently from the SubmitTurn caller. Delivery acceptance releases
    /// the barrier after synchronous UI emission; dropping the sender (including task abort) also
    /// releases it so the global event pump cannot remain head-of-line blocked.
    pub(crate) fn spawn_provisional_delivery_cleanup(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        activation: u64,
    ) -> oneshot::Sender<()> {
        let thread_id = thread_id.into();
        let bridge = Arc::clone(self);
        let (accepted_tx, accepted_rx) = oneshot::channel();
        tauri::async_runtime::spawn(async move {
            let _ = accepted_rx.await;
            bridge
                .finish_provisional_delivery(&thread_id, activation)
                .await;
        });
        accepted_tx
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
            let can_precede_terminal = state
                .deferred_terminals
                .get(thread_id)
                .is_some_and(|terminals| terminals.contains_key(turn_id))
                || state
                    .turn_epochs
                    .get(thread_id)
                    .is_some_and(|turns| turns.contains_key(turn_id))
                || state.activations.get(thread_id).is_some_and(|activation| {
                    state
                        .awaiting_submissions
                        .get(thread_id)
                        .is_some_and(|pending| pending.contains(activation))
                });
            if can_precede_terminal {
                state
                    .completed_background_turns
                    .entry(thread_id.into())
                    .or_default()
                    .insert(turn_id.into());
            }
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

    /// Converge local recovery intent with the authoritative server sink set returned by Resume.
    /// The server set is subtractive: turns unknown to this desktop are not adopted, while local
    /// turns whose sink expired or was explicitly released are retired generation-safely.
    async fn reconcile_background_snapshot(&self, snapshot: &proto::ThreadSnapshot) -> bool {
        let thread_id = snapshot.thread_id.as_str();
        let server_pending = snapshot
            .pending_background_turn_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let mut state = self.active_threads.write().await;
        let Some(pending) = state.background_pending.get_mut(thread_id) else {
            return false;
        };
        let before = pending.len();
        pending.retain(|turn_id| server_pending.contains(turn_id.as_str()));
        if pending.len() == before {
            return false;
        }
        if !pending.is_empty() {
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

    fn take_completed_background_turn(
        state: &mut ActiveState,
        thread_id: &str,
        turn_id: &str,
    ) -> bool {
        let completed = state
            .completed_background_turns
            .get_mut(thread_id)
            .is_some_and(|turns| turns.remove(turn_id));
        if state
            .completed_background_turns
            .get(thread_id)
            .is_some_and(HashSet::is_empty)
        {
            state.completed_background_turns.remove(thread_id);
        }
        completed
    }

    async fn record_delivered_projection(
        &self,
        thread_id: &str,
        turn_id: &str,
        events: &[ChatStreamEvent],
    ) -> bool {
        if turn_id.is_empty() {
            return false;
        }
        loop {
            // Register the waiter before inspecting state so an ACK/cleanup notification cannot
            // race between releasing the state lock and awaiting capacity.
            let notified = self.provisional_delivery_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let mut state = self.active_threads.write().await;
            let current_activation = state.activations.get(thread_id).copied();
            let turn_activation = state
                .turn_epochs
                .get(thread_id)
                .and_then(|turns| turns.get(turn_id))
                .copied();
            if !state.threads.contains(thread_id) || current_activation.is_none() {
                return false;
            }
            let activation = current_activation.expect("validated current activation");
            if state
                .provisional_delivery_pending
                .contains(&(thread_id.into(), activation))
            {
                drop(state);
                notified.await;
                continue;
            }
            let pending = state
                .awaiting_submissions
                .get(thread_id)
                .is_some_and(|pending| pending.contains(&activation));
            if pending {
                let buffer = state
                    .provisional_nonterminal_events
                    .entry(thread_id.into())
                    .or_default()
                    .entry((turn_id.into(), activation))
                    .or_default();
                if buffer.len().saturating_add(events.len()) > PROVISIONAL_EVENT_BUFFER_CAPACITY {
                    drop(state);
                    notified.await;
                    continue;
                }
                buffer.extend(events.iter().cloned());
                return false;
            }
            if turn_activation != Some(activation) {
                return false;
            }
            Self::record_projection_state(&mut state, thread_id, turn_id, events);
            return true;
        }
    }

    fn record_projection_state(
        state: &mut ActiveState,
        thread_id: &str,
        turn_id: &str,
        events: &[ChatStreamEvent],
    ) {
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
                ChatStreamEvent::AsyncMessage { id, .. } => {
                    state
                        .delivered_async_messages
                        .entry(thread_id.into())
                        .or_default()
                        .insert(id.clone());
                }
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
                ChatStreamEvent::AsyncMessage {
                    id,
                    content,
                    questions,
                } => {
                    if state
                        .delivered_async_messages
                        .entry(thread_id.into())
                        .or_default()
                        .insert(id.clone())
                    {
                        recovered.push(ChatStreamEvent::AsyncMessage {
                            id,
                            content,
                            questions,
                        });
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

    async fn observe_extension(
        &self,
        thread_id: &str,
        turn_id: &str,
        extension: &proto::ThreadExtension,
    ) -> bool {
        // Lifecycle state is authoritative and must advance even when projection delivery is
        // deduplicated (for example, a marker seen live before the deferred terminal ACK).
        if matches!(
            extension.namespace.as_str(),
            "astro.background_complete" | "astro.background_expired"
        ) {
            self.complete_background_turn(thread_id, turn_id).await;
        }
        self.accept_extension(thread_id, &extension.item_id, &extension.payload_json)
            .await
    }

    async fn dedup_terminal_projection(
        &self,
        thread_id: &str,
        turn_id: &str,
        events: Vec<ChatStreamEvent>,
    ) -> Vec<ChatStreamEvent> {
        let mut state = self.active_threads.write().await;
        Self::dedup_terminal_projection_state(&mut state, thread_id, turn_id, events)
    }

    fn dedup_terminal_projection_state(
        state: &mut ActiveState,
        thread_id: &str,
        turn_id: &str,
        events: Vec<ChatStreamEvent>,
    ) -> Vec<ChatStreamEvent> {
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
        let mut state = loop {
            let notified = self.provisional_delivery_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let state = self.active_threads.write().await;
            let delivery_pending = state.activations.get(thread_id).is_some_and(|activation| {
                state
                    .provisional_delivery_pending
                    .contains(&(thread_id.into(), *activation))
            });
            if !delivery_pending {
                break state;
            }
            drop(state);
            notified.await;
        };
        if state
            .deferred_terminals
            .get(thread_id)
            .is_some_and(|terminals| terminals.contains_key(turn_id))
        {
            return Vec::new();
        }
        let turn_epoch = state
            .turn_epochs
            .get(thread_id)
            .and_then(|turns| turns.get(turn_id))
            .copied();
        let Some(turn_epoch) = turn_epoch else {
            let current_activation = state.activations.get(thread_id).copied();
            let currently_pending = current_activation.is_some_and(|activation| {
                state
                    .awaiting_submissions
                    .get(thread_id)
                    .is_some_and(|pending| pending.contains(&activation))
            });
            if let Some(awaiting_activation) = current_activation.filter(|_| currently_pending) {
                let background_completed =
                    Self::take_completed_background_turn(&mut state, thread_id, turn_id);
                state
                    .deferred_terminals
                    .entry(thread_id.into())
                    .or_default()
                    .insert(
                        turn_id.into(),
                        DeferredTerminal {
                            awaiting_activation,
                            background_turn: background_turn && !background_completed,
                            events,
                        },
                    );
            }
            return Vec::new();
        };
        let current_activation = state.activations.get(thread_id).copied();
        let current_is_pending = current_activation.is_some_and(|activation| {
            state
                .awaiting_submissions
                .get(thread_id)
                .is_some_and(|pending| pending.contains(&activation))
        });
        if current_activation == Some(turn_epoch) && current_is_pending {
            let background_completed =
                Self::take_completed_background_turn(&mut state, thread_id, turn_id);
            state
                .deferred_terminals
                .entry(thread_id.into())
                .or_default()
                .insert(
                    turn_id.into(),
                    DeferredTerminal {
                        awaiting_activation: turn_epoch,
                        background_turn: background_turn && !background_completed,
                        events,
                    },
                );
            return Vec::new();
        }
        if current_activation == Some(turn_epoch) {
            let background_completed =
                Self::take_completed_background_turn(&mut state, thread_id, turn_id);
            if background_turn && !background_completed {
                state
                    .background_pending
                    .entry(thread_id.into())
                    .or_default()
                    .insert(turn_id.into());
            }
            Self::clear_thread(&mut state, thread_id);
            drop(state);
            self.provisional_delivery_notify.notify_waiters();
            self.queue_terminal_unsubscribe(thread_id, turn_epoch);
            return events;
        }

        if let Some(current_activation) = current_activation.filter(|_| current_is_pending) {
            let background_completed =
                Self::take_completed_background_turn(&mut state, thread_id, turn_id);
            let background_turn = background_turn && !background_completed;
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
        state.provisional_nonterminal_events.remove(thread_id);
        state
            .provisional_delivery_pending
            .retain(|(pending_thread_id, _)| pending_thread_id != thread_id);
        state.completed_background_turns.remove(thread_id);
        state.delivered_agent_text.remove(thread_id);
        state.delivered_reasoning.remove(thread_id);
        state.delivered_async_messages.remove(thread_id);
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
        Self::remove_provisional_nonterminal_activation(state, thread_id, activation);
        if let Some(terminals) = state.deferred_terminals.get_mut(thread_id) {
            terminals.retain(|_, terminal| terminal.awaiting_activation != activation);
            if terminals.is_empty() {
                state.deferred_terminals.remove(thread_id);
            }
        }
    }

    fn take_provisional_nonterminal_events(
        state: &mut ActiveState,
        thread_id: &str,
        turn_id: &str,
        activation: u64,
    ) -> Vec<ChatStreamEvent> {
        let Some(mut buffers) = state.provisional_nonterminal_events.remove(thread_id) else {
            return Vec::new();
        };
        let matching = buffers
            .remove(&(turn_id.into(), activation))
            .unwrap_or_default();
        buffers.retain(|(_, buffered_activation), _| *buffered_activation != activation);
        if !buffers.is_empty() {
            state
                .provisional_nonterminal_events
                .insert(thread_id.into(), buffers);
        }
        matching
    }

    fn remove_provisional_nonterminal_activation(
        state: &mut ActiveState,
        thread_id: &str,
        activation: u64,
    ) {
        if let Some(buffers) = state.provisional_nonterminal_events.get_mut(thread_id) {
            buffers.retain(|(_, buffered_activation), _| *buffered_activation != activation);
            if buffers.is_empty() {
                state.provisional_nonterminal_events.remove(thread_id);
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
    let bridge: ManagedThreadEventsBridge = Arc::new(ThreadEventsBridge::new());
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
                if reconciled.keep_active && reconciled.active_turn_id.is_some() {
                    emit_session_status(app, &thread_id, "active", Vec::new(), None);
                } else if let Some(turn_id) = reconciled.terminal_turn_id.as_deref() {
                    let terminal_turn = snapshot.turns.iter().find(|turn| turn.id == turn_id);
                    let failed = terminal_turn.is_some_and(|turn| turn.status == "failed");
                    emit_session_status(
                        app,
                        &thread_id,
                        if failed { "systemError" } else { "idle" },
                        Vec::new(),
                        terminal_turn
                            .and_then(|turn| turn.error.as_ref())
                            .map(|error| error.message.clone()),
                    );
                }
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
                    let extension = recovered.extension;
                    if let Some(session_event) =
                        extension_to_session_event(&thread_id, extension.clone())
                    {
                        emit_session_event(app, session_event);
                    }
                    emit_chat_events(app, &thread_id, map_extension_to_chat(extension));
                }
                bridge.reconcile_background_snapshot(&snapshot).await;
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
    if let Some(proto::thread_event::Payload::Realtime(realtime)) = event.payload.as_ref() {
        let payload = serde_json::from_str(&realtime.payload_json).unwrap_or_else(
            |error| serde_json::json!({ "serialization_error": error.to_string() }),
        );
        let _ = app.emit(
            REALTIME_CONVERSATION_EVENT,
            RealtimeConversationEventDto {
                session_id: thread_id,
                kind: realtime.kind.clone(),
                payload,
            },
        );
        return;
    }
    emit_status_for_thread_event(app, &thread_id, event.payload.as_ref());
    let is_extension = matches!(
        event.payload.as_ref(),
        Some(proto::thread_event::Payload::Extension(_))
    );
    if let Some(proto::thread_event::Payload::TurnStarted(started)) = event.payload.as_ref() {
        bridge
            .bind_observed_turn(&thread_id, &started.turn_id)
            .await;
    }
    if let Some(proto::thread_event::Payload::Extension(extension)) = event.payload.as_ref() {
        if !bridge
            .observe_extension(&thread_id, &turn_id, extension)
            .await
        {
            return;
        }
        if let Some(session_event) = extension_to_session_event(&thread_id, extension.clone()) {
            emit_session_event(app, session_event);
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
    } else if is_extension
        || bridge
            .record_delivered_projection(&thread_id, &turn_id, &events)
            .await
    {
        events
    } else {
        Vec::new()
    };
    emit_live_desktop_pet_events(app, &thread_id, &events);
    crate::infra::notify::emit_live_task_notices(app, &events);
    emit_chat_events(app, &thread_id, events);
}

fn emit_status_for_thread_event(
    app: &AppHandle,
    thread_id: &str,
    payload: Option<&proto::thread_event::Payload>,
) {
    use proto::thread_event::Payload;

    match payload {
        Some(Payload::TurnStarted(_)) => {
            emit_session_status(app, thread_id, "active", Vec::new(), None);
        }
        Some(Payload::ControlRequest(control))
            if matches!(
                control.kind.as_str(),
                "exec_approval"
                    | "apply_patch_approval"
                    | "request_permissions"
                    | "request_user_input"
                    | "elicitation"
            ) =>
        {
            let flag = if control.kind == "request_user_input" || control.kind == "elicitation" {
                "waitingOnUserInput"
            } else {
                "waitingOnApproval"
            };
            emit_session_status(app, thread_id, "active", vec![flag.into()], None);
        }
        Some(Payload::TurnComplete(complete)) => {
            emit_session_status(
                app,
                thread_id,
                if complete.has_error {
                    "systemError"
                } else {
                    "idle"
                },
                Vec::new(),
                complete.error.as_ref().map(|error| error.message.clone()),
            );
        }
        Some(Payload::TurnAborted(_)) => {
            emit_session_status(app, thread_id, "idle", Vec::new(), None);
        }
        _ => {}
    }
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

// History/snapshot replay uses emit_chat_events too, but must never celebrate an
// old completion or restore an obsolete approval pose. Only accepted live events
// reach this projection, using the status clock to order the two channels.
fn emit_live_desktop_pet_events(app: &AppHandle, thread_id: &str, events: &[ChatStreamEvent]) {
    for event in events {
        if let Some(state) = desktop_pet_activity_for_event(event) {
            let _ = app.emit(
                DESKTOP_PET_ACTIVITY_CHANGED_EVENT,
                DesktopPetActivityChangedDto {
                    session_id: thread_id.to_string(),
                    state: state.to_string(),
                    ts_ms: next_session_status_ts_ms(),
                },
            );
        }
    }
}

fn desktop_pet_activity_for_event(event: &ChatStreamEvent) -> Option<&'static str> {
    match event {
        ChatStreamEvent::ToolCall { name, phase, .. } if phase == "started" => {
            let name = name.to_ascii_lowercase();
            (name.contains("review") || name.contains("inspect") || name.contains("audit"))
                .then_some("review")
        }
        ChatStreamEvent::RunFinished { outcome_type, .. } => match outcome_type.as_str() {
            "success" => Some("jumping"),
            "error" => Some("failed"),
            "hitl_waiting" => Some("waiting"),
            "interrupt" => Some("idle"),
            _ => None,
        },
        _ => None,
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
        Some(Payload::PlanDelta(delta))
        | Some(Payload::ExecOutputDelta(delta))
        | Some(Payload::PatchDelta(delta)) => map_output_delta(delta),
        Some(Payload::ControlRequest(control)) => map_control_request(turn_id, control),
        Some(Payload::TokenCount(tokens)) => vec![ChatStreamEvent::Usage {
            prompt_tokens: (if tokens.input_tokens_include_cache {
                tokens.input_tokens
            } else {
                tokens
                    .input_tokens
                    .saturating_add(tokens.cache_read_tokens)
                    .saturating_add(tokens.cache_write_tokens)
            })
            .min(u32::MAX.into()) as u32,
            uncached_input_tokens: (if tokens.input_tokens_include_cache {
                tokens.uncached_input_tokens
            } else {
                tokens.input_tokens
            })
            .min(u32::MAX.into()) as u32,
            completion_tokens: tokens.output_tokens.min(u32::MAX.into()) as u32,
            total_tokens: tokens.total_tokens.min(u32::MAX.into()) as u32,
            cache_read_tokens: tokens.cache_read_tokens.min(u32::MAX.into()) as u32,
            cache_write_tokens: tokens.cache_write_tokens.min(u32::MAX.into()) as u32,
            reasoning_tokens: tokens.reasoning_tokens.min(u32::MAX.into()) as u32,
            request_count: tokens.request_count.min(u32::MAX.into()) as u32,
            provider_total_tokens: tokens
                .provider_total_tokens_reported
                .then_some(tokens.provider_total_tokens.min(u32::MAX.into()) as u32),
            cache_read_reported: tokens.cache_read_reported,
            cache_write_reported: tokens.cache_write_reported,
            reasoning_reported: tokens.reasoning_reported,
        }],
        Some(Payload::Error(error)) => vec![ChatStreamEvent::Error {
            message: error.message,
        }],
        Some(Payload::Warning(warning)) => {
            if warning.error_type == "reconnecting" {
                vec![ChatStreamEvent::StreamError {
                    message: warning.message,
                    error_type: warning.error_type,
                }]
            } else {
                vec![activity(
                    turn_id,
                    "warning",
                    serde_json::json!({"message":warning.message,"error_type":warning.error_type})
                        .to_string(),
                )]
            }
        }
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
        Some(Payload::Realtime(_)) => Vec::new(),
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

fn map_output_delta(delta: proto::ThreadDelta) -> Vec<ChatStreamEvent> {
    vec![ChatStreamEvent::ToolOutputDelta {
        id: delta.item_id,
        delta: delta.delta,
    }]
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
    let elicitation = (control.kind == "elicitation")
        .then(|| payload.get("request"))
        .flatten();
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
            "message": elicitation.and_then(|request| request.get("message")).and_then(serde_json::Value::as_str).or_else(|| payload.get("message").and_then(serde_json::Value::as_str)).unwrap_or_default(),
            "tool_call_id": control.item_id,
            "response_schema_json": elicitation.and_then(|request| request.get("requestedSchema")).cloned().or_else(|| payload.get("response_schema").cloned()).unwrap_or_default().to_string(),
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
            web_action: tool.web_action,
            web_page_title: tool.web_page_title,
            phase: if started {
                "started"
            } else {
                match tool.status {
                    agent_protocol::ToolStatus::InProgress => "started",
                    agent_protocol::ToolStatus::Completed => "completed",
                    agent_protocol::ToolStatus::Failed => "failed",
                    agent_protocol::ToolStatus::Declined => "declined",
                    agent_protocol::ToolStatus::Interrupted => "interrupted",
                }
            }
            .into(),
            batch_id: tool.batch_id,
            execution_mode: tool.execution_mode.map(|mode| match mode {
                agent_protocol::ToolExecutionMode::Serial => "serial".into(),
                agent_protocol::ToolExecutionMode::Parallel => "parallel".into(),
            }),
            media: tool.media.into_iter().map(media_asset_dto).collect(),
            file_changes: tool.file_changes,
        }],
        Ok(TurnItem::AgentMessage(message))
            if !started
                && message.delivery == Some(agent_protocol::AgentMessageDelivery::Async) =>
        {
            vec![ChatStreamEvent::AsyncMessage {
                id: message.id,
                content: message.content,
                questions: message.questions,
            }]
        }
        Ok(TurnItem::AgentMessage(message)) if started && message.delivery.is_none() => {
            vec![ChatStreamEvent::Token {
                content: message.content,
            }]
        }
        Ok(TurnItem::Reasoning(text)) if started => {
            vec![ChatStreamEvent::Reasoning {
                content: text.content,
            }]
        }
        Ok(TurnItem::AgentMessage(_)) | Ok(TurnItem::Reasoning(_)) => Vec::new(),
        Ok(TurnItem::Plan(text)) => vec![ChatStreamEvent::ToolCall {
            id: text.id,
            name: "plan".into(),
            arguments_json: String::new(),
            result: text.content,
            web_action: None,
            web_page_title: None,
            phase: if started { "started" } else { "completed" }.into(),
            batch_id: None,
            execution_mode: None,
            media: Vec::new(),
            file_changes: Vec::new(),
        }],
        Ok(TurnItem::HookPrompt(prompt)) => vec![ChatStreamEvent::Hook {
            name: "hook_prompt".into(),
            detail: prompt
                .fragments
                .into_iter()
                .map(|fragment| fragment.text)
                .collect::<Vec<_>>()
                .join("\n\n"),
            outcome: if started { "started" } else { "completed" }.into(),
        }],
        Ok(TurnItem::Extension(extension)) if extension.namespace == "astro.memory" => {
            match serde_json::from_value::<MemoryExtensionPayload>(extension.payload) {
                Ok(payload) => vec![memory_update_from_payload(payload)],
                Err(error) => vec![ChatStreamEvent::Error {
                    message: format!("invalid memory update payload: {error}"),
                }],
            }
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
        | "astro.background_expired"
        | "astro.pending"
        | "astro.session_metadata"
        | "astro.agent_thread"
        | "astro.agent_thread_resync" => Vec::new(),
        "astro.context_usage" => vec![context_usage_event(&extension.payload_json)],
        "astro.user_input_committed" => {
            let client_message_id =
                serde_json::from_str::<serde_json::Value>(&extension.payload_json)
                    .ok()
                    .and_then(|payload| {
                        payload
                            .get("client_message_id")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_default();
            vec![ChatStreamEvent::UserInputCommitted { client_message_id }]
        }
        "astro.memory" => {
            match serde_json::from_str::<MemoryExtensionPayload>(&extension.payload_json) {
                Ok(payload) => vec![memory_update_from_payload(payload)],
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

#[derive(Deserialize)]
struct MemoryExtensionPayload {
    source: String,
    target: String,
    summary: String,
    live_written: bool,
}

fn memory_update_from_payload(payload: MemoryExtensionPayload) -> ChatStreamEvent {
    ChatStreamEvent::MemoryUpdate {
        operation: payload.source,
        content: payload.summary,
    }
}

fn context_usage_event(payload: &str) -> ChatStreamEvent {
    let value = serde_json::from_str::<serde_json::Value>(payload).unwrap_or_default();
    let total_tokens = json_u32(&value, "total_tokens");
    ChatStreamEvent::ContextUsage {
        context_window: json_u32(&value, "context_window"),
        total_tokens,
        estimated_total_tokens: value
            .get("estimated_total_tokens")
            .and_then(serde_json::Value::as_u64)
            .map(|number| number.min(u64::from(u32::MAX)) as u32)
            .unwrap_or(total_tokens),
        source: value
            .get("source")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("local_estimate")
            .to_string(),
        latest_usage: value.get("latest_usage").cloned(),
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

fn json_u64(value: &serde_json::Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
}

fn required_json_str<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
}

fn extension_to_session_event(
    thread_id: &str,
    extension: proto::ThreadExtension,
) -> Option<ExtensionSessionEventDto> {
    let value = serde_json::from_str::<serde_json::Value>(&extension.payload_json).ok()?;
    let (
        memory_updated,
        pending_changed,
        session_metadata_changed,
        agent_thread_changed,
        resync_required,
        event_id,
        stream_id,
        agent_id,
        session_id,
    ) = match extension.namespace.as_str() {
        "astro.memory" => {
            let payload = serde_json::from_value::<MemoryExtensionPayload>(value).ok()?;
            (
                Some(MemoryUpdatedDto {
                    source: payload.source,
                    target: payload.target,
                    summary: payload.summary,
                    live_written: payload.live_written,
                }),
                None,
                None,
                None,
                None,
                0,
                String::new(),
                String::new(),
                thread_id.to_string(),
            )
        }
        "astro.pending" => (
            None,
            Some(PendingChangedDto {
                pending_count: json_u32(&value, "pending_count"),
                reason: json_str(&value, "reason"),
            }),
            None,
            None,
            None,
            0,
            String::new(),
            String::new(),
            thread_id.to_string(),
        ),
        "astro.session_metadata" => (
            None,
            None,
            Some(SessionMetadataChangedDto {
                title: json_str(&value, "title"),
            }),
            None,
            None,
            0,
            String::new(),
            String::new(),
            thread_id.to_string(),
        ),
        "astro.agent_thread" => {
            let root_thread_id = required_json_str(&value, "root_thread_id")?.to_string();
            let activity_sequence = json_u64(&value, "activity_sequence");
            let stream_id = required_json_str(&value, "stream_id")?.to_string();
            (
                None,
                None,
                None,
                Some(AgentThreadChangedDto {
                    activity_sequence,
                    root_thread_id: root_thread_id.clone(),
                    thread_id: json_str(&value, "thread_id"),
                    parent_thread_id: json_str(&value, "parent_thread_id"),
                    canonical_path: json_str(&value, "canonical_path"),
                    task_name: json_str(&value, "task_name"),
                    agent_type: json_str(&value, "agent_type"),
                    session_id: json_str(&value, "session_id"),
                    status_kind: json_str(&value, "status_kind"),
                    status_payload_json: json_str(&value, "status_payload_json"),
                    activity_kind: json_str(&value, "activity_kind"),
                }),
                None,
                activity_sequence,
                stream_id,
                json_str(&value, "agent_id"),
                root_thread_id,
            )
        }
        "astro.agent_thread_resync" => {
            let root_thread_id = required_json_str(&value, "root_thread_id")?.to_string();
            let stream_id = required_json_str(&value, "stream_id")?.to_string();
            (
                None,
                None,
                None,
                None,
                Some(SessionResyncRequiredDto {
                    reason: json_str(&value, "reason"),
                }),
                0,
                stream_id,
                json_str(&value, "agent_id"),
                root_thread_id,
            )
        }
        _ => return None,
    };
    Some(ExtensionSessionEventDto {
        session_id: Some(session_id),
        agent_id,
        ts_ms: now_ts_ms(),
        event_id,
        stream_id,
        memory_updated,
        pending_changed,
        session_metadata_changed,
        agent_thread_changed,
        resync_required,
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
            Ok(TurnItem::AgentMessage(message))
                if message.delivery == Some(agent_protocol::AgentMessageDelivery::Async) =>
            {
                events.push(ChatStreamEvent::AsyncMessage {
                    id: message.id,
                    content: message.content,
                    questions: message.questions,
                });
                continue;
            }
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
            .observe_extension(&snapshot.thread_id, &recovered.turn_id, extension)
            .await
        {
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
    provider_id: Option<&'a str>,
    backend_id: Option<&'a str>,
    model: Option<&'a str>,
    reasoning_effort: Option<&'a str>,
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
        provider_id: snapshot.provider_id.as_deref(),
        backend_id: snapshot.backend_id.as_deref(),
        model: snapshot.model.as_deref(),
        reasoning_effort: snapshot.reasoning_effort.as_deref(),
        turns: snapshot.turns.iter().map(turn_dto).collect(),
        active_turn: snapshot.active_turn.as_ref().map(turn_dto),
        has_active_turn: snapshot.has_active_turn,
    }
}

fn emit_snapshot(app: &AppHandle, snapshot: &proto::ThreadSnapshot) {
    let _ = app.emit(SNAPSHOT_EVENT, snapshot_dto(snapshot));
}

#[cfg(test)]
#[path = "thread_events_tests.rs"]
mod tests;
