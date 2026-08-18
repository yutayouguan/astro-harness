use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use tokio::sync::{mpsc, Notify};
use uuid::Uuid;

use crate::{
    ActivityBus, ActivityCursor, AgentActivityKind, AgentGraphStore, AgentPath, AgentRegistry,
    AgentThreadV2, ExecutionPermit, Limits, MailboxKind, MailboxMessage, MessageAgentV2Request,
    NewMailboxMessage, RunnerEvent, SpawnReservation, ThreadReservation,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    MailboxActivity,
    Steered,
    TimedOut,
}

pub type WaitAgentResult = WaitOutcome;

#[derive(Clone)]
pub struct AgentRuntimeHandle {
    pub interrupt: Arc<dyn Fn() + Send + Sync>,
    pub terminate: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
pub struct RuntimeHandleRegistry {
    handles: Mutex<HashMap<String, AgentRuntimeHandle>>,
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
}

#[derive(Clone)]
pub struct AgentControl {
    root_thread_id: String,
    store: AgentGraphStore,
    registry: Arc<AgentRegistry>,
    activity: Arc<ActivityBus>,
    runtimes: Arc<RuntimeHandleRegistry>,
}

impl AgentControl {
    pub fn open(
        root_thread_id: String,
        store: AgentGraphStore,
        limits: Limits,
    ) -> anyhow::Result<Arc<Self>> {
        store.ensure_root_thread(&root_thread_id)?;
        let snapshot = store.snapshot(&root_thread_id)?;
        let registry = AgentRegistry::from_threads(limits, &snapshot.threads)?;
        Ok(Arc::new(Self {
            root_thread_id,
            store,
            registry: Arc::new(registry),
            activity: Arc::new(ActivityBus::default()),
            runtimes: Arc::new(RuntimeHandleRegistry::default()),
        }))
    }

    pub fn root_thread_id(&self) -> &str {
        &self.root_thread_id
    }

    pub fn reserve_spawn<'a>(
        &'a self,
        parent: &AgentPath,
        task_name: &str,
    ) -> anyhow::Result<SpawnReservation<'a>> {
        let parent_thread = self
            .store
            .get_by_path(&self.root_thread_id, parent)?
            .ok_or_else(|| anyhow::anyhow!("unknown parent agent path {parent:?}"))?;
        let thread_id = Uuid::new_v4().to_string();
        let mut reservation = self.registry.reserve_spawn(parent, task_name, &thread_id)?;
        let identity = reservation.thread();
        let persisted = self.store.reserve_thread(&ThreadReservation {
            thread_id: identity.thread_id.clone(),
            root_thread_id: self.root_thread_id.clone(),
            parent_thread_id: parent_thread.thread_id,
            canonical_path: identity.canonical_path.clone(),
            task_name: identity.task_name.clone(),
            agent_type: identity.agent_type.clone(),
            session_id: identity.session_id.clone(),
        })?;
        reservation.attach_persisted(&self.store, &self.activity, persisted);
        Ok(reservation)
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
        self.store
            .get_by_path(&self.root_thread_id, &resolved)?
            .ok_or_else(|| anyhow::anyhow!("unknown agent target {resolved}"))
    }

    pub fn list_agents(
        &self,
        current: &AgentPath,
        prefix: Option<&str>,
    ) -> anyhow::Result<Vec<AgentThreadV2>> {
        self.require_path(current, "current agent")?;
        let prefix = match prefix {
            Some(prefix) => current.resolve(prefix.trim()).map_err(anyhow::Error::msg)?,
            None => current.clone(),
        };
        let snapshot = self.store.snapshot(&self.root_thread_id)?;
        Ok(snapshot
            .threads
            .into_iter()
            .filter(|thread| thread.canonical_path.starts_with(&prefix))
            .collect())
    }

    pub fn enqueue_message(
        &self,
        sender: &AgentPath,
        request: MessageAgentV2Request,
        trigger_turn: bool,
    ) -> anyhow::Result<MailboxMessage> {
        let sender_thread = self.require_path(sender, "message sender")?;
        let target = self.resolve_target(sender, &request.target)?;
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

    pub fn record_runner_event(
        &self,
        thread_id: &str,
        event: RunnerEvent,
    ) -> anyhow::Result<AgentThreadV2> {
        let existing = self
            .store
            .get_thread(thread_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown agent thread {thread_id:?}"))?;
        if existing.root_thread_id != self.root_thread_id {
            anyhow::bail!("agent thread {thread_id:?} belongs to a different root");
        }
        let terminated = matches!(event, RunnerEvent::RuntimeTerminated);
        let thread = self.store.apply_status_event(thread_id, event)?;
        let kind = if terminated {
            self.store.close_edge(thread_id)?;
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

    pub fn notify_main_steer(&self) {
        self.activity.publish(AgentActivityKind::MainSteer, None);
    }

    pub fn acquire_execution(&self, thread_id: &str) -> anyhow::Result<ExecutionPermit<'_>> {
        self.registry.acquire_execution(thread_id)
    }

    pub fn identity_count(&self) -> anyhow::Result<usize> {
        self.registry.identity_count()
    }

    pub fn register_runtime(
        &self,
        thread_id: &str,
        handle: AgentRuntimeHandle,
    ) -> anyhow::Result<()> {
        self.runtimes.register(thread_id, handle)
    }

    pub fn runtime_handle(&self, thread_id: &str) -> anyhow::Result<Option<AgentRuntimeHandle>> {
        self.runtimes.get(thread_id)
    }

    pub fn remove_runtime(&self, thread_id: &str) -> anyhow::Result<Option<AgentRuntimeHandle>> {
        self.runtimes.remove(thread_id)
    }

    fn require_path(&self, path: &AgentPath, label: &str) -> anyhow::Result<AgentThreadV2> {
        self.store
            .get_by_path(&self.root_thread_id, path)?
            .ok_or_else(|| anyhow::anyhow!("unknown {label} path {path}"))
    }
}

/// Transitional legacy limit retained until Tasks 6/10 migrate downstream callers.
pub const MAX_LIVE_AGENT_THREADS: usize = 32;

#[derive(Debug)]
pub enum AgentThreadCommand {
    FollowUp(String),
    Close,
}

#[derive(Debug, Default)]
pub struct AgentThreadControl {
    interrupted: AtomicBool,
    closed: AtomicBool,
    notify: Notify,
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
            if self.is_interrupted() || self.is_closed() {
                return;
            }
            self.notify.notified().await;
        }
    }
}

#[derive(Clone)]
struct LiveThread {
    tx: mpsc::UnboundedSender<AgentThreadCommand>,
    control: Arc<AgentThreadControl>,
    parent_session_id: String,
}

#[derive(Default)]
/// Transitional process-global registry retained only for legacy callers.
/// V2 code must share one root-scoped [`AgentControl`] instead.
pub struct LiveAgentThreads {
    inner: Mutex<HashMap<String, LiveThread>>,
}

impl LiveAgentThreads {
    pub fn global() -> &'static Self {
        static REGISTRY: OnceLock<LiveAgentThreads> = OnceLock::new();
        REGISTRY.get_or_init(Self::default)
    }

    pub fn ensure_capacity(
        &self,
        parent_session_id: &str,
        max_per_session: usize,
    ) -> anyhow::Result<()> {
        let live = self.lock_inner();
        check_capacity(&live, parent_session_id, max_per_session)
    }

    /// 原子检查并登记存活线程。已完成但仍可追问的线程也保留在此表中，
    /// 因此会继续占用会话级与全局资源预算，直到显式关闭或进程退出。
    pub fn register_bounded(
        &self,
        thread_id: &str,
        parent_session_id: &str,
        max_per_session: usize,
    ) -> anyhow::Result<(
        Arc<AgentThreadControl>,
        mpsc::UnboundedReceiver<AgentThreadCommand>,
    )> {
        let (tx, rx) = mpsc::unbounded_channel();
        let control = Arc::new(AgentThreadControl::default());
        let mut live = self.lock_inner();
        if live.contains_key(thread_id) {
            anyhow::bail!("agent thread is already live: {thread_id}");
        }
        check_capacity(&live, parent_session_id, max_per_session)?;
        live.insert(
            thread_id.to_string(),
            LiveThread {
                tx,
                control: Arc::clone(&control),
                parent_session_id: parent_session_id.to_string(),
            },
        );
        Ok((control, rx))
    }

    pub fn send_follow_up(&self, thread_id: &str, message: String) -> anyhow::Result<()> {
        let live = self
            .lock_inner()
            .get(thread_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("agent thread is not live: {thread_id}"))?;
        live.tx
            .send(AgentThreadCommand::FollowUp(message))
            .map_err(|_| anyhow::anyhow!("agent thread command channel closed: {thread_id}"))
    }

    pub fn interrupt(&self, thread_id: &str) -> anyhow::Result<()> {
        let live = self
            .lock_inner()
            .get(thread_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("agent thread is not live: {thread_id}"))?;
        live.control.interrupt();
        Ok(())
    }

    pub fn close(&self, thread_id: &str) -> anyhow::Result<()> {
        let live = self
            .lock_inner()
            .get(thread_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("agent thread is not live: {thread_id}"))?;
        live.control.close();
        let _ = live.tx.send(AgentThreadCommand::Close);
        Ok(())
    }

    pub fn remove(&self, thread_id: &str) {
        self.lock_inner().remove(thread_id);
    }

    pub fn is_live(&self, thread_id: &str) -> bool {
        self.lock_inner().contains_key(thread_id)
    }

    fn lock_inner(&self) -> MutexGuard<'_, HashMap<String, LiveThread>> {
        match self.inner.lock() {
            Ok(live) => live,
            Err(poisoned) => {
                tracing::warn!("recovering poisoned legacy live-agent registry");
                poisoned.into_inner()
            }
        }
    }
}

fn check_capacity(
    live: &HashMap<String, LiveThread>,
    parent_session_id: &str,
    max_per_session: usize,
) -> anyhow::Result<()> {
    let session_count = live
        .values()
        .filter(|thread| thread.parent_session_id == parent_session_id)
        .count();
    if session_count >= max_per_session {
        anyhow::bail!(
            "subagent live-thread limit reached for session ({session_count}/{max_per_session})"
        );
    }
    if live.len() >= MAX_LIVE_AGENT_THREADS {
        anyhow::bail!(
            "global subagent live-thread limit reached ({}/{MAX_LIVE_AGENT_THREADS})",
            live.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
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

    #[tokio::test]
    async fn follow_up_interrupt_and_close_are_independent_controls() {
        let registry = LiveAgentThreads::default();
        let (control, mut rx) = registry.register_bounded("thread", "parent", 1).unwrap();
        registry.send_follow_up("thread", "next".into()).unwrap();
        assert!(matches!(
            rx.recv().await,
            Some(AgentThreadCommand::FollowUp(message)) if message == "next"
        ));

        registry.interrupt("thread").unwrap();
        control.cancelled().await;
        assert!(control.is_interrupted());
        assert!(!control.is_closed());

        control.begin_turn();
        assert!(!control.is_interrupted());
        registry.close("thread").unwrap();
        assert!(control.is_closed());
        assert!(matches!(rx.recv().await, Some(AgentThreadCommand::Close)));
    }

    #[test]
    fn completed_live_threads_still_consume_session_budget() {
        let registry = LiveAgentThreads::default();
        let (_first, _first_rx) = registry.register_bounded("first", "parent", 1).unwrap();
        let error = registry
            .register_bounded("second", "parent", 1)
            .unwrap_err();
        assert!(error.to_string().contains("live-thread limit"));

        registry.remove("first");
        assert!(registry.register_bounded("second", "parent", 1).is_ok());
    }

    #[test]
    fn live_thread_limit_is_scoped_per_parent_session() {
        let registry = LiveAgentThreads::default();
        let (_first, _first_rx) = registry.register_bounded("first", "parent-a", 1).unwrap();
        assert!(registry.register_bounded("second", "parent-b", 1).is_ok());
    }

    #[test]
    fn global_live_thread_limit_bounds_all_sessions() {
        let registry = LiveAgentThreads::default();
        for index in 0..MAX_LIVE_AGENT_THREADS {
            registry
                .register_bounded(&format!("thread-{index}"), &format!("parent-{index}"), 1)
                .unwrap();
        }
        let error = registry
            .register_bounded("overflow", "overflow-parent", 1)
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("global subagent live-thread limit"));
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
            vec!["/root/alpha", "/root/alpha/nested"]
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
        let interrupt_count = Arc::new(AtomicUsize::new(0));
        let terminate_count = Arc::new(AtomicUsize::new(0));
        let interrupt_counter = Arc::clone(&interrupt_count);
        let terminate_counter = Arc::clone(&terminate_count);
        first
            .register_runtime(
                "worker",
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

        let handle = first.runtime_handle("worker").unwrap().unwrap();
        (handle.interrupt)();
        (handle.terminate)();
        assert_eq!(interrupt_count.load(Ordering::SeqCst), 1);
        assert_eq!(terminate_count.load(Ordering::SeqCst), 1);
        assert!(second.runtime_handle("worker").unwrap().is_none());
        assert!(first.remove_runtime("worker").unwrap().is_some());
        assert!(first.runtime_handle("worker").unwrap().is_none());
    }
}
