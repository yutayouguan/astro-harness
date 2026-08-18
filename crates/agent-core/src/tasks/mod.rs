//! Session-owned task lifecycle.
//!
//! Names and ownership follow Codex's task model: a [`SessionTask`] describes
//! one workflow, [`RunningTask`] records the active task, and [`ActiveTurn`]
//! enforces the single-active-task invariant for a session.

mod regular;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use agent_protocol::{TurnAbortReason, TurnAbortedEvent, TurnInput};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::runtime::{Session, TurnContext};

pub(crate) use regular::RegularTask;

pub(crate) type SessionTaskResult = anyhow::Result<Option<String>>;
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
#[cfg(not(test))]
const TASK_ABORT_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const TASK_ABORT_TIMEOUT: Duration = Duration::from_millis(50);
#[cfg(not(test))]
const TASK_ABORT_HOOK_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const TASK_ABORT_HOOK_TIMEOUT: Duration = Duration::from_millis(50);

/// The workflow currently owned by a session task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // ReviewTask and CompactTask land in the next Phase B batch.
pub enum TaskKind {
    Regular,
    Review,
    Compact,
}

/// Async task that drives one session turn.
pub(crate) trait SessionTask: Send + Sync + 'static {
    fn kind(&self) -> TaskKind;

    fn span_name(&self) -> &'static str;

    fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> impl Future<Output = SessionTaskResult> + Send;

    fn abort(
        &self,
        session: Arc<Session>,
        ctx: Arc<TurnContext>,
    ) -> impl Future<Output = ()> + Send {
        async move {
            let _ = (session, ctx);
        }
    }
}

/// Object-safe adapter used by the active-task registry.
pub(crate) trait AnySessionTask: Send + Sync + 'static {
    fn kind(&self) -> TaskKind;

    fn span_name(&self) -> &'static str;

    fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> BoxFuture<'static, SessionTaskResult>;

    fn abort<'a>(&'a self, session: Arc<Session>, ctx: Arc<TurnContext>) -> BoxFuture<'a, ()>;
}

impl<T> AnySessionTask for T
where
    T: SessionTask,
{
    fn kind(&self) -> TaskKind {
        SessionTask::kind(self)
    }

    fn span_name(&self) -> &'static str {
        SessionTask::span_name(self)
    }

    fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> BoxFuture<'static, SessionTaskResult> {
        Box::pin(SessionTask::run(
            self,
            session,
            ctx,
            input,
            cancellation_token,
        ))
    }

    fn abort<'a>(&'a self, session: Arc<Session>, ctx: Arc<TurnContext>) -> BoxFuture<'a, ()> {
        Box::pin(SessionTask::abort(self, session, ctx))
    }
}

/// Metadata for the task currently running in a session.
pub(crate) struct RunningTask {
    pub(crate) kind: TaskKind,
    pub(crate) task: Arc<dyn AnySessionTask>,
    pub(crate) cancellation_token: CancellationToken,
    pub(crate) turn_context: Arc<TurnContext>,
    pub(crate) completion: CancellationToken,
    pub(crate) handle: JoinHandle<()>,
}

/// Turn-scoped task registry. A session owns at most one running task.
#[derive(Default)]
pub(crate) struct ActiveTurn {
    pub(crate) task: Option<RunningTask>,
}

impl ActiveTurn {
    pub(crate) fn start(
        &mut self,
        task: Arc<dyn AnySessionTask>,
        cancellation_token: CancellationToken,
        turn_context: Arc<TurnContext>,
        completion: CancellationToken,
        handle: JoinHandle<()>,
    ) -> anyhow::Result<()> {
        if self.task.is_some() {
            handle.abort();
            anyhow::bail!("session already has an active task");
        }
        tracing::debug!(
            kind = ?task.kind(),
            span_name = task.span_name(),
            sub_id = turn_context.sub_id(),
            "session task started"
        );
        self.task = Some(RunningTask {
            kind: task.kind(),
            task,
            cancellation_token,
            turn_context,
            completion,
            handle,
        });
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn finish(&mut self, sub_id: &str) {
        if self
            .task
            .as_ref()
            .is_some_and(|task| task.turn_context.sub_id() == sub_id)
        {
            self.task = None;
        }
    }
}

impl Session {
    /// Start one session-owned task after replacing any previous task.
    pub(crate) async fn spawn_task<T: SessionTask>(
        self: &Arc<Self>,
        turn_context: Arc<TurnContext>,
        input: Vec<TurnInput>,
        task: T,
    ) -> anyhow::Result<()> {
        self.abort_all_tasks(TurnAbortReason::Replaced).await?;

        let task: Arc<dyn AnySessionTask> = Arc::new(task);
        let cancellation_token = CancellationToken::new();
        let completion = CancellationToken::new();
        let installed = Arc::new(Notify::new());
        self.cancel_signal().reset();
        self.bind_turn_context(Arc::clone(&turn_context)).await;

        let session = Arc::clone(self);
        let ctx = Arc::clone(&turn_context);
        let task_for_run = Arc::clone(&task);
        let child = cancellation_token.child_token();
        let cancellation_for_run = cancellation_token.clone();
        let completion_for_run = completion.clone();
        let turn_id = turn_context.sub_id().to_string();
        let turn_id_for_run = turn_id.clone();
        self.task_completions
            .lock()
            .await
            .insert(turn_id_for_run.clone(), completion.clone());
        let installed_for_run = Arc::clone(&installed);
        let handle = tokio::spawn(async move {
            installed_for_run.notified().await;
            let result = task_for_run
                .run(Arc::clone(&session), Arc::clone(&ctx), input, child)
                .await;
            if !cancellation_for_run.is_cancelled() {
                session.on_task_finished(ctx, result).await;
                session
                    .complete_task_lifecycle(&turn_id_for_run, &completion_for_run)
                    .await;
            }
        });

        let installed_result = self
            .install_running_task(
                task,
                cancellation_token.clone(),
                turn_context,
                completion.clone(),
                handle,
            )
            .await;
        if installed_result.is_ok() {
            installed.notify_one();
        } else {
            cancellation_token.cancel();
            self.complete_task_lifecycle(&turn_id, &completion).await;
        }
        installed_result
    }

    async fn install_running_task(
        &self,
        task: Arc<dyn AnySessionTask>,
        cancellation_token: CancellationToken,
        turn_context: Arc<TurnContext>,
        completion: CancellationToken,
        handle: JoinHandle<()>,
    ) -> anyhow::Result<()> {
        let mut active_turn = self.active_turn.lock().await;
        let active_turn = active_turn.get_or_insert_with(ActiveTurn::default);
        active_turn.start(task, cancellation_token, turn_context, completion, handle)
    }

    pub(crate) async fn wait_for_task(&self, turn_id: &str) {
        let completion = self.task_completions.lock().await.get(turn_id).cloned();
        if let Some(completion) = completion {
            completion.cancelled().await;
        }
    }

    async fn complete_task_lifecycle(&self, turn_id: &str, completion: &CancellationToken) {
        completion.cancel();
        self.task_completions.lock().await.remove(turn_id);
    }

    /// Cooperatively abort the active task and wait for its lifecycle to finish.
    pub async fn abort_all_tasks(self: &Arc<Self>, reason: TurnAbortReason) -> anyhow::Result<()> {
        let running = {
            let mut active_turn = self.active_turn.lock().await;
            let running = active_turn.as_mut().and_then(|turn| turn.task.as_mut());
            if let Some(running) = running {
                running.cancellation_token.cancel();
                self.cancel_signal().cancel();
            }
            active_turn.as_mut().and_then(|turn| turn.task.take())
        };
        let Some(mut running) = running else {
            if let Some(turn_id) = self.current_turn_id().await {
                self.wait_for_task(&turn_id).await;
            }
            return Ok(());
        };

        let turn_id = running.turn_context.sub_id().to_string();

        if tokio::time::timeout(TASK_ABORT_TIMEOUT, &mut running.handle)
            .await
            .is_err()
        {
            tracing::warn!(?reason, %turn_id, "forcing session task abort after timeout");
            running.handle.abort();
            if tokio::time::timeout(TASK_ABORT_TIMEOUT, &mut running.handle)
                .await
                .is_err()
            {
                tracing::warn!(?reason, %turn_id, "forced session task did not terminate before timeout");
            }
        }
        let task = Arc::clone(&running.task);
        let session = Arc::clone(self);
        let turn_context = Arc::clone(&running.turn_context);
        let mut abort_hook = tokio::spawn(async move {
            task.abort(session, turn_context).await;
        });
        if tokio::time::timeout(TASK_ABORT_HOOK_TIMEOUT, &mut abort_hook)
            .await
            .is_err()
        {
            abort_hook.abort();
            tracing::warn!(?reason, %turn_id, "session task abort hook timed out");
        }
        self.emit_runtime_event(
            turn_id.clone(),
            agent_protocol::EventMsg::TurnAborted(TurnAbortedEvent {
                turn_id: Some(turn_id.clone()),
                reason,
            }),
        )
        .await;
        if self.current_turn_id().await.as_deref() == Some(&turn_id) {
            self.clear_current_turn_id().await;
        }
        let mut active_turn = self.active_turn.lock().await;
        if active_turn.as_ref().is_some_and(|turn| turn.task.is_none()) {
            *active_turn = None;
        }
        drop(active_turn);
        self.complete_task_lifecycle(&turn_id, &running.completion)
            .await;
        Ok(())
    }

    pub(crate) async fn on_task_finished(
        self: &Arc<Self>,
        turn_context: Arc<TurnContext>,
        result: SessionTaskResult,
    ) {
        if let Err(error) = result {
            tracing::warn!(%error, turn_id = turn_context.sub_id(), "session task failed");
        }
        let turn_id = turn_context.sub_id();
        let mut active_turn = self.active_turn.lock().await;
        if active_turn
            .as_ref()
            .and_then(|turn| turn.task.as_ref())
            .is_some_and(|running| running.turn_context.sub_id() == turn_id)
        {
            *active_turn = None;
        }
        drop(active_turn);
        if self.current_turn_id().await.as_deref() == Some(turn_id) {
            self.clear_current_turn_id().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use agent_rollout::{RolloutRecorder, ThreadHistoryMode};

    use super::*;
    use crate::runtime::{AstroThread, Config};

    struct NoopTask;

    impl SessionTask for NoopTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _turn_context: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            _cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            Ok(None)
        }
    }

    fn turn_context(sub_id: &str) -> Arc<TurnContext> {
        Arc::new(TurnContext::new(
            sub_id.to_string(),
            1,
            types::InteractionMode::Agent,
            None,
            None,
        ))
    }

    #[tokio::test]
    async fn active_turn_allows_only_one_running_task() {
        let mut active_turn = ActiveTurn::default();
        let first: Arc<dyn AnySessionTask> = Arc::new(NoopTask);
        let second: Arc<dyn AnySessionTask> = Arc::new(NoopTask);

        active_turn
            .start(
                first,
                CancellationToken::new(),
                turn_context("turn-1"),
                CancellationToken::new(),
                tokio::spawn(async {}),
            )
            .unwrap();
        assert!(active_turn
            .start(
                second,
                CancellationToken::new(),
                turn_context("turn-2"),
                CancellationToken::new(),
                tokio::spawn(async {}),
            )
            .is_err());

        active_turn.finish("turn-2");
        assert!(active_turn.task.is_some());
        active_turn.finish("turn-1");
        assert!(active_turn.task.is_none());
    }

    #[tokio::test]
    async fn session_owns_a_locked_optional_active_turn() {
        let dir = tempfile::tempdir().unwrap();
        let session = Session::with_session_id(
            crate::runtime::Config::with_defaults(dir.path().to_path_buf()),
            "locked-active-turn".into(),
        )
        .unwrap();

        let active_turn = session.active_turn.lock().await;
        assert!(active_turn.is_none());
    }

    struct PendingTask {
        started: Arc<Notify>,
    }

    impl SessionTask for PendingTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.pending_test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            self.started.notify_one();
            cancellation_token.cancelled().await;
            Ok(None)
        }
    }

    #[tokio::test]
    async fn abort_all_tasks_cancels_and_clears_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                crate::runtime::Config::with_defaults(dir.path().to_path_buf()),
                "abort-task-test".into(),
            )
            .unwrap(),
        );
        let turn_context = session.create_turn_context("turn-abort".into()).await;
        let started = Arc::new(Notify::new());
        let task = PendingTask {
            started: Arc::clone(&started),
        };
        session
            .spawn_task(turn_context, Vec::new(), task)
            .await
            .unwrap();

        started.notified().await;
        session
            .abort_all_tasks(TurnAbortReason::Interrupted)
            .await
            .unwrap();
        assert!(session.active_turn.lock().await.is_none());
    }

    struct OrderedAbortTask {
        run_finished: Arc<AtomicBool>,
        hook_started: Arc<Notify>,
        hook_release: Arc<Notify>,
        hook_saw_run_finished: Arc<AtomicBool>,
    }

    impl SessionTask for OrderedAbortTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.ordered_abort_test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            cancellation_token.cancelled().await;
            self.run_finished.store(true, Ordering::SeqCst);
            Ok(None)
        }

        async fn abort(&self, _session: Arc<Session>, _ctx: Arc<TurnContext>) {
            self.hook_saw_run_finished
                .store(self.run_finished.load(Ordering::SeqCst), Ordering::SeqCst);
            self.hook_started.notify_one();
            self.hook_release.notified().await;
        }
    }

    async fn task_test_thread(name: &str) -> (tempfile::TempDir, Arc<Session>, Arc<AstroThread>) {
        let dir = tempfile::tempdir().unwrap();
        let rollout = RolloutRecorder::open(
            dir.path().join("rollout.jsonl"),
            ThreadHistoryMode::Paginated,
        )
        .await
        .unwrap();
        let session = Arc::new(
            Session::with_session_id(Config::with_defaults(dir.path().to_path_buf()), name.into())
                .unwrap(),
        );
        let thread = AstroThread::spawn(Arc::clone(&session), rollout).unwrap();
        (dir, session, thread)
    }

    #[tokio::test]
    async fn abort_waits_for_run_handle_before_abort_hook_and_clears_after_event() {
        let (_dir, session, thread) = task_test_thread("ordered-abort-test").await;
        let run_finished = Arc::new(AtomicBool::new(false));
        let hook_started = Arc::new(Notify::new());
        let hook_release = Arc::new(Notify::new());
        let hook_saw_run_finished = Arc::new(AtomicBool::new(false));
        let context = session
            .create_turn_context("turn-ordered-abort".into())
            .await;
        session
            .spawn_task(
                context,
                Vec::new(),
                OrderedAbortTask {
                    run_finished: Arc::clone(&run_finished),
                    hook_started: Arc::clone(&hook_started),
                    hook_release: Arc::clone(&hook_release),
                    hook_saw_run_finished: Arc::clone(&hook_saw_run_finished),
                },
            )
            .await
            .unwrap();

        let abort = tokio::spawn({
            let session = Arc::clone(&session);
            async move {
                session
                    .abort_all_tasks(TurnAbortReason::Interrupted)
                    .await
                    .unwrap();
            }
        });
        hook_started.notified().await;
        let wait_for_task = tokio::spawn({
            let session = Arc::clone(&session);
            async move {
                session.wait_for_task("turn-ordered-abort").await;
            }
        });
        tokio::task::yield_now().await;
        assert!(
            !wait_for_task.is_finished(),
            "wait_for_task must remain pending through the abort hook"
        );
        assert!(session.active_turn.lock().await.is_some());
        assert_eq!(
            session.current_turn_id().await.as_deref(),
            Some("turn-ordered-abort")
        );
        hook_release.notify_one();
        abort.await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), wait_for_task)
            .await
            .expect("wait_for_task should finish with the complete abort lifecycle")
            .unwrap();

        assert!(run_finished.load(Ordering::SeqCst));
        assert!(hook_saw_run_finished.load(Ordering::SeqCst));
        let event = thread.next_event().await.unwrap();
        assert!(matches!(
            event.msg,
            agent_protocol::EventMsg::TurnAborted(_)
        ));
        assert!(session.active_turn.lock().await.is_none());
        assert!(session.current_turn_id().await.is_none());
    }

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    struct UncooperativeAbortTask {
        run_dropped: Arc<AtomicBool>,
        hook_called: Arc<AtomicBool>,
        hook_saw_run_dropped: Arc<AtomicBool>,
    }

    impl SessionTask for UncooperativeAbortTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.uncooperative_abort_test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            _cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            let _drop_flag = DropFlag(Arc::clone(&self.run_dropped));
            std::future::pending().await
        }

        async fn abort(&self, _session: Arc<Session>, _ctx: Arc<TurnContext>) {
            self.hook_saw_run_dropped
                .store(self.run_dropped.load(Ordering::SeqCst), Ordering::SeqCst);
            self.hook_called.store(true, Ordering::SeqCst);
            std::future::pending().await
        }
    }

    #[tokio::test(start_paused = true)]
    async fn abort_forces_run_then_bounds_hook_and_emits_one_terminal() {
        let (_dir, session, thread) = task_test_thread("bounded-abort-test").await;
        let run_dropped = Arc::new(AtomicBool::new(false));
        let hook_called = Arc::new(AtomicBool::new(false));
        let hook_saw_run_dropped = Arc::new(AtomicBool::new(false));
        let context = session
            .create_turn_context("turn-bounded-abort".into())
            .await;
        session
            .spawn_task(
                context,
                Vec::new(),
                UncooperativeAbortTask {
                    run_dropped: Arc::clone(&run_dropped),
                    hook_called: Arc::clone(&hook_called),
                    hook_saw_run_dropped: Arc::clone(&hook_saw_run_dropped),
                },
            )
            .await
            .unwrap();

        let abort = tokio::spawn({
            let session = Arc::clone(&session);
            async move {
                session
                    .abort_all_tasks(TurnAbortReason::Interrupted)
                    .await
                    .unwrap();
            }
        });
        tokio::task::yield_now().await;
        assert!(!hook_called.load(Ordering::SeqCst));
        tokio::time::advance(Duration::from_secs(6)).await;
        tokio::task::yield_now().await;
        assert!(run_dropped.load(Ordering::SeqCst));
        assert!(hook_called.load(Ordering::SeqCst));
        assert!(hook_saw_run_dropped.load(Ordering::SeqCst));
        tokio::time::advance(Duration::from_secs(6)).await;
        abort.await.unwrap();

        let event = thread.next_event().await.unwrap();
        assert!(matches!(
            event.msg,
            agent_protocol::EventMsg::TurnAborted(_)
        ));
        session
            .abort_all_tasks(TurnAbortReason::Interrupted)
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(1), thread.next_event())
                .await
                .is_err()
        );
        assert!(session.active_turn.lock().await.is_none());
        assert!(session.current_turn_id().await.is_none());
    }

    struct SyncBlockingAbortHookTask {
        hook_started: Arc<Notify>,
        hook_release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl SessionTask for SyncBlockingAbortHookTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.sync_blocking_abort_hook_test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            cancellation_token.cancelled().await;
            Ok(None)
        }

        async fn abort(&self, _session: Arc<Session>, _ctx: Arc<TurnContext>) {
            self.hook_started.notify_one();
            let _ = self
                .hook_release
                .lock()
                .expect("hook release mutex poisoned")
                .recv();
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn synchronous_abort_hook_cannot_block_interrupt_forever() {
        let (_dir, session, _thread) = task_test_thread("sync-hook-abort-test").await;
        let hook_started = Arc::new(Notify::new());
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let context = session
            .create_turn_context("turn-sync-hook-abort".into())
            .await;
        session
            .spawn_task(
                context,
                Vec::new(),
                SyncBlockingAbortHookTask {
                    hook_started: Arc::clone(&hook_started),
                    hook_release: std::sync::Mutex::new(release_rx),
                },
            )
            .await
            .unwrap();

        let mut abort = tokio::spawn({
            let session = Arc::clone(&session);
            async move {
                session
                    .abort_all_tasks(TurnAbortReason::Interrupted)
                    .await
                    .unwrap();
            }
        });
        hook_started.notified().await;
        let result = tokio::time::timeout(Duration::from_millis(250), &mut abort).await;
        release_tx.send(()).unwrap();
        if result.is_err() {
            abort.await.unwrap();
        }
        assert!(
            result.is_ok(),
            "interrupt must bound a synchronous abort hook"
        );
    }

    struct SyncBlockingRunTask {
        run_started: Arc<Notify>,
        run_release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
        hook_called: Arc<AtomicBool>,
    }

    impl SessionTask for SyncBlockingRunTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.sync_blocking_run_test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            _cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            self.run_started.notify_one();
            tokio::task::block_in_place(|| {
                let _ = self
                    .run_release
                    .lock()
                    .expect("run release mutex poisoned")
                    .recv();
            });
            Ok(None)
        }

        async fn abort(&self, _session: Arc<Session>, _ctx: Arc<TurnContext>) {
            self.hook_called.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn forced_abort_does_not_await_synchronous_run_without_a_bound() {
        let (_dir, session, _thread) = task_test_thread("sync-run-abort-test").await;
        let run_started = Arc::new(Notify::new());
        let hook_called = Arc::new(AtomicBool::new(false));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let context = session
            .create_turn_context("turn-sync-run-abort".into())
            .await;
        session
            .spawn_task(
                context,
                Vec::new(),
                SyncBlockingRunTask {
                    run_started: Arc::clone(&run_started),
                    run_release: std::sync::Mutex::new(release_rx),
                    hook_called: Arc::clone(&hook_called),
                },
            )
            .await
            .unwrap();
        run_started.notified().await;

        let mut abort = tokio::spawn({
            let session = Arc::clone(&session);
            async move {
                session
                    .abort_all_tasks(TurnAbortReason::Interrupted)
                    .await
                    .unwrap();
            }
        });
        let result = tokio::time::timeout(Duration::from_millis(250), &mut abort).await;
        release_tx.send(()).unwrap();
        if result.is_err() {
            abort.await.unwrap();
        }
        assert!(
            result.is_ok(),
            "forced JoinHandle abort must remain bounded"
        );
        assert!(hook_called.load(Ordering::SeqCst));
    }
}
