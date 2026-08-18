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

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::runtime::{Session, TurnContext};

pub(crate) use regular::RegularTask;

pub(crate) type SessionTaskResult = anyhow::Result<Option<String>>;
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
const TASK_ABORT_TIMEOUT: Duration = Duration::from_secs(5);

/// Why an active session task was aborted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnAbortReason {
    Interrupted,
    Replaced,
    ReviewEnded,
    BudgetLimited,
}

/// Input submitted to a session task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnInput {
    UserInput {
        content: String,
        image_data_urls: Vec<String>,
    },
}

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
    pub(crate) done: Arc<Notify>,
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
        done: Arc<Notify>,
    ) -> anyhow::Result<()> {
        if self.task.is_some() {
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
            done,
        });
        Ok(())
    }

    pub(crate) fn finish(&mut self, sub_id: &str) {
        if let Some(task) = self
            .task
            .as_ref()
            .filter(|task| task.turn_context.sub_id() == sub_id)
        {
            tracing::debug!(
                kind = ?task.kind,
                span_name = task.task.span_name(),
                cancelled = task.cancellation_token.is_cancelled(),
                sub_id,
                "session task finished"
            );
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
    ) -> SessionTaskResult {
        self.abort_all_tasks(TurnAbortReason::Replaced).await?;

        let task: Arc<dyn AnySessionTask> = Arc::new(task);
        let cancellation_token = CancellationToken::new();
        let done = Arc::new(Notify::new());
        {
            let mut active_turn = self.active_turn.lock().await;
            let active_turn = active_turn.get_or_insert_with(ActiveTurn::default);
            active_turn.start(
                Arc::clone(&task),
                cancellation_token.clone(),
                Arc::clone(&turn_context),
                Arc::clone(&done),
            )?;
            // Keep registration and turn binding atomic with respect to abort/replace.
            // A competing task must not overwrite the winning task's context.
            self.cancel_signal().reset();
            self.bind_turn_context(Arc::clone(&turn_context)).await;
        }

        let task_result = Arc::clone(&task)
            .run(
                Arc::clone(self),
                Arc::clone(&turn_context),
                input,
                cancellation_token.child_token(),
            )
            .await;

        done.notify_one();
        {
            let mut active_turn = self.active_turn.lock().await;
            if let Some(turn) = active_turn.as_mut() {
                turn.finish(turn_context.sub_id());
                if turn.task.is_none() {
                    *active_turn = None;
                }
            }
        }
        if self.current_turn_id().await.as_deref() == Some(turn_context.sub_id()) {
            self.clear_current_turn_id().await;
        }
        task_result
    }

    /// Cooperatively abort the active task and wait for its lifecycle to finish.
    pub async fn abort_all_tasks(self: &Arc<Self>, reason: TurnAbortReason) -> anyhow::Result<()> {
        let active = {
            let active_turn = self.active_turn.lock().await;
            active_turn
                .as_ref()
                .and_then(|turn| turn.task.as_ref())
                .map(|running| {
                    (
                        Arc::clone(&running.task),
                        running.cancellation_token.clone(),
                        Arc::clone(&running.turn_context),
                        Arc::clone(&running.done),
                    )
                })
        };
        let Some((task, cancellation_token, turn_context, done)) = active else {
            return Ok(());
        };

        let notified = done.notified();
        cancellation_token.cancel();
        self.cancel_signal().cancel();
        task.abort(Arc::clone(self), Arc::clone(&turn_context))
            .await;

        tokio::time::timeout(TASK_ABORT_TIMEOUT, notified)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "timed out aborting {:?} task {}",
                    reason,
                    turn_context.sub_id()
                )
            })?;

        {
            let mut active_turn = self.active_turn.lock().await;
            if let Some(turn) = active_turn.as_mut() {
                turn.finish(turn_context.sub_id());
                if turn.task.is_none() {
                    *active_turn = None;
                }
            }
        }
        if self.current_turn_id().await.as_deref() == Some(turn_context.sub_id()) {
            self.clear_current_turn_id().await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn active_turn_allows_only_one_running_task() {
        let mut active_turn = ActiveTurn::default();
        let first: Arc<dyn AnySessionTask> = Arc::new(NoopTask);
        let second: Arc<dyn AnySessionTask> = Arc::new(NoopTask);

        active_turn
            .start(
                first,
                CancellationToken::new(),
                turn_context("turn-1"),
                Arc::new(Notify::new()),
            )
            .unwrap();
        assert!(active_turn
            .start(
                second,
                CancellationToken::new(),
                turn_context("turn-2"),
                Arc::new(Notify::new()),
            )
            .is_err());

        active_turn.finish("turn-2");
        assert!(active_turn.task.is_some());
        active_turn.finish("turn-1");
        assert!(active_turn.task.is_none());
    }

    #[test]
    fn session_task_lifecycle_is_callable_through_arc_session() {
        fn assert_arc_session_api(
            session: Arc<Session>,
            turn_context: Arc<TurnContext>,
            task: NoopTask,
        ) {
            drop(session.spawn_task(turn_context, Vec::new(), task));
            drop(session.abort_all_tasks(TurnAbortReason::Interrupted));
        }

        let _ = assert_arc_session_api;
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
        let run = tokio::spawn({
            let session = Arc::clone(&session);
            async move {
                session
                    .spawn_task(turn_context, Vec::new(), task)
                    .await
                    .unwrap();
            }
        });

        started.notified().await;
        session
            .abort_all_tasks(TurnAbortReason::Interrupted)
            .await
            .unwrap();
        run.await.unwrap();
        assert!(session.active_turn.lock().await.is_none());
    }
}
