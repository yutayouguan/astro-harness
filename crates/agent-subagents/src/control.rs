use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::Notify;
use uuid::Uuid;

use crate::{
    ActivityBus, ActivityCursor, AgentActivityKind, AgentGraphStore, AgentPath, AgentRegistry,
    AgentStatusV2, AgentThreadV2, AgentTreeSnapshotV2, ExecutionPermit, Limits, MailboxKind,
    MailboxMessage, MessageAgentV2Request, NewMailboxMessage, RunnerEvent, SpawnReservation,
    ThreadReservation,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    MailboxActivity,
    Steered,
    TimedOut,
}

pub type WaitAgentResult = WaitOutcome;

#[cfg(test)]
type BeforeRuntimeInsertHook = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct AgentRuntimeHandle {
    pub interrupt: Arc<dyn Fn() + Send + Sync>,
    pub terminate: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
pub struct RuntimeHandleRegistry {
    handles: Mutex<HashMap<String, AgentRuntimeHandle>>,
}

#[derive(Default)]
struct RuntimeLifecycleState {
    closing_prefixes: BTreeSet<AgentPath>,
    inflight_spawns: HashMap<String, AgentPath>,
}

/// A spawn reservation that remains visible to a concurrent desktop subtree
/// close until it is committed, aborted, or dropped.
pub struct AgentSpawnReservation<'a> {
    inner: Option<SpawnReservation<'a>>,
    control: &'a AgentControl,
    lease_id: String,
}

impl<'a> AgentSpawnReservation<'a> {
    pub fn canonical_path(&self) -> &AgentPath {
        self.inner
            .as_ref()
            .expect("active spawn reservation")
            .canonical_path()
    }

    pub fn thread_id(&self) -> &str {
        self.inner
            .as_ref()
            .expect("active spawn reservation")
            .thread_id()
    }

    pub fn thread(&self) -> &AgentThreadV2 {
        self.inner
            .as_ref()
            .expect("active spawn reservation")
            .thread()
    }

    pub fn abort(mut self) -> anyhow::Result<()> {
        let result = self.inner.take().expect("active spawn reservation").abort();
        self.release_lease();
        result
    }

    pub fn commit(mut self) -> anyhow::Result<()> {
        let result = self
            .inner
            .take()
            .expect("active spawn reservation")
            .commit();
        self.release_lease();
        result
    }

    fn release_lease(&mut self) {
        if !self.lease_id.is_empty() {
            self.control.release_spawn_lease(&self.lease_id);
            self.lease_id.clear();
        }
    }
}

impl Drop for AgentSpawnReservation<'_> {
    fn drop(&mut self) {
        // Roll back the underlying reservation before waking a close waiter.
        drop(self.inner.take());
        self.release_lease();
    }
}

/// Cancel-safe admission barrier for one canonical subtree prefix.
pub struct CloseAdmissionGuard {
    control: Arc<AgentControl>,
    prefix: AgentPath,
    active: bool,
}

impl CloseAdmissionGuard {
    pub async fn wait_for_inflight_spawns(&self) -> anyhow::Result<()> {
        loop {
            let notified = self.control.lifecycle_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.control.has_inflight_spawn_under(&self.prefix)? {
                return Ok(());
            }
            notified.await;
        }
    }
}

impl Drop for CloseAdmissionGuard {
    fn drop(&mut self) {
        if self.active {
            self.control.release_close_prefix(&self.prefix);
            self.active = false;
        }
    }
}

impl RuntimeHandleRegistry {
    pub fn register(&self, thread_id: &str, handle: AgentRuntimeHandle) -> anyhow::Result<()> {
        if thread_id.trim().is_empty() {
            anyhow::bail!("runtime handle thread id must not be empty");
        }
        let mut handles = self
            .handles
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime handle registry mutex is poisoned"))?;
        if handles.contains_key(thread_id) {
            anyhow::bail!("runtime handle already registered for thread {thread_id:?}");
        }
        handles.insert(thread_id.to_string(), handle);
        Ok(())
    }

    pub fn get(&self, thread_id: &str) -> anyhow::Result<Option<AgentRuntimeHandle>> {
        Ok(self
            .handles
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime handle registry mutex is poisoned"))?
            .get(thread_id)
            .cloned())
    }

    pub fn remove(&self, thread_id: &str) -> anyhow::Result<Option<AgentRuntimeHandle>> {
        Ok(self
            .handles
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime handle registry mutex is poisoned"))?
            .remove(thread_id))
    }

    pub fn remove_if_same(
        &self,
        thread_id: &str,
        expected: &AgentRuntimeHandle,
    ) -> anyhow::Result<Option<AgentRuntimeHandle>> {
        let mut handles = self
            .handles
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime handle registry mutex is poisoned"))?;
        let matches = handles.get(thread_id).is_some_and(|current| {
            Arc::ptr_eq(&current.interrupt, &expected.interrupt)
                && Arc::ptr_eq(&current.terminate, &expected.terminate)
        });
        if matches {
            Ok(handles.remove(thread_id))
        } else {
            Ok(None)
        }
    }
}

#[derive(Clone)]
pub struct AgentControl {
    root_thread_id: String,
    store: AgentGraphStore,
    registry: Arc<AgentRegistry>,
    activity: Arc<ActivityBus>,
    runtimes: Arc<RuntimeHandleRegistry>,
    runtime_lifecycle: Arc<Mutex<RuntimeLifecycleState>>,
    lifecycle_notify: Arc<Notify>,
    #[cfg(test)]
    before_runtime_insert_hook: Arc<Mutex<Option<BeforeRuntimeInsertHook>>>,
}

impl AgentControl {
    pub fn open(
        root_thread_id: String,
        store: AgentGraphStore,
        limits: Limits,
    ) -> anyhow::Result<Arc<Self>> {
        store.ensure_root_thread(&root_thread_id)?;
        let snapshot = store.snapshot(&root_thread_id)?;
        if snapshot.threads.iter().any(|thread| {
            thread.canonical_path != AgentPath::root()
                && thread.status == crate::AgentStatusV2::PendingInit
        }) {
            anyhow::bail!(
                "pending reservations require exclusive recovery before opening root {root_thread_id:?}"
            );
        }
        let registry = AgentRegistry::from_threads(limits, &snapshot.threads)?;
        Ok(Arc::new(Self {
            root_thread_id,
            store,
            registry: Arc::new(registry),
            activity: Arc::new(ActivityBus::default()),
            runtimes: Arc::new(RuntimeHandleRegistry::default()),
            runtime_lifecycle: Arc::new(Mutex::new(RuntimeLifecycleState::default())),
            lifecycle_notify: Arc::new(Notify::new()),
            #[cfg(test)]
            before_runtime_insert_hook: Arc::new(Mutex::new(None)),
        }))
    }

    pub fn root_thread_id(&self) -> &str {
        &self.root_thread_id
    }

    pub fn graph_db_path(&self) -> &Path {
        self.store.path()
    }

    pub fn reserve_spawn<'a>(
        &'a self,
        parent: &AgentPath,
        task_name: &str,
    ) -> anyhow::Result<AgentSpawnReservation<'a>> {
        self.reserve_spawn_typed(parent, task_name, "default")
    }

    pub fn reserve_spawn_typed<'a>(
        &'a self,
        parent: &AgentPath,
        task_name: &str,
        agent_type: &str,
    ) -> anyhow::Result<AgentSpawnReservation<'a>> {
        let mut lifecycle = self.lock_runtime_lifecycle()?;
        let parent_thread = self.require_path(parent, "parent agent")?;
        if parent_thread.status == AgentStatusV2::Shutdown {
            anyhow::bail!("cannot spawn under a Shutdown agent: {parent}");
        }
        let child_path = parent.child(task_name).map_err(anyhow::Error::msg)?;
        if lifecycle
            .closing_prefixes
            .iter()
            .any(|prefix| child_path.starts_with(prefix))
        {
            anyhow::bail!("cannot spawn inside a closing agent subtree: {child_path}");
        }
        let thread_id = Uuid::new_v4().to_string();
        let mut reservation = self
            .registry
            .reserve_spawn_typed(parent, task_name, agent_type, &thread_id)?;
        lifecycle
            .inflight_spawns
            .insert(thread_id.clone(), child_path);
        let identity = reservation.thread();
        let persisted = match self.store.reserve_thread(&ThreadReservation {
            thread_id: identity.thread_id.clone(),
            root_thread_id: self.root_thread_id.clone(),
            parent_thread_id: parent_thread.thread_id,
            canonical_path: identity.canonical_path.clone(),
            task_name: identity.task_name.clone(),
            agent_type: identity.agent_type.clone(),
            session_id: identity.session_id.clone(),
        }) {
            Ok(persisted) => persisted,
            Err(error) => {
                lifecycle.inflight_spawns.remove(&thread_id);
                drop(lifecycle);
                self.lifecycle_notify.notify_waiters();
                return Err(error);
            }
        };
        reservation.attach_persisted(&self.store, &self.activity, persisted);
        drop(lifecycle);
        Ok(AgentSpawnReservation {
            inner: Some(reservation),
            control: self,
            lease_id: thread_id,
        })
    }

    pub fn resolve_target(
        &self,
        current: &AgentPath,
        target: &str,
    ) -> anyhow::Result<AgentThreadV2> {
        self.require_path(current, "current agent")?;
        let resolved = current.resolve(target.trim()).map_err(anyhow::Error::msg)?;
        if &resolved == current {
            anyhow::bail!("an agent cannot target itself: {resolved}");
        }
        self.require_path(&resolved, "agent target")
    }

    pub fn list_agents(
        &self,
        current: &AgentPath,
        prefix: Option<&str>,
    ) -> anyhow::Result<Vec<AgentThreadV2>> {
        self.require_path(current, "current agent")?;
        let prefix = match prefix {
            Some(prefix) => current.resolve(prefix.trim()).map_err(anyhow::Error::msg)?,
            None => AgentPath::root(),
        };
        let snapshot = self.store.snapshot(&self.root_thread_id)?;
        let mut threads = Vec::new();
        for thread in snapshot.threads {
            if thread.canonical_path.starts_with(&prefix) && self.is_committed_thread(&thread)? {
                threads.push(thread);
            }
        }
        Ok(threads)
    }

    pub fn enqueue_message(
        &self,
        sender: &AgentPath,
        request: MessageAgentV2Request,
        trigger_turn: bool,
    ) -> anyhow::Result<MailboxMessage> {
        let lifecycle = self.lock_runtime_lifecycle()?;
        self.enqueue_message_locked(&lifecycle, sender, request, trigger_turn)
    }

    fn enqueue_message_locked(
        &self,
        lifecycle: &RuntimeLifecycleState,
        sender: &AgentPath,
        request: MessageAgentV2Request,
        trigger_turn: bool,
    ) -> anyhow::Result<MailboxMessage> {
        let sender_thread = self.require_path(sender, "message sender")?;
        let target = self.resolve_target(sender, &request.target)?;
        if target.status == AgentStatusV2::Shutdown {
            anyhow::bail!("cannot message a Shutdown agent: {}", target.canonical_path);
        }
        if lifecycle
            .closing_prefixes
            .iter()
            .any(|prefix| target.canonical_path.starts_with(prefix))
        {
            anyhow::bail!(
                "cannot message an agent in a closing subtree: {}",
                target.canonical_path
            );
        }
        let message = request.message.trim();
        if message.is_empty() {
            anyhow::bail!("agent message must not be empty");
        }
        let message_id = Uuid::new_v4().to_string();
        let stored = self.store.enqueue(&NewMailboxMessage {
            idempotency_key: format!("agent-message:{message_id}"),
            message_id,
            sender_thread_id: sender_thread.thread_id,
            recipient_thread_id: target.thread_id.clone(),
            kind: if trigger_turn {
                MailboxKind::Followup
            } else {
                MailboxKind::Message
            },
            payload: message.to_string(),
            trigger_turn,
        })?;
        self.activity.publish(
            AgentActivityKind::Mailbox {
                thread_id: target.thread_id.clone(),
            },
            Some(target),
        );
        Ok(stored)
    }

    /// Linearize a follow-up's Shutdown check, durable enqueue, and runtime
    /// admission under the lifecycle lock. The admission closure may take only
    /// the runtime-manager active mutex; the global lock order is lifecycle → active.
    pub fn enqueue_followup_with_admission<T>(
        &self,
        sender: &AgentPath,
        request: MessageAgentV2Request,
        admission: impl FnOnce(&AgentThreadV2) -> anyhow::Result<T>,
    ) -> anyhow::Result<(MailboxMessage, T)> {
        let lifecycle = self.lock_runtime_lifecycle()?;
        let target = self.resolve_target(sender, &request.target)?;
        if target.status == crate::AgentStatusV2::Shutdown {
            anyhow::bail!(
                "cannot follow up a Shutdown agent: {}",
                target.canonical_path
            );
        }
        let message = self.enqueue_message_locked(&lifecycle, sender, request, true)?;
        let admitted = admission(&target)?;
        Ok((message, admitted))
    }

    /// Persist input steered into the currently running main/root turn. This
    /// intentionally does not publish activity: the caller publishes
    /// `MainSteer` only after the durable write succeeds.
    pub fn persist_main_steer(
        &self,
        path: &AgentPath,
        payload: String,
    ) -> anyhow::Result<MailboxMessage> {
        self.persist_main_steer_with_id(path, Uuid::new_v4().to_string(), payload)
    }

    /// Persist a main/root steer using the caller-reserved mailbox identity.
    /// The running turn uses this same id for exact delivery acknowledgement.
    pub fn persist_main_steer_with_id(
        &self,
        path: &AgentPath,
        message_id: String,
        payload: String,
    ) -> anyhow::Result<MailboxMessage> {
        let thread = self.require_path(path, "main steer recipient")?;
        if payload.trim().is_empty() {
            anyhow::bail!("main steer payload must not be empty");
        }
        self.store.enqueue(&NewMailboxMessage {
            idempotency_key: format!("main-steer:{message_id}"),
            message_id,
            sender_thread_id: thread.thread_id.clone(),
            recipient_thread_id: thread.thread_id,
            kind: MailboxKind::Followup,
            payload,
            trigger_turn: true,
        })
    }

    pub fn drain_mailbox(&self, path: &AgentPath) -> anyhow::Result<Vec<MailboxMessage>> {
        let thread = self.require_path(path, "mailbox recipient")?;
        self.store.pending_for(&thread.thread_id, 0)
    }

    pub fn ack_mailbox(&self, path: &AgentPath, through_sequence: i64) -> anyhow::Result<()> {
        let thread = self.require_path(path, "mailbox recipient")?;
        self.store
            .mark_delivered(&thread.thread_id, through_sequence)
    }

    pub fn status_events(&self, thread_id: &str) -> anyhow::Result<Vec<crate::StoredStatusEvent>> {
        self.store.status_events(thread_id)
    }

    pub fn record_runner_event(
        &self,
        thread_id: &str,
        event: RunnerEvent,
    ) -> anyhow::Result<AgentThreadV2> {
        let terminated = matches!(&event, RunnerEvent::RuntimeTerminated);
        let _lifecycle = self.lock_runtime_lifecycle()?;
        let existing = self
            .store
            .get_thread(thread_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown agent thread {thread_id:?}"))?;
        if existing.root_thread_id != self.root_thread_id {
            anyhow::bail!("agent thread {thread_id:?} belongs to a different root");
        }
        if existing.status == crate::AgentStatusV2::Shutdown && !terminated {
            anyhow::bail!("cannot record a runner event for Shutdown agent {thread_id:?}");
        }
        let committed_path = self
            .registry
            .committed_path_for_thread(thread_id)?
            .ok_or_else(|| anyhow::anyhow!("agent thread {thread_id:?} is not committed"))?;
        if committed_path != existing.canonical_path {
            anyhow::bail!(
                "committed agent identity path {committed_path} does not match durable path {}",
                existing.canonical_path
            );
        }
        let thread = self.store.apply_status_event(thread_id, event)?;
        if terminated {
            self.runtimes.remove(thread_id)?;
        }
        let kind = if terminated {
            AgentActivityKind::EdgeClosed {
                thread_id: thread_id.to_string(),
            }
        } else {
            AgentActivityKind::StatusChanged {
                thread_id: thread_id.to_string(),
            }
        };
        self.activity.publish(kind, Some(thread.clone()));
        Ok(thread)
    }

    pub async fn wait_activity(
        &self,
        cursor: ActivityCursor,
        timeout: Duration,
    ) -> WaitAgentResult {
        match self.activity.wait_after(cursor, timeout).await {
            Some(activity) if activity.kind == AgentActivityKind::MainSteer => WaitOutcome::Steered,
            Some(_) => WaitOutcome::MailboxActivity,
            None => WaitOutcome::TimedOut,
        }
    }

    pub fn activity_cursor(&self) -> ActivityCursor {
        self.activity.cursor()
    }

    /// Returns the next activity in strict sequence order for read-only
    /// projection observers. Runtime wait semantics remain on [`Self::wait_activity`].
    pub fn next_activity_after(
        &self,
        cursor: ActivityCursor,
        timeout: Duration,
    ) -> impl Future<Output = crate::ActivityObservation> + Send + 'static {
        let activity = Arc::clone(&self.activity);
        async move { activity.observe_after(cursor, timeout).await }
    }

    pub fn notify_main_steer(&self) {
        self.activity.publish(AgentActivityKind::MainSteer, None);
    }

    pub fn acquire_execution(&self, thread_id: &str) -> anyhow::Result<ExecutionPermit<'_>> {
        self.registry.acquire_execution(thread_id)
    }

    pub fn identity_count(&self) -> anyhow::Result<usize> {
        self.registry.identity_count()
    }

    pub fn active_execution_count(&self) -> anyhow::Result<usize> {
        self.registry.active_execution_count()
    }

    pub fn snapshot(&self) -> anyhow::Result<AgentTreeSnapshotV2> {
        self.snapshot_with_after_cursor(|| {})
    }

    fn snapshot_with_after_cursor<F>(&self, after_cursor: F) -> anyhow::Result<AgentTreeSnapshotV2>
    where
        F: FnOnce(),
    {
        // Capture the observer-generation cursor before reading durable state.
        // A concurrent mutation can therefore be present in both the snapshot
        // and a later event (an idempotent duplicate), but can never be skipped
        // because the snapshot advertised a cursor newer than its contents.
        let activity_sequence = self.activity.cursor().0;
        after_cursor();
        let mut snapshot = self.store.snapshot(&self.root_thread_id)?;
        snapshot.activity_sequence = activity_sequence;
        Ok(snapshot)
    }

    /// Resolve a desktop target from the root namespace. Unlike model target
    /// resolution this deliberately permits `/root`, whose close semantics are
    /// defined as "close descendants only" by the desktop controller.
    pub fn resolve_desktop_target(&self, target: &str) -> anyhow::Result<AgentThreadV2> {
        let path = AgentPath::root()
            .resolve(target.trim())
            .map_err(anyhow::Error::msg)?;
        self.require_path(&path, "desktop agent target")
    }

    pub fn begin_close(self: &Arc<Self>, prefix: AgentPath) -> anyhow::Result<CloseAdmissionGuard> {
        let mut lifecycle = self.lock_runtime_lifecycle()?;
        if lifecycle
            .closing_prefixes
            .iter()
            .any(|existing| existing.starts_with(&prefix) || prefix.starts_with(existing))
        {
            anyhow::bail!("agent subtree close already overlaps {prefix}");
        }
        lifecycle.closing_prefixes.insert(prefix.clone());
        drop(lifecycle);
        Ok(CloseAdmissionGuard {
            control: Arc::clone(self),
            prefix,
            active: true,
        })
    }

    pub fn is_path_closing(&self, path: &AgentPath) -> anyhow::Result<bool> {
        Ok(self
            .lock_runtime_lifecycle()?
            .closing_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix)))
    }

    /// Roll back a spawn that was committed only long enough for the runtime
    /// manager to attempt admission. This is intentionally limited to a
    /// durable `PendingInit` row with no registered runtime.
    pub fn abort_committed_pending_spawn(&self, thread: &AgentThreadV2) -> anyhow::Result<()> {
        if thread.root_thread_id != self.root_thread_id {
            anyhow::bail!("agent thread belongs to a different root");
        }
        if self.runtimes.get(&thread.thread_id)?.is_some() {
            anyhow::bail!("cannot abort a spawn with a registered runtime");
        }
        self.store.rollback_pending_thread(&thread.thread_id)?;
        self.registry
            .rollback_committed_spawn(&thread.canonical_path, &thread.thread_id)
    }

    /// Idempotent final cleanup owned by the runtime launch task. It may run
    /// after the caller already removed durable state, but only releases the
    /// exact matching in-memory identity after execution has ended.
    pub fn finalize_unaccepted_spawn(
        &self,
        thread: &AgentThreadV2,
        turn_id: Option<&str>,
    ) -> anyhow::Result<()> {
        if thread.root_thread_id != self.root_thread_id {
            anyhow::bail!("agent thread belongs to a different root");
        }
        if self.runtimes.get(&thread.thread_id)?.is_some() {
            anyhow::bail!("cannot finalize an unaccepted spawn with a registered runtime");
        }
        if let Some(current) = self.store.get_thread(&thread.thread_id)? {
            anyhow::ensure!(
                current.canonical_path == thread.canonical_path,
                "refusing to clean a replacement agent thread"
            );
            match current.status {
                AgentStatusV2::PendingInit => {
                    self.store.rollback_pending_thread(&thread.thread_id)?;
                }
                AgentStatusV2::Running => {
                    let turn_id = turn_id.ok_or_else(|| {
                        anyhow::anyhow!("running unaccepted spawn is missing its generation token")
                    })?;
                    self.store
                        .rollback_unaccepted_started_thread(&thread.thread_id, turn_id)?;
                }
                _ => anyhow::bail!(
                    "refusing to clean unaccepted spawn after its durable generation advanced"
                ),
            }
        }
        self.registry
            .rollback_committed_spawn_if_matches(&thread.canonical_path, &thread.thread_id)?;
        Ok(())
    }

    pub fn register_runtime(
        &self,
        thread_id: &str,
        handle: AgentRuntimeHandle,
    ) -> anyhow::Result<()> {
        let _lifecycle = self.lock_runtime_lifecycle()?;
        let path = self
            .registry
            .committed_path_for_thread(thread_id)?
            .ok_or_else(|| {
                anyhow::anyhow!("agent thread {thread_id:?} is unknown or not committed")
            })?;
        if path == AgentPath::root() {
            anyhow::bail!("cannot register a child runtime handle for the root agent");
        }
        let thread = self.require_path(&path, "runtime agent")?;
        if thread.status == crate::AgentStatusV2::Shutdown {
            anyhow::bail!("cannot register a runtime handle for Shutdown agent {thread_id:?}");
        }
        #[cfg(test)]
        if let Some(hook) = self
            .before_runtime_insert_hook
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime insert hook mutex is poisoned"))?
            .clone()
        {
            hook();
        }
        self.runtimes.register(thread_id, handle)
    }

    #[cfg(test)]
    fn set_before_runtime_insert_hook(
        &self,
        hook: Option<BeforeRuntimeInsertHook>,
    ) -> anyhow::Result<()> {
        *self
            .before_runtime_insert_hook
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime insert hook mutex is poisoned"))? = hook;
        Ok(())
    }

    pub fn runtime_handle(&self, thread_id: &str) -> anyhow::Result<Option<AgentRuntimeHandle>> {
        self.runtimes.get(thread_id)
    }

    pub fn remove_runtime(&self, thread_id: &str) -> anyhow::Result<Option<AgentRuntimeHandle>> {
        self.runtimes.remove(thread_id)
    }

    pub fn remove_runtime_if_same(
        &self,
        thread_id: &str,
        expected: &AgentRuntimeHandle,
    ) -> anyhow::Result<Option<AgentRuntimeHandle>> {
        self.runtimes.remove_if_same(thread_id, expected)
    }

    fn lock_runtime_lifecycle(&self) -> anyhow::Result<MutexGuard<'_, RuntimeLifecycleState>> {
        self.runtime_lifecycle
            .lock()
            .map_err(|_| anyhow::anyhow!("agent runtime lifecycle mutex is poisoned"))
    }

    fn has_inflight_spawn_under(&self, prefix: &AgentPath) -> anyhow::Result<bool> {
        Ok(self
            .lock_runtime_lifecycle()?
            .inflight_spawns
            .values()
            .any(|path| path.starts_with(prefix)))
    }

    fn release_spawn_lease(&self, lease_id: &str) {
        match self.runtime_lifecycle.lock() {
            Ok(mut lifecycle) => {
                lifecycle.inflight_spawns.remove(lease_id);
            }
            Err(poisoned) => {
                tracing::warn!("recovering poisoned lifecycle state during spawn lease release");
                poisoned.into_inner().inflight_spawns.remove(lease_id);
            }
        }
        self.lifecycle_notify.notify_waiters();
    }

    fn release_close_prefix(&self, prefix: &AgentPath) {
        match self.runtime_lifecycle.lock() {
            Ok(mut lifecycle) => {
                lifecycle.closing_prefixes.remove(prefix);
            }
            Err(poisoned) => {
                tracing::warn!("recovering poisoned lifecycle state during close release");
                poisoned.into_inner().closing_prefixes.remove(prefix);
            }
        }
        self.lifecycle_notify.notify_waiters();
    }

    fn require_path(&self, path: &AgentPath, label: &str) -> anyhow::Result<AgentThreadV2> {
        let thread = self
            .store
            .get_by_path(&self.root_thread_id, path)?
            .ok_or_else(|| anyhow::anyhow!("unknown {label} path {path}"))?;
        if !self.is_committed_thread(&thread)? {
            anyhow::bail!("{label} path {path} is not committed");
        }
        Ok(thread)
    }

    fn is_committed_thread(&self, thread: &AgentThreadV2) -> anyhow::Result<bool> {
        Ok(self
            .registry
            .thread_id_for_path(&thread.canonical_path)?
            .as_deref()
            == Some(thread.thread_id.as_str()))
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct CancelWaitHook {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[derive(Debug, Default)]
/// Cancellation signal for one active V2 Agent Thread turn.
pub struct AgentThreadControl {
    interrupted: AtomicBool,
    closed: AtomicBool,
    notify: Notify,
    #[cfg(test)]
    cancel_wait_hook: Mutex<Option<CancelWaitHook>>,
}

impl AgentThreadControl {
    pub fn interrupt(&self) {
        self.interrupted.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn begin_turn(&self) {
        if !self.closed.load(Ordering::SeqCst) {
            self.interrupted.store(false, Ordering::SeqCst);
        }
    }

    pub fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_interrupted() || self.is_closed() {
                return;
            }
            #[cfg(test)]
            let hook = { self.cancel_wait_hook.lock().unwrap().clone() };
            #[cfg(test)]
            if let Some(hook) = hook {
                hook.entered.notify_one();
                hook.release.notified().await;
            }
            notified.await;
        }
    }

    #[cfg(test)]
    fn set_cancel_wait_hook(&self, hook: Option<CancelWaitHook>) {
        *self.cancel_wait_hook.lock().unwrap() = hook;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::Barrier;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;

    fn limits() -> crate::Limits {
        crate::Limits {
            max_threads: 8,
            max_depth: 3,
            max_running: 2,
        }
    }

    fn open_control(
        dir: &TempDir,
        root_thread_id: &str,
    ) -> (Arc<AgentControl>, crate::AgentGraphStore) {
        let store = crate::AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        let control = AgentControl::open(root_thread_id.into(), store.clone(), limits()).unwrap();
        (control, store)
    }

    fn commit_spawn(
        control: &AgentControl,
        parent: &crate::AgentPath,
        task_name: &str,
    ) -> crate::AgentThreadV2 {
        let reservation = control.reserve_spawn(parent, task_name).unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().unwrap();
        thread
    }

    #[test]
    fn typed_spawn_persists_requested_agent_type() {
        let dir = tempfile::tempdir().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let reservation = control
            .reserve_spawn_typed(&crate::AgentPath::root(), "review", "reviewer")
            .unwrap();
        let thread_id = reservation.thread_id().to_string();
        reservation.commit().unwrap();

        let thread = store.get_thread(&thread_id).unwrap().unwrap();
        assert_eq!(thread.agent_type, "reviewer");
    }

    #[test]
    fn committed_pending_spawn_can_be_aborted_after_start_rejection() {
        let dir = tempfile::tempdir().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let reservation = control
            .reserve_spawn(&crate::AgentPath::root(), "worker")
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().unwrap();

        control.abort_committed_pending_spawn(&thread).unwrap();

        assert!(store.get_thread(&thread.thread_id).unwrap().is_none());
        assert_eq!(control.identity_count().unwrap(), 0);
        assert!(control
            .resolve_target(&crate::AgentPath::root(), "/root/worker")
            .is_err());
    }

    #[test]
    fn reopened_control_snapshot_uses_new_activity_generation_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let (initial, store) = open_control(&dir, "root-thread");
        let worker = commit_spawn(&initial, &crate::AgentPath::root(), "worker");
        initial
            .record_runner_event(
                &worker.thread_id,
                crate::RunnerEvent::TurnStarted {
                    turn_id: "turn-1".into(),
                },
            )
            .unwrap();
        initial
            .record_runner_event(
                &worker.thread_id,
                crate::RunnerEvent::TurnCompleted {
                    turn_id: "turn-1".into(),
                    last_message: "done".into(),
                },
            )
            .unwrap();
        assert!(store.snapshot("root-thread").unwrap().activity_sequence > 0);
        drop(initial);

        let reopened = AgentControl::open("root-thread".into(), store, limits()).unwrap();
        let snapshot = reopened.snapshot().unwrap();
        assert_eq!(snapshot.activity_sequence, 0);
        reopened
            .enqueue_message(
                &crate::AgentPath::root(),
                crate::MessageAgentV2Request {
                    target: "/root/worker".into(),
                    message: "next".into(),
                },
                false,
            )
            .unwrap();
        assert!(reopened.activity_cursor().0 > snapshot.activity_sequence);
    }

    #[test]
    fn snapshot_cursor_before_store_read_allows_racing_activity_replay() {
        let dir = tempfile::tempdir().unwrap();
        let (control, _store) = open_control(&dir, "root-thread");

        let snapshot = control
            .snapshot_with_after_cursor(|| {
                commit_spawn(&control, &crate::AgentPath::root(), "racing_worker");
            })
            .unwrap();

        assert_eq!(snapshot.activity_sequence, 0);
        assert!(snapshot
            .threads
            .iter()
            .any(|thread| thread.canonical_path.as_str() == "/root/racing_worker"));
        assert!(control.activity_cursor().0 > snapshot.activity_sequence);
    }

    fn runtime_handle() -> AgentRuntimeHandle {
        AgentRuntimeHandle {
            interrupt: Arc::new(|| {}),
            terminate: Arc::new(|| {}),
        }
    }

    #[tokio::test]
    async fn cancelled_waiter_cannot_miss_interrupt_between_check_and_registration() {
        let control = Arc::new(AgentThreadControl::default());
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        control.set_cancel_wait_hook(Some(CancelWaitHook {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        }));
        let waiter = tokio::spawn({
            let control = Arc::clone(&control);
            async move { control.cancelled().await }
        });

        entered.notified().await;
        control.interrupt();
        release.notify_one();

        tokio::time::timeout(Duration::from_millis(100), waiter)
            .await
            .expect("cancelled waiter lost the deterministic interrupt wakeup")
            .unwrap();
    }

    #[test]
    fn open_ensures_one_root_without_overwriting_it() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let root = store.get_thread("root-thread").unwrap().unwrap();
        assert_eq!(root.canonical_path, crate::AgentPath::root());
        assert_eq!(root.parent_thread_id, None);
        assert_eq!(root.session_id, "root-thread");
        assert_eq!(control.root_thread_id(), "root-thread");

        store
            .apply_status_event(
                "root-thread",
                crate::RunnerEvent::TurnCompleted {
                    turn_id: "turn-1".into(),
                    last_message: "kept".into(),
                },
            )
            .unwrap();
        let reopened = AgentControl::open("root-thread".into(), store.clone(), limits()).unwrap();
        assert_eq!(reopened.root_thread_id(), "root-thread");
        assert_eq!(
            store.get_thread("root-thread").unwrap().unwrap().status,
            crate::AgentStatusV2::Completed {
                last_message: "kept".into()
            }
        );
        assert_eq!(
            store
                .snapshot("root-thread")
                .unwrap()
                .threads
                .iter()
                .filter(|thread| thread.canonical_path == crate::AgentPath::root())
                .count(),
            1
        );
    }

    #[test]
    fn second_open_rejects_live_pending_reservation_without_deleting_it() {
        let dir = TempDir::new().unwrap();
        let (first, store) = open_control(&dir, "root-thread");
        let reservation = first
            .reserve_spawn(&crate::AgentPath::root(), "worker")
            .unwrap();
        let thread_id = reservation.thread().thread_id.clone();

        let error = AgentControl::open("root-thread".into(), store.clone(), limits())
            .err()
            .expect("a second owner must not load a pending reservation");

        assert!(error.to_string().contains("exclusive recovery"));
        assert_eq!(
            store.get_thread(&thread_id).unwrap().unwrap().status,
            crate::AgentStatusV2::PendingInit
        );
        assert_eq!(
            store.edge_state(&thread_id).unwrap().as_deref(),
            Some("open")
        );
        reservation.commit().unwrap();
        assert!(store.get_thread(&thread_id).unwrap().is_some());
        assert_eq!(
            store.edge_state(&thread_id).unwrap().as_deref(),
            Some("open")
        );
        assert_eq!(
            first
                .resolve_target(&crate::AgentPath::root(), "worker")
                .unwrap()
                .thread_id,
            thread_id
        );
    }

    #[test]
    fn control_spawn_reservation_rolls_back_store_and_registry() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let path = crate::AgentPath::parse("/root/worker").unwrap();
        let thread_id = {
            let reservation = control
                .reserve_spawn(&crate::AgentPath::root(), "worker")
                .unwrap();
            let thread_id = reservation.thread().thread_id.clone();
            assert!(store.get_thread(&thread_id).unwrap().is_some());
            thread_id
        };
        assert!(store.get_thread(&thread_id).unwrap().is_none());
        let retry = control
            .reserve_spawn(&crate::AgentPath::root(), "worker")
            .unwrap();
        assert!(store.get_by_path("root-thread", &path).unwrap().is_some());
        drop(retry);
        assert!(store.get_by_path("root-thread", &path).unwrap().is_none());
    }

    #[test]
    fn explicit_abort_synchronously_rolls_back_durable_and_memory_reservation() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let root = crate::AgentPath::root();
        let reservation = control.reserve_spawn(&root, "worker").unwrap();
        let thread_id = reservation.thread().thread_id.clone();

        reservation.abort().unwrap();

        assert!(store.get_thread(&thread_id).unwrap().is_none());
        let replacement = control.reserve_spawn(&root, "worker").unwrap();
        assert_ne!(replacement.thread().thread_id, thread_id);
    }

    #[test]
    fn commit_rejects_a_cleaned_durable_reservation_without_ghost_identity() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let root = crate::AgentPath::root();
        let reservation = control.reserve_spawn(&root, "worker").unwrap();
        let thread_id = reservation.thread().thread_id.clone();
        let cursor = control.activity_cursor();
        assert_eq!(
            store.cleanup_pending_reservations("root-thread").unwrap(),
            1
        );

        let error = reservation.commit().unwrap_err();

        assert!(error.to_string().contains("durable pending reservation"));
        assert_eq!(control.activity_cursor(), cursor);
        assert_eq!(control.identity_count().unwrap(), 0);
        assert!(store.get_thread(&thread_id).unwrap().is_none());
        assert!(store.edge_state(&thread_id).unwrap().is_none());
        assert!(control.resolve_target(&root, "worker").is_err());
        let replacement = control.reserve_spawn(&root, "worker").unwrap();
        assert_ne!(replacement.thread().thread_id, thread_id);
    }

    #[test]
    fn validation_failure_rolls_back_durable_reservation_before_releasing_memory() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let root = crate::AgentPath::root();
        let reservation = control.reserve_spawn(&root, "worker").unwrap();
        let thread_id = reservation.thread().thread_id.clone();
        store.close_edge(&thread_id).unwrap();

        let error = reservation.commit().unwrap_err();

        assert!(error.to_string().contains("durable pending reservation"));
        assert!(store.get_thread(&thread_id).unwrap().is_none());
        assert!(store.edge_state(&thread_id).unwrap().is_none());
        assert!(control.reserve_spawn(&root, "worker").is_ok());
    }

    #[test]
    fn validation_and_rollback_failures_are_both_reported() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let reservation = control
            .reserve_spawn(&crate::AgentPath::root(), "worker")
            .unwrap();
        let thread_id = reservation.thread().thread_id.clone();
        store
            .apply_status_event(
                &thread_id,
                crate::RunnerEvent::TurnStarted {
                    turn_id: "invalid-early-start".into(),
                },
            )
            .unwrap();

        let error = reservation.commit().unwrap_err().to_string();

        assert!(error.contains("durable pending reservation validation failed"));
        assert!(error.contains("durable rollback failed"));
    }

    #[test]
    fn runner_events_require_committed_identity_and_drop_still_rolls_back() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let root = crate::AgentPath::root();
        let reservation = control.reserve_spawn(&root, "worker").unwrap();
        let thread_id = reservation.thread().thread_id.clone();
        assert_eq!(
            store.get_thread(&thread_id).unwrap().unwrap().status,
            crate::AgentStatusV2::PendingInit
        );

        let error = control
            .record_runner_event(
                &thread_id,
                crate::RunnerEvent::TurnStarted {
                    turn_id: "too-early".into(),
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("not committed"));
        assert_eq!(
            store.get_thread(&thread_id).unwrap().unwrap().status,
            crate::AgentStatusV2::PendingInit
        );

        drop(reservation);
        assert!(store.get_thread(&thread_id).unwrap().is_none());
        let edge_count: i64 = rusqlite::Connection::open(store.path())
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM agent_spawn_edges WHERE child_thread_id = ?1",
                [&thread_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(edge_count, 0);

        let committed = control.reserve_spawn(&root, "worker").unwrap();
        let committed_id = committed.thread().thread_id.clone();
        committed.commit().unwrap();
        assert_eq!(
            control
                .record_runner_event(
                    &committed_id,
                    crate::RunnerEvent::TurnStarted {
                        turn_id: "allowed".into(),
                    },
                )
                .unwrap()
                .status,
            crate::AgentStatusV2::Running
        );
    }

    #[test]
    fn uncommitted_reservations_are_not_model_visible_or_messageable() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let root = crate::AgentPath::root();
        let reservation = control.reserve_spawn(&root, "worker").unwrap();
        let pending = reservation.thread().clone();

        assert_eq!(
            control
                .list_agents(&root, None)
                .unwrap()
                .into_iter()
                .map(|thread| thread.canonical_path)
                .collect::<Vec<_>>(),
            vec![root.clone()]
        );
        assert!(control.resolve_target(&root, "worker").is_err());
        assert!(control
            .enqueue_message(
                &root,
                crate::MessageAgentV2Request {
                    target: "worker".into(),
                    message: "too early".into(),
                },
                true,
            )
            .is_err());
        assert!(store.pending_for(&pending.thread_id, 0).unwrap().is_empty());

        drop(reservation);
        assert!(store.get_thread(&pending.thread_id).unwrap().is_none());
        assert!(store.pending_for(&pending.thread_id, 0).unwrap().is_empty());

        let committed = control.reserve_spawn(&root, "worker").unwrap();
        let worker = committed.thread().clone();
        committed.commit().unwrap();
        assert!(control
            .list_agents(&root, None)
            .unwrap()
            .iter()
            .any(|thread| thread.thread_id == worker.thread_id));
        assert_eq!(
            control.resolve_target(&root, "worker").unwrap().thread_id,
            worker.thread_id
        );
        control
            .enqueue_message(
                &root,
                crate::MessageAgentV2Request {
                    target: "worker".into(),
                    message: "ready".into(),
                },
                true,
            )
            .unwrap();
        assert_eq!(store.pending_for(&worker.thread_id, 0).unwrap().len(), 1);

        let uncommitted_sender = control.reserve_spawn(&root, "sender").unwrap();
        assert!(control
            .enqueue_message(
                &uncommitted_sender.thread().canonical_path,
                crate::MessageAgentV2Request {
                    target: "/root/worker".into(),
                    message: "orphan sender".into(),
                },
                false,
            )
            .is_err());
        assert_eq!(store.pending_for(&worker.thread_id, 0).unwrap().len(), 1);
    }

    #[test]
    fn store_reservation_failure_does_not_leak_registry_slot() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let external = crate::ThreadReservation {
            thread_id: "external-thread".into(),
            root_thread_id: "root-thread".into(),
            parent_thread_id: "root-thread".into(),
            canonical_path: crate::AgentPath::parse("/root/raced").unwrap(),
            task_name: "raced".into(),
            agent_type: "default".into(),
            session_id: "external-thread".into(),
        };
        store.reserve_thread(&external).unwrap();
        assert!(control
            .reserve_spawn(&crate::AgentPath::root(), "raced")
            .is_err());
        store.rollback_pending_thread("external-thread").unwrap();
        assert!(control
            .reserve_spawn(&crate::AgentPath::root(), "raced")
            .is_ok());
    }

    #[test]
    fn committed_threads_are_listed_by_sorted_subtree_and_resolved_by_path() {
        let dir = TempDir::new().unwrap();
        let (control, _store) = open_control(&dir, "root-thread");
        let root = crate::AgentPath::root();
        let alpha = commit_spawn(&control, &root, "alpha");
        let nested = commit_spawn(&control, &alpha.canonical_path, "nested");
        let beta = commit_spawn(&control, &root, "beta");

        let all = control.list_agents(&root, None).unwrap();
        assert_eq!(
            all.iter()
                .map(|thread| thread.canonical_path.as_str())
                .collect::<Vec<_>>(),
            vec!["/root", "/root/alpha", "/root/alpha/nested", "/root/beta"]
        );
        assert_eq!(
            control
                .list_agents(&alpha.canonical_path, None)
                .unwrap()
                .iter()
                .map(|thread| thread.canonical_path.as_str())
                .collect::<Vec<_>>(),
            vec!["/root", "/root/alpha", "/root/alpha/nested", "/root/beta"]
        );
        assert_eq!(
            control
                .list_agents(&root, Some("alpha"))
                .unwrap()
                .iter()
                .map(|thread| thread.canonical_path.as_str())
                .collect::<Vec<_>>(),
            vec!["/root/alpha", "/root/alpha/nested"]
        );
        assert_eq!(
            control
                .list_agents(&alpha.canonical_path, Some("nested"))
                .unwrap()
                .iter()
                .map(|thread| thread.canonical_path.as_str())
                .collect::<Vec<_>>(),
            vec!["/root/alpha/nested"]
        );
        assert_eq!(
            control.resolve_target(&root, "alpha").unwrap().thread_id,
            alpha.thread_id
        );
        assert_eq!(
            control
                .resolve_target(&alpha.canonical_path, "nested")
                .unwrap()
                .thread_id,
            nested.thread_id
        );
        assert_eq!(
            control
                .resolve_target(&root, "/root/beta")
                .unwrap()
                .thread_id,
            beta.thread_id
        );
        assert!(control.resolve_target(&root, "/root").is_err());
        assert!(control.resolve_target(&root, "missing").is_err());
    }

    #[test]
    fn identified_main_steer_keeps_caller_identity_and_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let root = crate::AgentPath::root();

        let first = control
            .persist_main_steer_with_id(&root, "steer-message-1".into(), "payload".into())
            .unwrap();
        let retry = control
            .persist_main_steer_with_id(&root, "steer-message-1".into(), "payload".into())
            .unwrap();

        assert_eq!(first.message_id, "steer-message-1");
        assert_eq!(retry, first);
        assert_eq!(store.pending_for("root-thread", 0).unwrap(), vec![first]);
    }

    #[tokio::test]
    async fn wait_activity_maps_mailbox_steer_and_timeout() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let root = crate::AgentPath::root();
        let worker = commit_spawn(&control, &root, "worker");

        let mailbox_cursor = control.activity_cursor();
        let mailbox = control
            .enqueue_message(
                &root,
                crate::MessageAgentV2Request {
                    target: "worker".into(),
                    message: "continue".into(),
                },
                true,
            )
            .unwrap();
        assert_eq!(
            control
                .wait_activity(mailbox_cursor, Duration::from_millis(20))
                .await,
            WaitOutcome::MailboxActivity
        );
        assert_eq!(
            store.pending_for(&worker.thread_id, 0).unwrap(),
            vec![mailbox]
        );

        let steer_cursor = control.activity_cursor();
        control.notify_main_steer();
        assert_eq!(
            control
                .wait_activity(steer_cursor, Duration::from_millis(20))
                .await,
            WaitOutcome::Steered
        );
        assert_eq!(
            control
                .wait_activity(control.activity_cursor(), Duration::from_millis(1))
                .await,
            WaitOutcome::TimedOut
        );

        let priority_cursor = control.activity_cursor();
        control
            .enqueue_message(
                &root,
                crate::MessageAgentV2Request {
                    target: "worker".into(),
                    message: "mail before steer".into(),
                },
                false,
            )
            .unwrap();
        control.notify_main_steer();
        assert_eq!(
            control
                .wait_activity(priority_cursor, Duration::from_millis(20))
                .await,
            WaitOutcome::Steered
        );
    }

    #[tokio::test]
    async fn status_activity_wakes_waiter_but_store_remains_durable_source() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let worker = commit_spawn(&control, &crate::AgentPath::root(), "worker");
        let cursor = control.activity_cursor();
        let updated = control
            .record_runner_event(
                &worker.thread_id,
                crate::RunnerEvent::TurnCompleted {
                    turn_id: "turn-1".into(),
                    last_message: "durable".into(),
                },
            )
            .unwrap();

        assert_eq!(
            control
                .wait_activity(cursor, Duration::from_millis(20))
                .await,
            WaitOutcome::MailboxActivity
        );
        assert!(control.activity_cursor().0 > cursor.0);
        assert_eq!(
            store.get_thread(&worker.thread_id).unwrap().unwrap(),
            updated
        );
        assert_eq!(store.status_events(&worker.thread_id).unwrap().len(), 1);
    }

    #[test]
    fn runtime_handles_are_registered_per_root_control() {
        let first_dir = TempDir::new().unwrap();
        let second_dir = TempDir::new().unwrap();
        let (first, _first_store) = open_control(&first_dir, "first-root");
        let (second, _second_store) = open_control(&second_dir, "second-root");
        let worker = commit_spawn(&first, &crate::AgentPath::root(), "worker");
        let pending = first
            .reserve_spawn(&crate::AgentPath::root(), "pending")
            .unwrap();

        assert!(first.register_runtime("unknown", runtime_handle()).is_err());
        assert!(first
            .register_runtime("first-root", runtime_handle())
            .is_err());
        assert!(first
            .register_runtime(&pending.thread().thread_id, runtime_handle())
            .is_err());
        assert!(second
            .register_runtime(&worker.thread_id, runtime_handle())
            .is_err());
        pending.abort().unwrap();

        let interrupt_count = Arc::new(AtomicUsize::new(0));
        let terminate_count = Arc::new(AtomicUsize::new(0));
        let interrupt_counter = Arc::clone(&interrupt_count);
        let terminate_counter = Arc::clone(&terminate_count);
        first
            .register_runtime(
                &worker.thread_id,
                AgentRuntimeHandle {
                    interrupt: Arc::new(move || {
                        interrupt_counter.fetch_add(1, Ordering::SeqCst);
                    }),
                    terminate: Arc::new(move || {
                        terminate_counter.fetch_add(1, Ordering::SeqCst);
                    }),
                },
            )
            .unwrap();

        let handle = first.runtime_handle(&worker.thread_id).unwrap().unwrap();
        (handle.interrupt)();
        (handle.terminate)();
        assert_eq!(interrupt_count.load(Ordering::SeqCst), 1);
        assert_eq!(terminate_count.load(Ordering::SeqCst), 1);
        assert!(second.runtime_handle(&worker.thread_id).unwrap().is_none());
        assert!(first.remove_runtime(&worker.thread_id).unwrap().is_some());
        assert!(first.runtime_handle(&worker.thread_id).unwrap().is_none());
    }

    #[test]
    fn conditional_runtime_removal_preserves_a_replacement_handle() {
        let dir = TempDir::new().unwrap();
        let (control, _store) = open_control(&dir, "root-thread");
        let worker = commit_spawn(&control, &crate::AgentPath::root(), "worker");
        let original = runtime_handle();
        let replacement = runtime_handle();
        control
            .register_runtime(&worker.thread_id, original.clone())
            .unwrap();
        assert!(control.remove_runtime(&worker.thread_id).unwrap().is_some());
        control
            .register_runtime(&worker.thread_id, replacement.clone())
            .unwrap();

        assert!(control
            .remove_runtime_if_same(&worker.thread_id, &original)
            .unwrap()
            .is_none());
        let current = control.runtime_handle(&worker.thread_id).unwrap().unwrap();
        assert!(Arc::ptr_eq(&current.interrupt, &replacement.interrupt));
        assert!(Arc::ptr_eq(&current.terminate, &replacement.terminate));
        assert!(control
            .remove_runtime_if_same(&worker.thread_id, &replacement)
            .unwrap()
            .is_some());
    }

    #[test]
    fn runtime_termination_removes_registered_handle_after_durable_transition() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let worker = commit_spawn(&control, &crate::AgentPath::root(), "worker");
        control
            .register_runtime(&worker.thread_id, runtime_handle())
            .unwrap();

        let terminated = control
            .record_runner_event(&worker.thread_id, crate::RunnerEvent::RuntimeTerminated)
            .unwrap();

        assert_eq!(terminated.status, crate::AgentStatusV2::Shutdown);
        assert_eq!(
            store.edge_state(&worker.thread_id).unwrap().as_deref(),
            Some("closed")
        );
        assert!(control.runtime_handle(&worker.thread_id).unwrap().is_none());
    }

    #[test]
    fn shutdown_agent_rejects_runtime_registration() {
        let dir = TempDir::new().unwrap();
        let (control, _store) = open_control(&dir, "root-thread");
        let worker = commit_spawn(&control, &crate::AgentPath::root(), "worker");
        control
            .record_runner_event(&worker.thread_id, crate::RunnerEvent::RuntimeTerminated)
            .unwrap();

        let error = control
            .register_runtime(&worker.thread_id, runtime_handle())
            .unwrap_err();

        assert!(error.to_string().contains("Shutdown"));
        assert!(control.runtime_handle(&worker.thread_id).unwrap().is_none());
    }

    #[test]
    fn concurrent_runtime_registration_and_termination_leave_no_stale_handle() {
        let dir = TempDir::new().unwrap();
        let (control, store) = open_control(&dir, "root-thread");
        let worker = commit_spawn(&control, &crate::AgentPath::root(), "worker");
        let start_termination = Arc::new(Barrier::new(2));
        let hook_barrier = Arc::clone(&start_termination);
        control
            .set_before_runtime_insert_hook(Some(Arc::new(move || {
                hook_barrier.wait();
                std::thread::sleep(Duration::from_millis(100));
            })))
            .unwrap();

        let register_control = Arc::clone(&control);
        let register_thread_id = worker.thread_id.clone();
        let register = std::thread::spawn(move || {
            register_control.register_runtime(&register_thread_id, runtime_handle())
        });
        let terminate_control = Arc::clone(&control);
        let terminate_thread_id = worker.thread_id.clone();
        let terminate = std::thread::spawn(move || {
            start_termination.wait();
            terminate_control
                .record_runner_event(&terminate_thread_id, crate::RunnerEvent::RuntimeTerminated)
        });

        register.join().unwrap().unwrap();
        terminate.join().unwrap().unwrap();
        control.set_before_runtime_insert_hook(None).unwrap();

        assert_eq!(
            store.get_thread(&worker.thread_id).unwrap().unwrap().status,
            crate::AgentStatusV2::Shutdown
        );
        assert!(control.runtime_handle(&worker.thread_id).unwrap().is_none());
    }
}
