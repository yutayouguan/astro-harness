//! Turn 级不可变运行时状态，一个用户轮次内保持固定。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::tasks::TurnInput;

#[derive(Debug)]
struct PendingInputSignal {
    mailbox_message_id: String,
}

/// 排队等待注入当前 turn 的用户输入及可选上下文。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct QueuedTurnInput {
    pub(crate) input: TurnInput,
    pub(crate) inject_context: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnInputReadiness {
    Preparing,
    Accepting,
    Closed,
}

#[derive(Debug)]
struct TurnInputState {
    pending: Vec<QueuedTurnInput>,
    mailbox_pending: Vec<PendingInputSignal>,
    readiness: TurnInputReadiness,
    in_flight_admissions: usize,
}

/// 输入准入预留凭证，drop 时自动释放 in-flight 计数。
pub(crate) struct TurnInputReservation {
    turn_context: Arc<TurnContext>,
    finished: bool,
}

#[derive(Debug, Default)]
struct ChildTracker {
    active: AtomicUsize,
    changed: Notify,
}

/// 子任务存活凭证，drop 时递减活跃计数并通知等待者。
#[derive(Debug)]
pub(crate) struct ChildPermit {
    tracker: Arc<ChildTracker>,
}

impl Drop for ChildPermit {
    fn drop(&mut self) {
        if self.tracker.active.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.tracker.changed.notify_one();
        }
    }
}

/// 单个用户 turn 内所有采样步骤共享的不可变上下文。
#[derive(Debug)]
pub struct TurnContext {
    /// 活跃 turn 的稳定标识符。
    pub(crate) sub_id: String,
    /// 会话内从 1 开始的用户轮次序号。
    pub(crate) turn: usize,
    /// turn 开始时准入的交互模式。
    pub(crate) mode: types::InteractionMode,
    /// turn 开始时准入的工具暴露模式。
    pub(crate) requested_tool_mode: types::ToolMode,
    /// turn 开始时准入的权限配置。
    pub(crate) permission_profile: Option<String>,
    /// turn 开始时准入的项目根路径。
    pub(crate) project_root: Option<PathBuf>,
    /// turn 开始时准入的所有可写项目根路径。
    pub(crate) workspace_roots: Vec<PathBuf>,
    /// 注入活跃任务的用户输入，在下一次采样请求前被消费。
    input_state: Mutex<TurnInputState>,
    input_notify: Notify,
    child_tracker: Arc<ChildTracker>,
    #[cfg(test)]
    preparing_reservation_notify: Notify,
}

/// 终态采样边界的输入决策：已排队、邮箱待处理、或关闭。
pub(crate) enum TerminalInputDecision {
    Queued(Vec<QueuedTurnInput>),
    MailboxPending,
    Closed,
}

impl TurnContext {
    #[cfg(test)]
    pub(crate) fn new(
        sub_id: String,
        turn: usize,
        mode: types::InteractionMode,
        permission_profile: Option<String>,
        project_root: Option<PathBuf>,
    ) -> Self {
        let workspace_roots = project_root.iter().cloned().collect();
        Self::new_with_roots(
            sub_id,
            turn,
            mode,
            permission_profile,
            project_root,
            workspace_roots,
        )
    }

    pub(crate) fn new_with_roots(
        sub_id: String,
        turn: usize,
        mode: types::InteractionMode,
        permission_profile: Option<String>,
        project_root: Option<PathBuf>,
        workspace_roots: Vec<PathBuf>,
    ) -> Self {
        Self {
            sub_id,
            turn,
            mode,
            requested_tool_mode: types::ToolMode::Direct,
            permission_profile,
            project_root,
            workspace_roots,
            input_state: Mutex::new(TurnInputState {
                pending: Vec::new(),
                mailbox_pending: Vec::new(),
                readiness: TurnInputReadiness::Preparing,
                in_flight_admissions: 0,
            }),
            input_notify: Notify::new(),
            child_tracker: Arc::new(ChildTracker::default()),
            #[cfg(test)]
            preparing_reservation_notify: Notify::new(),
        }
    }

    pub fn sub_id(&self) -> &str {
        &self.sub_id
    }

    pub fn turn(&self) -> usize {
        self.turn
    }

    pub fn mode(&self) -> types::InteractionMode {
        self.mode
    }

    pub(crate) fn with_requested_tool_mode(mut self, mode: types::ToolMode) -> Self {
        self.requested_tool_mode = mode;
        self
    }

    pub fn requested_tool_mode(&self) -> types::ToolMode {
        self.requested_tool_mode
    }

    pub fn permission_profile(&self) -> Option<&str> {
        self.permission_profile.as_deref()
    }

    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    pub fn workspace_roots(&self) -> &[PathBuf] {
        &self.workspace_roots
    }

    /// 注册一个子任务并返回存活凭证。
    pub(crate) fn track_child(&self) -> ChildPermit {
        self.child_tracker.active.fetch_add(1, Ordering::AcqRel);
        ChildPermit {
            tracker: Arc::clone(&self.child_tracker),
        }
    }

    /// 等待所有子任务完成（活跃计数归零）。
    pub(crate) async fn wait_for_children(&self) {
        loop {
            let changed = self.child_tracker.changed.notified();
            if self.child_tracker.active.load(Ordering::Acquire) == 0 {
                return;
            }
            changed.await;
        }
    }

    pub(crate) fn has_live_children(&self) -> bool {
        self.child_tracker.active.load(Ordering::Acquire) != 0
    }

    /// 等待 turn 准备完成后获取输入预留，admission 关闭则返回 None。
    pub(crate) async fn reserve_input(self: &Arc<Self>) -> Option<TurnInputReservation> {
        loop {
            let notified = self.input_notify.notified();
            {
                let mut state = self
                    .input_state
                    .lock()
                    .expect("turn input state mutex poisoned");
                match state.readiness {
                    TurnInputReadiness::Preparing => {
                        #[cfg(test)]
                        self.preparing_reservation_notify.notify_one();
                    }
                    TurnInputReadiness::Accepting => {
                        state.in_flight_admissions += 1;
                        return Some(TurnInputReservation {
                            turn_context: Arc::clone(self),
                            finished: false,
                        });
                    }
                    TurnInputReadiness::Closed => return None,
                }
            }
            notified.await;
        }
    }

    /// 将输入准入状态从 Preparing 切换到 Accepting。
    pub(crate) fn open_input_admission(&self) {
        {
            let mut state = self
                .input_state
                .lock()
                .expect("turn input state mutex poisoned");
            if state.readiness == TurnInputReadiness::Preparing {
                state.readiness = TurnInputReadiness::Accepting;
            }
        }
        self.input_notify.notify_waiters();
    }

    /// 关闭输入准入并清空待处理队列。
    pub(crate) fn close_input_admission(&self) {
        {
            let mut state = self
                .input_state
                .lock()
                .expect("turn input state mutex poisoned");
            state.readiness = TurnInputReadiness::Closed;
            state.pending.clear();
            state.mailbox_pending.clear();
        }
        self.input_notify.notify_waiters();
    }

    /// 取出并清空当前排队的所有待处理输入。
    pub(crate) fn take_pending_input(&self) -> Vec<QueuedTurnInput> {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        std::mem::take(&mut state.pending)
    }

    /// 预留邮箱消息 ID，确保持久写入与确认使用相同标识。
    pub(crate) fn reserve_mailbox_input(&self) -> Option<String> {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        if state.readiness == TurnInputReadiness::Closed {
            return None;
        }
        let mailbox_message_id = uuid::Uuid::new_v4().to_string();
        state.mailbox_pending.push(PendingInputSignal {
            mailbox_message_id: mailbox_message_id.clone(),
        });
        Some(mailbox_message_id)
    }

    /// 确认已投递的邮箱消息，仅移除匹配的 pending 信号。
    pub(crate) fn acknowledge_mailbox_inputs(&self, delivered_message_ids: &[String]) -> usize {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        let before = state.mailbox_pending.len();
        state.mailbox_pending.retain(|pending| {
            !delivered_message_ids
                .iter()
                .any(|delivered| delivered == &pending.mailbox_message_id)
        });
        let acknowledged = before - state.mailbox_pending.len();
        drop(state);
        if acknowledged > 0 {
            self.input_notify.notify_waiters();
        }
        acknowledged
    }

    /// 撤回指定邮箱消息的 pending 信号。
    pub(crate) fn retract_input(&self, mailbox_message_id: &str) {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        state
            .mailbox_pending
            .retain(|pending| pending.mailbox_message_id != mailbox_message_id);
        drop(state);
        self.input_notify.notify_waiters();
    }

    /// 仅当队列和准入均无待处理时，原子地关闭输入引导。
    #[cfg(test)]
    pub(crate) fn close_if_no_pending_input(&self) -> bool {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        if state.pending.is_empty()
            && state.mailbox_pending.is_empty()
            && state.in_flight_admissions == 0
        {
            state.readiness = TurnInputReadiness::Closed;
            true
        } else {
            false
        }
    }

    /// 等待 in-flight admission 完成，原子选择排队输入、邮箱投递或关闭。
    pub(crate) async fn wait_for_terminal_input(&self) -> TerminalInputDecision {
        loop {
            let notified = self.input_notify.notified();
            {
                let mut state = self
                    .input_state
                    .lock()
                    .expect("turn input state mutex poisoned");
                if !state.pending.is_empty() {
                    return TerminalInputDecision::Queued(std::mem::take(&mut state.pending));
                }
                if !state.mailbox_pending.is_empty() {
                    return TerminalInputDecision::MailboxPending;
                }
                match state.readiness {
                    TurnInputReadiness::Preparing => {}
                    TurnInputReadiness::Accepting if state.in_flight_admissions == 0 => {
                        state.readiness = TurnInputReadiness::Closed;
                        return TerminalInputDecision::Closed;
                    }
                    TurnInputReadiness::Closed => return TerminalInputDecision::Closed,
                    TurnInputReadiness::Accepting => {}
                }
            }
            notified.await;
        }
    }

    /// 测试辅助方法，保留原始的仅队列断言接口。
    #[cfg(test)]
    pub(crate) async fn take_pending_input_or_close(&self) -> Vec<QueuedTurnInput> {
        match self.wait_for_terminal_input().await {
            TerminalInputDecision::Queued(inputs) => inputs,
            TerminalInputDecision::Closed => Vec::new(),
            TerminalInputDecision::MailboxPending => {
                panic!("mailbox input is pending at a queue-only terminal boundary")
            }
        }
    }
}

impl TurnInputReservation {
    #[cfg(test)]
    pub(crate) fn commit(mut self, input: TurnInput, inject_context: Option<String>) {
        self.finish(Some(QueuedTurnInput {
            input,
            inject_context,
        }));
    }

    fn finish(&mut self, input: Option<QueuedTurnInput>) {
        if self.finished {
            return;
        }
        {
            let mut state = self
                .turn_context
                .input_state
                .lock()
                .expect("turn input state mutex poisoned");
            debug_assert!(state.in_flight_admissions > 0);
            if let Some(input) = input {
                state.pending.push(input);
            }
            state.in_flight_admissions -= 1;
            self.finished = true;
        }
        self.turn_context.input_notify.notify_one();
    }
}

impl Drop for TurnInputReservation {
    fn drop(&mut self) {
        self.finish(None);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn input(text: &str) -> TurnInput {
        TurnInput {
            content: text.to_string(),
            image_data_urls: Vec::new(),
            client_message_id: None,
        }
    }

    #[tokio::test]
    async fn closing_an_empty_input_queue_rejects_late_steer() {
        let turn_context = Arc::new(TurnContext::new(
            "turn-1".into(),
            1,
            types::InteractionMode::Agent,
            None,
            None,
        ));
        turn_context.open_input_admission();
        turn_context
            .reserve_input()
            .await
            .expect("initial reservation")
            .commit(input("first"), None);
        assert_eq!(
            turn_context
                .take_pending_input()
                .into_iter()
                .map(|queued| queued.input)
                .collect::<Vec<_>>(),
            vec![input("first")]
        );
        assert!(turn_context.take_pending_input_or_close().await.is_empty());
        assert!(turn_context.reserve_input().await.is_none());
    }

    #[tokio::test]
    async fn reservation_blocks_terminal_close_until_commit() {
        let turn_context = Arc::new(TurnContext::new(
            "turn-1".into(),
            1,
            types::InteractionMode::Agent,
            None,
            None,
        ));
        turn_context.open_input_admission();
        let reservation = turn_context.reserve_input().await.expect("reservation");
        let mut close = Box::pin(turn_context.take_pending_input_or_close());

        tokio::select! {
            biased;
            value = &mut close => panic!("terminal close completed early: {value:?}"),
            _ = tokio::task::yield_now() => {}
        }
        reservation.commit(input("follow up"), None);

        assert_eq!(
            close
                .await
                .into_iter()
                .map(|queued| queued.input)
                .collect::<Vec<_>>(),
            vec![input("follow up")]
        );
    }

    #[tokio::test]
    async fn dropping_reservation_unblocks_terminal_close_and_closes_queue() {
        let turn_context = Arc::new(TurnContext::new(
            "turn-1".into(),
            1,
            types::InteractionMode::Agent,
            None,
            None,
        ));
        turn_context.open_input_admission();
        let reservation = turn_context.reserve_input().await.expect("reservation");
        let mut close = Box::pin(turn_context.take_pending_input_or_close());

        tokio::select! {
            biased;
            value = &mut close => panic!("terminal close completed early: {value:?}"),
            _ = tokio::task::yield_now() => {}
        }
        drop(reservation);

        assert!(close.await.is_empty());
        assert!(turn_context.reserve_input().await.is_none());
    }

    #[tokio::test]
    async fn reservation_waits_for_preparation_and_observes_close() {
        let turn_context = Arc::new(TurnContext::new(
            "turn-1".into(),
            1,
            types::InteractionMode::Agent,
            None,
            None,
        ));
        let reservation = tokio::spawn({
            let turn_context = Arc::clone(&turn_context);
            async move { turn_context.reserve_input().await }
        });

        tokio::task::yield_now().await;
        assert!(!reservation.is_finished());
        turn_context.close_input_admission();

        assert!(reservation.await.unwrap().is_none());
    }

    #[test]
    fn acknowledgement_removes_only_matching_mailbox_identities() {
        let turn_context = TurnContext::new(
            "turn-1".into(),
            1,
            types::InteractionMode::Agent,
            None,
            None,
        );
        let first = turn_context.reserve_mailbox_input().unwrap();
        let second = turn_context.reserve_mailbox_input().unwrap();
        let unrelated = turn_context.reserve_mailbox_input().unwrap();

        assert_eq!(
            turn_context.acknowledge_mailbox_inputs(&["old-generation-message".into()]),
            0
        );
        assert_eq!(
            turn_context.acknowledge_mailbox_inputs(std::slice::from_ref(&first)),
            1
        );
        assert_eq!(turn_context.acknowledge_mailbox_inputs(&[first]), 0);
        assert!(!turn_context.close_if_no_pending_input());
        assert_eq!(turn_context.acknowledge_mailbox_inputs(&[second]), 1);
        assert!(!turn_context.close_if_no_pending_input());
        turn_context.retract_input(&unrelated);
        assert!(turn_context.close_if_no_pending_input());
    }
}
