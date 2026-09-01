//! 可恢复任务生命周期——Session 拥有的单活跃任务调度与中断。

mod compact;
mod regular;
mod review;

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

pub use agent_protocol::TurnInput;
use agent_protocol::{
    ErrorEvent, EventMsg, SuspendTurnOutcome, TurnAbortReason, TurnAbortedEvent, TurnCompleteEvent,
};
use futures::FutureExt;
use tokio::sync::{oneshot, Notify};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::runtime::{Session, TurnContext};

pub(crate) use compact::CompactTask;
pub(crate) use regular::RegularTask;
pub(crate) use review::ReviewTask;

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

/// Turn 被取消时的哨兵错误类型。
#[derive(Debug, thiserror::Error)]
#[error("turn cancelled")]
pub(crate) struct TurnCancelled;

/// 任务类型枚举：Regular / Review / Compact。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Regular,
    Review,
    Compact,
}

/// 驱动单个 session turn 的异步任务 trait。
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

    fn take_auxiliary_handles(&self) -> Vec<JoinHandle<()>> {
        Vec::new()
    }
}

/// SessionTask 的对象安全适配器，供活跃任务注册表使用。
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

    fn take_auxiliary_handles(&self) -> Vec<JoinHandle<()>>;
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

    fn take_auxiliary_handles(&self) -> Vec<JoinHandle<()>> {
        SessionTask::take_auxiliary_handles(self)
    }
}

/// 当前正在运行的任务元数据（kind、取消令牌、JoinHandle 等）。
pub(crate) struct RunningTask {
    pub(crate) kind: TaskKind,
    pub(crate) task: Arc<dyn AnySessionTask>,
    pub(crate) cancellation_token: CancellationToken,
    pub(crate) turn_context: Arc<TurnContext>,
    pub(crate) completion: CancellationToken,
    pub(crate) handle: JoinHandle<()>,
    auxiliary_handles: Vec<JoinHandle<()>>,
}

/// Turn 级任务注册表，保证 session 内最多一个活跃任务。
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
        let auxiliary_handles = task.take_auxiliary_handles();
        self.task = Some(RunningTask {
            kind: task.kind(),
            task,
            cancellation_token,
            turn_context,
            completion,
            handle,
            auxiliary_handles,
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
    /// Detach an unfinished regular turn for recovery by another runtime.
    ///
    /// Unlike interruption, suspension deliberately emits no terminal turn event.
    pub(crate) async fn suspend_active_regular_turn(
        self: &Arc<Self>,
    ) -> anyhow::Result<SuspendTurnOutcome> {
        let _admission = self.task_admission.lock().await;
        {
            let active_turn = self.active_turn.lock().await;
            let Some(running) = active_turn.as_ref().and_then(|turn| turn.task.as_ref()) else {
                return Ok(SuspendTurnOutcome::NotActive);
            };
            if running.kind != TaskKind::Regular {
                return Ok(SuspendTurnOutcome::UnsupportedTask);
            }
            if running.turn_context.has_live_children() {
                return Ok(SuspendTurnOutcome::HasLiveDescendants);
            }
        }

        self.flush_rollout().await?;
        let mut running = {
            let mut active_turn = self.active_turn.lock().await;
            let Some(turn) = active_turn.as_mut() else {
                return Ok(SuspendTurnOutcome::NotActive);
            };
            let Some(running) = turn.task.as_ref() else {
                return Ok(SuspendTurnOutcome::NotActive);
            };
            if running.kind != TaskKind::Regular {
                return Ok(SuspendTurnOutcome::UnsupportedTask);
            }
            if running.turn_context.has_live_children() {
                return Ok(SuspendTurnOutcome::HasLiveDescendants);
            }
            let running = turn
                .task
                .take()
                .ok_or_else(|| anyhow::anyhow!("accepted turn suspension lost its task"))?;
            *active_turn = None;
            running
        };
        let turn_id = running.turn_context.sub_id().to_string();
        running.turn_context.close_input_admission();
        running.cancellation_token.cancel();
        self.cancel_signal().cancel();
        if tokio::time::timeout(TASK_ABORT_TIMEOUT, &mut running.handle)
            .await
            .is_err()
        {
            running.handle.abort();
            let _ = (&mut running.handle).await;
        }
        running.turn_context.wait_for_children().await;
        drop(running.task);
        Self::await_auxiliary_handles(&mut running.auxiliary_handles).await;
        if self.current_turn_id().await.as_deref() == Some(&turn_id) {
            self.clear_current_turn_id().await;
        }
        self.complete_task_lifecycle(&turn_id, &running.completion)
            .await;
        self.flush_rollout().await?;
        Ok(SuspendTurnOutcome::Suspended { turn_id })
    }

    /// 启动一个 session 任务，若已有活跃任务则先中止。
    pub(crate) async fn spawn_task<T: SessionTask>(
        self: &Arc<Self>,
        turn_context: Arc<TurnContext>,
        input: Vec<TurnInput>,
        task: T,
    ) -> anyhow::Result<()> {
        self.spawn_task_inner(turn_context, input, task, async {})
            .await
    }

    pub(crate) async fn spawn_task_with_install_hook<T, F>(
        self: &Arc<Self>,
        turn_context: Arc<TurnContext>,
        input: Vec<TurnInput>,
        task: T,
        install_hook: F,
    ) -> anyhow::Result<()>
    where
        T: SessionTask,
        F: Future<Output = ()>,
    {
        self.spawn_task_inner(turn_context, input, task, install_hook)
            .await
    }

    async fn spawn_task_inner<T, F>(
        self: &Arc<Self>,
        turn_context: Arc<TurnContext>,
        input: Vec<TurnInput>,
        task: T,
        install_hook: F,
    ) -> anyhow::Result<()>
    where
        T: SessionTask,
        F: Future<Output = ()>,
    {
        let _admission = self.task_admission.lock().await;
        anyhow::ensure!(
            !self.runtime_is_shutting_down(),
            "session runtime is shutting down"
        );
        self.abort_all_tasks_inner(TurnAbortReason::Replaced, false)
            .await?;

        let task: Arc<dyn AnySessionTask> = Arc::new(task);
        let cancellation_token = CancellationToken::new();
        let completion = CancellationToken::new();
        let installed = Arc::new(Notify::new());

        let session = Arc::clone(self);
        let ctx = Arc::clone(&turn_context);
        let task_for_run = Arc::clone(&task);
        let child = cancellation_token.child_token();
        let turn_id = turn_context.sub_id().to_string();
        let turn_id_for_run = turn_id.clone();
        let turn_context_for_finish = Arc::clone(&turn_context);
        self.task_completions
            .lock()
            .await
            .insert(turn_id_for_run.clone(), completion.clone());
        let installed_for_run = Arc::clone(&installed);
        let handle = tokio::spawn(async move {
            installed_for_run.notified().await;
            let run_session = Arc::clone(&session);
            let result =
                AssertUnwindSafe(
                    async move { task_for_run.run(run_session, ctx, input, child).await },
                )
                .catch_unwind()
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("session task panicked")));
            debug_assert_eq!(turn_context_for_finish.sub_id(), turn_id_for_run);
            session
                .on_task_finished(turn_context_for_finish, result)
                .await;
        });

        let installed_result = self
            .install_running_task(
                task,
                cancellation_token.clone(),
                Arc::clone(&turn_context),
                completion.clone(),
                handle,
            )
            .await;
        if installed_result.is_ok() {
            self.cancel_signal().reset();
            install_hook.await;
            self.bind_turn_context(turn_context).await;
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

    /// 等待指定 turn 的完整任务生命周期结束。
    pub async fn wait_for_task(&self, turn_id: &str) {
        let completion = self.task_completions.lock().await.get(turn_id).cloned();
        if let Some(completion) = completion {
            completion.cancelled().await;
        }
    }

    pub(crate) async fn terminating_turn_id(&self) -> Option<String> {
        if let Some(running) = self
            .active_turn
            .lock()
            .await
            .as_ref()
            .and_then(|turn| turn.task.as_ref())
        {
            return running
                .cancellation_token
                .is_cancelled()
                .then(|| running.turn_context.sub_id().to_string());
        }
        let turn_id = self.current_turn_id().await?;
        self.task_completions
            .lock()
            .await
            .contains_key(&turn_id)
            .then_some(turn_id)
    }

    async fn complete_task_lifecycle(&self, turn_id: &str, completion: &CancellationToken) {
        completion.cancel();
        self.task_completions.lock().await.remove(turn_id);
    }

    async fn claim_natural_finish(&self, turn_id: &str) -> Option<RunningTask> {
        let mut active_turn = self.active_turn.lock().await;
        let turn = active_turn.as_mut()?;
        let can_claim = turn.task.as_ref().is_some_and(|running| {
            running.turn_context.sub_id() == turn_id && !running.cancellation_token.is_cancelled()
        });
        if !can_claim {
            return None;
        }
        let running = turn.task.take();
        *active_turn = None;
        running
    }

    pub(crate) async fn on_task_finished(
        self: &Arc<Self>,
        turn_context: Arc<TurnContext>,
        result: SessionTaskResult,
    ) {
        let turn_id = turn_context.sub_id().to_string();
        let Some(mut running) = self.claim_natural_finish(&turn_id).await else {
            return;
        };
        running.turn_context.wait_for_children().await;
        drop(running.task);
        drop(running.handle);
        Self::await_auxiliary_handles(&mut running.auxiliary_handles).await;
        if self.current_turn_id().await.as_deref() == Some(&turn_id) {
            self.clear_current_turn_id().await;
        }
        match result {
            Ok(last_agent_message) => {
                self.send_event(
                    &turn_id,
                    EventMsg::TurnComplete(TurnCompleteEvent {
                        turn_id: turn_id.clone(),
                        last_agent_message,
                        error: None,
                    }),
                )
                .await;
            }
            Err(error) if error.downcast_ref::<TurnCancelled>().is_some() => {
                self.send_event(
                    &turn_id,
                    EventMsg::TurnAborted(TurnAbortedEvent {
                        turn_id: Some(turn_id.clone()),
                        reason: TurnAbortReason::Interrupted,
                    }),
                )
                .await;
            }
            Err(error) => {
                tracing::warn!(%error, %turn_id, "session task failed");
                let terminal_error = ErrorEvent {
                    message: error.to_string(),
                    error_type: "internal".into(),
                };
                self.send_event(&turn_id, EventMsg::Error(terminal_error.clone()))
                    .await;
                self.send_event(
                    &turn_id,
                    EventMsg::TurnComplete(TurnCompleteEvent {
                        turn_id: turn_id.clone(),
                        last_agent_message: None,
                        error: Some(terminal_error),
                    }),
                )
                .await;
            }
        }
        if let Err(error) = self.flush_rollout().await {
            tracing::warn!(%error, %turn_id, "failed to flush rollout after task completion");
        }
        self.complete_task_lifecycle(&turn_id, &running.completion)
            .await;
    }

    async fn await_auxiliary_handles(handles: &mut Vec<JoinHandle<()>>) {
        for handle in handles.drain(..) {
            let _ = handle.await;
        }
    }

    /// 协作式中止活跃任务，并等待其生命周期结束。
    pub async fn abort_all_tasks(self: &Arc<Self>, reason: TurnAbortReason) -> anyhow::Result<()> {
        let _admission = self.task_admission.lock().await;
        let emit_interrupt_hook = reason == TurnAbortReason::Interrupted;
        self.abort_all_tasks_inner(reason, emit_interrupt_hook)
            .await
    }

    pub(crate) async fn abort_all_tasks_for_shutdown(
        self: &Arc<Self>,
        reason: TurnAbortReason,
    ) -> (Option<(String, CancellationToken)>, anyhow::Result<()>) {
        let _admission = self.task_admission.lock().await;
        let lifecycle = {
            let active_turn = self.active_turn.lock().await;
            active_turn
                .as_ref()
                .and_then(|turn| turn.task.as_ref())
                .map(|running| {
                    (
                        running.turn_context.sub_id().to_string(),
                        running.completion.clone(),
                    )
                })
        };
        let lifecycle = match lifecycle {
            Some(lifecycle) => Some(lifecycle),
            None => match self.current_turn_id().await {
                Some(turn_id) => self
                    .task_completions
                    .lock()
                    .await
                    .get(&turn_id)
                    .cloned()
                    .map(|completion| (turn_id, completion)),
                None => None,
            },
        };
        let result = self.abort_all_tasks_inner(reason, false).await;
        (lifecycle, result)
    }

    async fn abort_all_tasks_inner(
        self: &Arc<Self>,
        reason: TurnAbortReason,
        emit_interrupt_hook: bool,
    ) -> anyhow::Result<()> {
        let running = {
            let mut active_turn = self.active_turn.lock().await;
            let running = active_turn.as_mut().and_then(|turn| turn.task.as_mut());
            if let Some(running) = running {
                running.cancellation_token.cancel();
                self.cancel_signal().cancel();
            }
            active_turn.as_mut().and_then(|turn| turn.task.take())
        };
        let Some(running) = running else {
            if let Some(turn_id) = self.current_turn_id().await {
                tokio::time::timeout(TASK_ABORT_TIMEOUT, self.wait_for_task(&turn_id))
                    .await
                    .map_err(|_| {
                        anyhow::anyhow!("task {turn_id} termination is still in progress")
                    })?;
            }
            return Ok(());
        };

        let (reply_tx, reply_rx) = oneshot::channel();
        let session = Arc::clone(self);
        tokio::spawn(async move {
            session
                .supervise_abort_lifecycle(running, reason, emit_interrupt_hook, reply_tx)
                .await;
        });
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("task abort supervisor stopped unexpectedly"))?
    }

    async fn supervise_abort_lifecycle(
        self: &Arc<Self>,
        mut running: RunningTask,
        reason: TurnAbortReason,
        emit_interrupt_hook: bool,
        reply_tx: oneshot::Sender<anyhow::Result<()>>,
    ) {
        let turn_id = running.turn_context.sub_id().to_string();
        let mut reply_tx = Some(reply_tx);

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
                tracing::warn!(?reason, %turn_id, "forced session task is still running; supervisor retained ownership");
                Self::report_deferred_abort(
                    &mut reply_tx,
                    anyhow::anyhow!("task {turn_id} did not terminate after forced abort"),
                );
                let _ = (&mut running.handle).await;
            }
        }

        if tokio::time::timeout(TASK_ABORT_TIMEOUT, running.turn_context.wait_for_children())
            .await
            .is_err()
        {
            tracing::warn!(?reason, %turn_id, "session task children are still running; supervisor retained ownership");
            Self::report_deferred_abort(
                &mut reply_tx,
                anyhow::anyhow!("task {turn_id} children did not terminate"),
            );
            running.turn_context.wait_for_children().await;
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
            if tokio::time::timeout(TASK_ABORT_HOOK_TIMEOUT, &mut abort_hook)
                .await
                .is_err()
            {
                tracing::warn!(?reason, %turn_id, "session task abort hook is still running; supervisor retained ownership");
                Self::report_deferred_abort(
                    &mut reply_tx,
                    anyhow::anyhow!("task {turn_id} abort hook did not terminate"),
                );
                let _ = (&mut abort_hook).await;
            }
        }

        drop(running.task);
        Self::await_auxiliary_handles(&mut running.auxiliary_handles).await;
        if reason == TurnAbortReason::Interrupted {
            if let Err(error) = self.record_interrupted_turn_marker().await {
                tracing::warn!(%error, %turn_id, "failed to persist interrupted-turn history marker");
            } else if let Err(error) = self.flush_rollout().await {
                tracing::warn!(%error, %turn_id, "failed to flush interrupted-turn marker before TurnAborted");
            }
        }
        if emit_interrupt_hook && self.subagent_hook_context().is_none() {
            if let Err(error) = self.flush_rollout().await {
                tracing::warn!(%error, %turn_id, "failed to flush rollout before interrupt hook");
            }
            let _ = self.fire_hook(
                ::hooks::INTERRUPT,
                ::hooks::HookPayload {
                    turn_id: Some(turn_id.clone()),
                    reason: Some("interrupted".into()),
                    detail: format!("turn={turn_id}"),
                    ..Default::default()
                },
            );
        }
        self.send_event(
            &turn_id,
            EventMsg::TurnAborted(TurnAbortedEvent {
                turn_id: Some(turn_id.clone()),
                reason,
            }),
        )
        .await;
        if let Err(error) = self.flush_rollout().await {
            tracing::warn!(%error, %turn_id, "failed to flush rollout after task abort");
        }
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
        if let Some(reply_tx) = reply_tx {
            let _ = reply_tx.send(Ok(()));
        }
    }

    fn report_deferred_abort(
        reply_tx: &mut Option<oneshot::Sender<anyhow::Result<()>>>,
        error: anyhow::Error,
    ) {
        if let Some(reply_tx) = reply_tx.take() {
            let _ = reply_tx.send(Err(error));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use agent_protocol::{Event, EventMsg, Op};
    use agent_rollout::RolloutRecorder;

    use super::*;
    use crate::runtime::{AgentStatus, AstroThread, Config};

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
        .await
        .unwrap();

        let active_turn = session.active_turn.lock().await;
        assert!(active_turn.is_none());
    }

    struct PendingTask {
        started: Arc<Notify>,
    }

    struct FailingTask;

    impl SessionTask for FailingTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.failing_test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            _cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            Err(anyhow::anyhow!("provider failed"))
        }
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
            .await
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

    #[tokio::test]
    async fn external_abort_cannot_observe_task_between_install_and_bind() {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                crate::runtime::Config::with_defaults(dir.path().to_path_buf()),
                "install-bind-abort-race".into(),
            )
            .await
            .unwrap(),
        );
        let context = session
            .create_turn_context("turn-install-bind-race".into())
            .await;
        let install_entered = Arc::new(tokio::sync::Barrier::new(2));
        let release_install = Arc::new(tokio::sync::Barrier::new(2));
        let run_finished = Arc::new(AtomicBool::new(false));
        let hook_started = Arc::new(Notify::new());
        let hook_release = Arc::new(Notify::new());
        let hook_saw_run_finished = Arc::new(AtomicBool::new(false));
        let spawn = {
            let session = Arc::clone(&session);
            let install_entered = Arc::clone(&install_entered);
            let release_install = Arc::clone(&release_install);
            let run_finished = Arc::clone(&run_finished);
            let hook_started = Arc::clone(&hook_started);
            let hook_release = Arc::clone(&hook_release);
            let hook_saw_run_finished = Arc::clone(&hook_saw_run_finished);
            tokio::spawn(async move {
                session
                    .spawn_task_with_install_hook(
                        context,
                        Vec::new(),
                        OrderedAbortTask {
                            run_finished,
                            hook_started,
                            hook_release,
                            hook_saw_run_finished,
                        },
                        async move {
                            install_entered.wait().await;
                            release_install.wait().await;
                        },
                    )
                    .await
            })
        };
        install_entered.wait().await;

        let abort = {
            let session = Arc::clone(&session);
            tokio::spawn(async move { session.abort_all_tasks(TurnAbortReason::Interrupted).await })
        };
        tokio::task::yield_now().await;
        assert!(
            !abort.is_finished(),
            "external abort must wait for install + current_turn bind to commit"
        );

        release_install.wait().await;
        spawn.await.unwrap().unwrap();
        hook_started.notified().await;
        assert_eq!(
            session.current_turn_id().await.as_deref(),
            Some("turn-install-bind-race"),
            "abort must observe the exact committed current_turn"
        );
        let completion_wait = {
            let session = Arc::clone(&session);
            tokio::spawn(async move {
                session.wait_for_task("turn-install-bind-race").await;
            })
        };
        tokio::task::yield_now().await;
        assert!(
            !completion_wait.is_finished(),
            "completion wait must remain pending for the installed turn lifecycle"
        );
        hook_release.notify_one();
        abort.await.unwrap().unwrap();
        completion_wait.await.unwrap();
        assert!(session.active_turn.lock().await.is_none());
        assert!(session.current_turn_id().await.is_none());
        assert!(run_finished.load(Ordering::SeqCst));
        assert!(hook_saw_run_finished.load(Ordering::SeqCst));
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
        let rollout = RolloutRecorder::open(dir.path().join("rollout.jsonl"))
            .await
            .unwrap();
        let session = Arc::new(
            Session::with_session_id(Config::with_defaults(dir.path().to_path_buf()), name.into())
                .await
                .unwrap(),
        );
        let thread = AstroThread::spawn(Arc::clone(&session), rollout).unwrap();
        (dir, session, thread)
    }

    async fn collect_terminal(thread: &AstroThread, turn_id: &str) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            let event = thread.next_event().await.unwrap();
            if event.id != turn_id {
                continue;
            }
            let terminal = event.msg.is_terminal();
            events.push(event);
            if terminal {
                return events;
            }
        }
    }

    #[tokio::test]
    async fn unexpected_error_emits_error_then_complete_with_error() {
        let (_dir, session, thread) = task_test_thread("task-failure-test").await;
        let turn_id = "turn-failure";
        let context = session.create_turn_context(turn_id.into()).await;
        session
            .spawn_task(context, Vec::new(), FailingTask)
            .await
            .unwrap();
        let events = collect_terminal(&thread, turn_id).await;
        assert!(matches!(events[events.len() - 2].msg, EventMsg::Error(_)));
        assert!(matches!(
            events.last().unwrap().msg,
            EventMsg::TurnComplete(ref event) if event.error.is_some()
        ));
        assert_eq!(
            events
                .iter()
                .filter(|event| event.msg.is_terminal())
                .count(),
            1
        );
        assert_eq!(thread.status(), AgentStatus::Idle);
    }

    #[tokio::test]
    async fn interrupt_emits_only_turn_aborted() {
        let (_dir, session, thread) = task_test_thread("task-interrupt-test").await;
        let turn_id = "turn-interrupt";
        let started = Arc::new(Notify::new());
        let context = session.create_turn_context(turn_id.into()).await;
        session
            .spawn_task(
                context,
                Vec::new(),
                PendingTask {
                    started: Arc::clone(&started),
                },
            )
            .await
            .unwrap();
        started.notified().await;
        thread.submit(Op::Interrupt).await.unwrap();
        let events = collect_terminal(&thread, turn_id).await;
        assert!(matches!(
            events.last().unwrap().msg,
            EventMsg::TurnAborted(_)
        ));
        assert_eq!(
            events
                .iter()
                .filter(|event| event.msg.is_terminal())
                .count(),
            1
        );
        let history = session.clone_response_history().await;
        assert!(matches!(
            history.last(),
            Some(agent_protocol::ResponseItem::Message { role, content, .. })
                if role == "developer"
                    && matches!(content.first(), Some(agent_protocol::ContentItem::InputText { text }) if text.contains("<turn_aborted>"))
        ));
        assert_eq!(thread.status(), AgentStatus::Idle);
    }

    #[tokio::test]
    async fn cleaning_background_terminals_does_not_interrupt_the_active_turn() {
        let (_dir, session, thread) = task_test_thread("task-clean-background-test").await;
        let started = Arc::new(Notify::new());
        let context = session
            .create_turn_context("turn-clean-background".into())
            .await;
        session
            .spawn_task(
                context,
                Vec::new(),
                PendingTask {
                    started: Arc::clone(&started),
                },
            )
            .await
            .unwrap();
        started.notified().await;

        thread.submit(Op::CleanBackgroundTerminals).await.unwrap();
        tokio::task::yield_now().await;
        assert!(session.active_turn.lock().await.is_some());

        thread.submit(Op::Interrupt).await.unwrap();
        let _ = collect_terminal(&thread, "turn-clean-background").await;
    }

    #[tokio::test]
    async fn interrupt_fires_root_hook_once_with_turn_context() {
        let (_dir, session, thread) = task_test_thread("task-interrupt-hook-test").await;
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let bus = Arc::new(hooks::PluginHookBus::new());
        let observed_for_hook = Arc::clone(&observed);
        bus.register(hooks::INTERRUPT, move |payload| {
            observed_for_hook.lock().unwrap().push((
                payload.hook_event_name.clone(),
                payload.turn_id.clone(),
                payload.reason.clone(),
            ));
            hooks::HookOutcome::Continue
        });
        session.set_hook_bus(bus);

        let turn_id = "turn-interrupt-hook";
        let started = Arc::new(Notify::new());
        let context = session.create_turn_context(turn_id.into()).await;
        session
            .spawn_task(
                context,
                Vec::new(),
                PendingTask {
                    started: Arc::clone(&started),
                },
            )
            .await
            .unwrap();
        started.notified().await;

        thread.submit(Op::Interrupt).await.unwrap();
        let events = collect_terminal(&thread, turn_id).await;
        assert!(matches!(
            events.last().unwrap().msg,
            EventMsg::TurnAborted(_)
        ));
        assert_eq!(
            observed.lock().unwrap().as_slice(),
            &[(
                hooks::INTERRUPT.to_string(),
                Some(turn_id.to_string()),
                Some("interrupted".to_string()),
            )]
        );
    }

    #[tokio::test]
    async fn replacing_a_task_does_not_fire_interrupt_hook() {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                crate::runtime::Config::with_defaults(dir.path().to_path_buf()),
                "task-replacement-hook-test".into(),
            )
            .await
            .unwrap(),
        );
        let interrupt_count = Arc::new(AtomicUsize::new(0));
        let bus = Arc::new(hooks::PluginHookBus::new());
        let interrupt_count_for_hook = Arc::clone(&interrupt_count);
        bus.register(hooks::INTERRUPT, move |_| {
            interrupt_count_for_hook.fetch_add(1, Ordering::SeqCst);
            hooks::HookOutcome::Continue
        });
        session.set_hook_bus(bus);

        let first_started = Arc::new(Notify::new());
        session
            .spawn_task(
                session.create_turn_context("turn-replaced".into()).await,
                Vec::new(),
                PendingTask {
                    started: Arc::clone(&first_started),
                },
            )
            .await
            .unwrap();
        first_started.notified().await;
        session
            .spawn_task(
                session.create_turn_context("turn-replacement".into()).await,
                Vec::new(),
                NoopTask,
            )
            .await
            .unwrap();
        session.wait_for_task("turn-replacement").await;

        assert_eq!(interrupt_count.load(Ordering::SeqCst), 0);
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

    struct CompletionAbortRaceTask {
        about_to_return: Arc<Notify>,
        release_return: Arc<Notify>,
        hook_started: Arc<Notify>,
        hook_release: Arc<Notify>,
    }

    impl SessionTask for CompletionAbortRaceTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.completion_abort_race_test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            _cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            self.about_to_return.notify_one();
            self.release_return.notified().await;
            Ok(None)
        }

        async fn abort(&self, _session: Arc<Session>, _ctx: Arc<TurnContext>) {
            self.hook_started.notify_one();
            self.hook_release.notified().await;
        }
    }

    #[tokio::test]
    async fn abort_and_natural_finish_atomically_claim_one_terminal_owner() {
        let (_dir, session, thread) = task_test_thread("finish-abort-race-test").await;
        let about_to_return = Arc::new(Notify::new());
        let release_return = Arc::new(Notify::new());
        let hook_started = Arc::new(Notify::new());
        let hook_release = Arc::new(Notify::new());
        let context = session
            .create_turn_context("turn-finish-abort-race".into())
            .await;
        session
            .spawn_task(
                context,
                Vec::new(),
                CompletionAbortRaceTask {
                    about_to_return: Arc::clone(&about_to_return),
                    release_return: Arc::clone(&release_return),
                    hook_started: Arc::clone(&hook_started),
                    hook_release: Arc::clone(&hook_release),
                },
            )
            .await
            .unwrap();
        about_to_return.notified().await;

        let active_turn_guard = session.active_turn.lock().await;
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
        release_return.notify_one();
        tokio::task::yield_now().await;
        drop(active_turn_guard);

        hook_started.notified().await;
        let wait_for_task = tokio::spawn({
            let session = Arc::clone(&session);
            async move {
                session.wait_for_task("turn-finish-abort-race").await;
            }
        });
        tokio::task::yield_now().await;
        assert!(
            !wait_for_task.is_finished(),
            "the losing natural finish must not complete the abort lifecycle"
        );
        assert_eq!(
            session.current_turn_id().await.as_deref(),
            Some("turn-finish-abort-race"),
            "the losing natural finish must not clear the abort owner's turn"
        );
        hook_release.notify_one();
        abort.await.unwrap();
        wait_for_task.await.unwrap();

        let event = thread.next_event().await.unwrap();
        assert!(matches!(
            event.msg,
            agent_protocol::EventMsg::TurnAborted(_)
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(1), thread.next_event())
                .await
                .is_err(),
            "the race must produce exactly one terminal abort event"
        );
    }

    struct PanicTask;

    impl SessionTask for PanicTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.panic_test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            _cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            panic!("scripted session task panic")
        }
    }

    #[tokio::test]
    async fn panicking_task_completes_lifecycle_and_releases_active_turn() {
        let (_dir, session, _thread) = task_test_thread("panic-task-test").await;
        let context = session.create_turn_context("turn-panic".into()).await;
        session
            .spawn_task(context, Vec::new(), PanicTask)
            .await
            .unwrap();

        tokio::time::timeout(
            Duration::from_millis(250),
            session.wait_for_task("turn-panic"),
        )
        .await
        .expect("task panic must still complete the task lifecycle");
        assert!(session.active_turn.lock().await.is_none());
        assert!(session.current_turn_id().await.is_none());
    }

    struct AuxiliaryDrainTask {
        auxiliary: std::sync::Mutex<Option<JoinHandle<()>>>,
    }

    impl SessionTask for AuxiliaryDrainTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.auxiliary_drain_test"
        }

        fn take_auxiliary_handles(&self) -> Vec<JoinHandle<()>> {
            self.auxiliary
                .lock()
                .expect("auxiliary mutex poisoned")
                .take()
                .into_iter()
                .collect()
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            _cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            Ok(None)
        }
    }

    #[tokio::test]
    async fn task_completion_waits_for_its_internal_event_drain() {
        let (_dir, session, _thread) = task_test_thread("auxiliary-drain-test").await;
        let release = Arc::new(Notify::new());
        let auxiliary = tokio::spawn({
            let release = Arc::clone(&release);
            async move { release.notified().await }
        });
        let context = session
            .create_turn_context("turn-auxiliary-drain".into())
            .await;
        session
            .spawn_task(
                context,
                Vec::new(),
                AuxiliaryDrainTask {
                    auxiliary: std::sync::Mutex::new(Some(auxiliary)),
                },
            )
            .await
            .unwrap();

        let wait_for_task = tokio::spawn({
            let session = Arc::clone(&session);
            async move { session.wait_for_task("turn-auxiliary-drain").await }
        });
        tokio::task::yield_now().await;
        assert!(
            !wait_for_task.is_finished(),
            "completion must not outlive an internal event drain"
        );
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(1), wait_for_task)
            .await
            .expect("completion should follow the event drain")
            .unwrap();
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

    #[tokio::test]
    async fn abort_forces_run_then_bounds_hook_and_emits_one_terminal() {
        let (_dir, session, thread) = task_test_thread("bounded-abort-test").await;
        tokio::time::pause();
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
    async fn synchronous_abort_hook_defers_terminal_until_the_hook_really_ends() {
        let (_dir, session, thread) = task_test_thread("sync-hook-abort-test").await;
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

        let abort = tokio::spawn({
            let session = Arc::clone(&session);
            async move { session.abort_all_tasks(TurnAbortReason::Interrupted).await }
        });
        hook_started.notified().await;
        let result = tokio::time::timeout(Duration::from_millis(250), abort)
            .await
            .expect("interrupt must return a bounded result")
            .unwrap();
        assert!(result.is_err(), "a still-running hook must be reported");
        let wait_for_task = tokio::spawn({
            let session = Arc::clone(&session);
            async move { session.wait_for_task("turn-sync-hook-abort").await }
        });
        tokio::task::yield_now().await;
        assert!(!wait_for_task.is_finished());
        assert_eq!(
            session.current_turn_id().await.as_deref(),
            Some("turn-sync-hook-abort")
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(10), thread.next_event())
                .await
                .is_err()
        );

        release_tx.send(()).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), thread.next_event())
            .await
            .expect("terminal event should follow the real hook exit")
            .unwrap();
        assert!(matches!(
            event.msg,
            agent_protocol::EventMsg::TurnAborted(_)
        ));
        tokio::time::timeout(Duration::from_secs(1), wait_for_task)
            .await
            .expect("completion should follow the real hook exit")
            .unwrap();
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
    async fn shutdown_waits_for_real_task_completion_and_is_shared_by_all_callers() {
        let (_dir, session, thread) = task_test_thread("sync-run-shutdown-test").await;
        let finalize_hits = Arc::new(AtomicUsize::new(0));
        let finalize_counter = Arc::clone(&finalize_hits);
        session.hook_bus().register(::hooks::SESSION_END, move |_| {
            finalize_counter.fetch_add(1, Ordering::SeqCst);
            ::hooks::HookOutcome::Continue
        });
        let run_started = Arc::new(Notify::new());
        let hook_called = Arc::new(AtomicBool::new(false));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let context = session
            .create_turn_context("turn-sync-run-shutdown".into())
            .await;
        session
            .spawn_task(
                context,
                Vec::new(),
                SyncBlockingRunTask {
                    run_started: Arc::clone(&run_started),
                    run_release: std::sync::Mutex::new(release_rx),
                    hook_called,
                },
            )
            .await
            .unwrap();
        run_started.notified().await;

        thread.submit(agent_protocol::Op::Shutdown).await.unwrap();
        while !session.cancel_signal().is_cancelled() {
            tokio::task::yield_now().await;
        }
        let concurrent_shutdown = tokio::spawn({
            let session = Arc::clone(&session);
            async move { session.shutdown_runtime().await }
        });

        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_ne!(thread.status(), AgentStatus::Shutdown);
        assert!(!concurrent_shutdown.is_finished());
        assert!(
            tokio::time::timeout(Duration::from_millis(10), thread.wait_terminated())
                .await
                .is_err(),
            "thread termination must wait for the blocked run to really exit"
        );

        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), concurrent_shutdown)
            .await
            .expect("all shutdown callers should observe the shared completion")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), thread.wait_terminated())
            .await
            .expect("thread should terminate after the task lifecycle completes");
        assert_eq!(thread.status(), AgentStatus::Shutdown);
        assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn shutdown_captures_task_installed_before_turn_context_bind() {
        let (_dir, session, thread) = task_test_thread("install-bind-shutdown-test").await;
        let install_reached = Arc::new(Notify::new());
        let release_install = Arc::new(Notify::new());
        let run_started = Arc::new(Notify::new());
        let hook_called = Arc::new(AtomicBool::new(false));
        let (release_run_tx, release_run_rx) = std::sync::mpsc::channel();
        let spawn = tokio::spawn({
            let session = Arc::clone(&session);
            let install_reached = Arc::clone(&install_reached);
            let release_install = Arc::clone(&release_install);
            let run_started = Arc::clone(&run_started);
            async move {
                let context = session
                    .create_turn_context("turn-install-bind-shutdown".into())
                    .await;
                session
                    .spawn_task_with_install_hook(
                        context,
                        Vec::new(),
                        SyncBlockingRunTask {
                            run_started,
                            run_release: std::sync::Mutex::new(release_run_rx),
                            hook_called,
                        },
                        async move {
                            install_reached.notify_one();
                            release_install.notified().await;
                        },
                    )
                    .await
            }
        });
        install_reached.notified().await;

        thread.submit(agent_protocol::Op::Shutdown).await.unwrap();
        tokio::task::yield_now().await;
        assert_ne!(thread.status(), AgentStatus::Shutdown);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), thread.wait_terminated())
                .await
                .is_err(),
            "shutdown must wait behind install-before-bind admission"
        );

        release_install.notify_one();
        run_started.notified().await;
        while !session.cancel_signal().is_cancelled() {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_ne!(thread.status(), AgentStatus::Shutdown);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), thread.wait_terminated())
                .await
                .is_err(),
            "bounded abort failure must retain the precise installed lifecycle"
        );

        release_run_tx.send(()).unwrap();
        spawn.await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(1), thread.wait_terminated())
            .await
            .expect("shutdown should finish after the installed lifecycle completes");
        assert_eq!(thread.status(), AgentStatus::Shutdown);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancelled_shutdown_owner_does_not_cancel_session_owned_teardown() {
        let (_dir, session, thread) = task_test_thread("cancelled-shutdown-owner-test").await;
        let finalize_hits = Arc::new(AtomicUsize::new(0));
        let finalize_counter = Arc::clone(&finalize_hits);
        session.hook_bus().register(::hooks::SESSION_END, move |_| {
            finalize_counter.fetch_add(1, Ordering::SeqCst);
            ::hooks::HookOutcome::Continue
        });
        let run_started = Arc::new(Notify::new());
        let hook_called = Arc::new(AtomicBool::new(false));
        let (release_run_tx, release_run_rx) = std::sync::mpsc::channel();
        let context = session
            .create_turn_context("turn-cancelled-shutdown-owner".into())
            .await;
        session
            .spawn_task(
                context,
                Vec::new(),
                SyncBlockingRunTask {
                    run_started: Arc::clone(&run_started),
                    run_release: std::sync::Mutex::new(release_run_rx),
                    hook_called,
                },
            )
            .await
            .unwrap();
        run_started.notified().await;

        let owner = tokio::spawn({
            let session = Arc::clone(&session);
            async move { session.shutdown_runtime().await }
        });
        while !session.cancel_signal().is_cancelled() {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!owner.is_finished());
        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());

        release_run_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), session.shutdown_runtime())
            .await
            .expect("a later caller must observe the detached teardown completion");
        assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);

        thread.submit(agent_protocol::Op::Shutdown).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), thread.wait_terminated())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn terminating_task_rejects_new_turn_without_blocking_actor_controls() {
        let (_dir, session, thread) = task_test_thread("terminating-control-test").await;
        let run_started = Arc::new(Notify::new());
        let hook_called = Arc::new(AtomicBool::new(false));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let context = session
            .create_turn_context("turn-terminating-control".into())
            .await;
        session
            .spawn_task(
                context,
                Vec::new(),
                SyncBlockingRunTask {
                    run_started: Arc::clone(&run_started),
                    run_release: std::sync::Mutex::new(release_rx),
                    hook_called,
                },
            )
            .await
            .unwrap();
        run_started.notified().await;

        thread.submit(agent_protocol::Op::Interrupt).await.unwrap();
        let abort_error = tokio::time::timeout(Duration::from_millis(250), thread.next_event())
            .await
            .expect("interrupt should return its bounded abort error")
            .unwrap();
        assert!(matches!(
            abort_error.msg,
            agent_protocol::EventMsg::Error(_)
        ));

        let (_, result) = tokio::time::timeout(
            Duration::from_millis(250),
            thread.submit_turn(
                agent_protocol::TurnInputRequest {
                    input: vec![TurnInput {
                        content: "must not start yet".into(),
                        image_data_urls: Vec::new(),
                        client_message_id: None,
                    }],
                    thread_settings: Default::default(),
                },
                agent_protocol::TurnInputMode::StartIfIdle,
            ),
        )
        .await
        .expect("terminating admission must not block the submission actor")
        .unwrap();
        assert!(matches!(
            result,
            agent_protocol::TurnInputSubmission::NotSubmitted { ref reason }
                if reason == "terminating"
        ));
        let (_, result) = tokio::time::timeout(
            Duration::from_millis(250),
            thread.submit_turn(
                agent_protocol::TurnInputRequest {
                    input: vec![TurnInput {
                        content: "must not steer a detached terminating task".into(),
                        image_data_urls: Vec::new(),
                        client_message_id: None,
                    }],
                    thread_settings: Default::default(),
                },
                agent_protocol::TurnInputMode::StartOrSteer,
            ),
        )
        .await
        .expect("start-or-steer must also reject a terminating lifecycle promptly")
        .unwrap();
        assert!(matches!(
            result,
            agent_protocol::TurnInputSubmission::NotSubmitted { ref reason }
                if reason == "terminating"
        ));

        thread
            .submit(agent_protocol::Op::EmitExtension {
                item: agent_protocol::ExtensionItem {
                    id: "control-probe".into(),
                    namespace: "control_probe".into(),
                    payload: serde_json::json!({"responsive": true}),
                },
                turn_id: None,
            })
            .await
            .unwrap();
        let extension = tokio::time::timeout(Duration::from_millis(250), thread.next_event())
            .await
            .expect("extension control must remain responsive")
            .unwrap();
        assert!(matches!(
            extension.msg,
            agent_protocol::EventMsg::ItemCompleted(_)
        ));

        release_tx.send(()).unwrap();
        session.wait_for_task("turn-terminating-control").await;
        thread.submit(agent_protocol::Op::Shutdown).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), thread.wait_terminated())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn synchronous_run_keeps_lifecycle_owned_until_the_run_really_ends() {
        let (_dir, session, thread) = task_test_thread("sync-run-abort-test").await;
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

        thread.submit(agent_protocol::Op::Interrupt).await.unwrap();
        let error = tokio::time::timeout(Duration::from_millis(250), thread.next_event())
            .await
            .expect("submission interrupt must report a bounded abort error")
            .unwrap();
        assert!(matches!(error.msg, agent_protocol::EventMsg::Error(_)));
        assert!(!hook_called.load(Ordering::SeqCst));

        let wait_for_task = tokio::spawn({
            let session = Arc::clone(&session);
            async move { session.wait_for_task("turn-sync-run-abort").await }
        });
        let next_task = tokio::spawn({
            let session = Arc::clone(&session);
            async move {
                let context = session
                    .create_turn_context("turn-after-sync-run".into())
                    .await;
                session.spawn_task(context, Vec::new(), NoopTask).await
            }
        });
        tokio::task::yield_now().await;
        assert!(!wait_for_task.is_finished());
        assert!(
            !next_task.is_finished(),
            "a new turn must wait for old side effects"
        );
        assert_eq!(
            session.current_turn_id().await.as_deref(),
            Some("turn-sync-run-abort")
        );

        release_tx.send(()).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), thread.next_event())
            .await
            .expect("terminal event should follow the real run exit")
            .unwrap();
        assert!(matches!(
            event.msg,
            agent_protocol::EventMsg::TurnAborted(_)
        ));
        tokio::time::timeout(Duration::from_secs(1), wait_for_task)
            .await
            .expect("completion should follow the real run exit")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), next_task)
            .await
            .expect("next turn should start after old lifecycle completion")
            .unwrap()
            .unwrap();
        assert!(hook_called.load(Ordering::SeqCst));
    }

    struct SpawnBlockingToolChildTask {
        child_started: Arc<Notify>,
        child_release: Arc<std::sync::Mutex<std::sync::mpsc::Receiver<()>>>,
    }

    impl SessionTask for SpawnBlockingToolChildTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.spawn_blocking_tool_child_test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            _cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            let child_permit = ctx.track_child();
            let child_started = Arc::clone(&self.child_started);
            let child_release = Arc::clone(&self.child_release);
            tokio::task::spawn_blocking(move || {
                let _child_permit = child_permit;
                child_started.notify_one();
                let _ = child_release
                    .lock()
                    .expect("child release mutex poisoned")
                    .recv();
            })
            .await?;
            Ok(None)
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn spawn_blocking_tool_child_keeps_abort_lifecycle_until_real_exit() {
        let (_dir, session, thread) = task_test_thread("spawn-blocking-child-test").await;
        let child_started = Arc::new(Notify::new());
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let context = session
            .create_turn_context("turn-spawn-blocking-child".into())
            .await;
        session
            .spawn_task(
                context,
                Vec::new(),
                SpawnBlockingToolChildTask {
                    child_started: Arc::clone(&child_started),
                    child_release: Arc::new(std::sync::Mutex::new(release_rx)),
                },
            )
            .await
            .unwrap();
        child_started.notified().await;

        thread.submit(agent_protocol::Op::Interrupt).await.unwrap();
        let error = tokio::time::timeout(Duration::from_millis(250), thread.next_event())
            .await
            .expect("blocking child should produce a bounded abort error")
            .unwrap();
        assert!(matches!(error.msg, agent_protocol::EventMsg::Error(_)));

        let wait_for_task = tokio::spawn({
            let session = Arc::clone(&session);
            async move { session.wait_for_task("turn-spawn-blocking-child").await }
        });
        let next_task = tokio::spawn({
            let session = Arc::clone(&session);
            async move {
                let context = session
                    .create_turn_context("turn-after-blocking-child".into())
                    .await;
                session.spawn_task(context, Vec::new(), NoopTask).await
            }
        });
        tokio::task::yield_now().await;
        assert!(!wait_for_task.is_finished());
        assert!(!next_task.is_finished());
        assert!(
            tokio::time::timeout(Duration::from_millis(10), thread.next_event())
                .await
                .is_err()
        );

        release_tx.send(()).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), thread.next_event())
            .await
            .expect("terminal should follow the real blocking child exit")
            .unwrap();
        assert!(matches!(
            event.msg,
            agent_protocol::EventMsg::TurnAborted(_)
        ));
        tokio::time::timeout(Duration::from_secs(1), wait_for_task)
            .await
            .expect("completion should follow the real blocking child exit")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), next_task)
            .await
            .expect("next turn should start after the blocking child exits")
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn concurrent_spawn_admission_serializes_replace_install_and_bind() {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "concurrent-spawn-admission".into(),
            )
            .await
            .unwrap(),
        );
        let active_turn_guard = session.active_turn.lock().await;

        let first = tokio::spawn({
            let session = Arc::clone(&session);
            async move {
                let context = session.create_turn_context("turn-admission-1".into()).await;
                session
                    .spawn_task(
                        context,
                        Vec::new(),
                        PendingTask {
                            started: Arc::new(Notify::new()),
                        },
                    )
                    .await
            }
        });
        tokio::task::yield_now().await;
        let second = tokio::spawn({
            let session = Arc::clone(&session);
            async move {
                let context = session.create_turn_context("turn-admission-2".into()).await;
                session
                    .spawn_task(
                        context,
                        Vec::new(),
                        PendingTask {
                            started: Arc::new(Notify::new()),
                        },
                    )
                    .await
            }
        });
        tokio::task::yield_now().await;
        drop(active_turn_guard);

        tokio::time::timeout(Duration::from_secs(1), first)
            .await
            .expect("first admission should finish")
            .unwrap()
            .expect("first admission should install before replacement");
        tokio::time::timeout(Duration::from_secs(1), second)
            .await
            .expect("second admission should finish")
            .unwrap()
            .expect("second admission should atomically replace the first");

        let active_turn_id = session
            .active_turn
            .lock()
            .await
            .as_ref()
            .and_then(|turn| turn.task.as_ref())
            .map(|running| running.turn_context.sub_id().to_string());
        assert_eq!(active_turn_id.as_deref(), Some("turn-admission-2"));
        assert_eq!(
            session.current_turn_id().await.as_deref(),
            active_turn_id.as_deref(),
            "the losing admission must never overwrite the installed turn context"
        );

        session
            .abort_all_tasks(TurnAbortReason::Interrupted)
            .await
            .unwrap();
    }
}
