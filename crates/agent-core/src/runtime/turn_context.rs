//! Turn-scoped immutable runtime state.
//!
//! The naming and ownership boundary follow Codex's `TurnContext`: values in
//! this structure remain fixed for one user turn, while request-scoped values
//! belong to [`super::StepContext`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::tasks::TurnInput;

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
    readiness: TurnInputReadiness,
    in_flight_admissions: usize,
}

pub(crate) struct TurnInputReservation {
    turn_context: Arc<TurnContext>,
    finished: bool,
}

/// Immutable state shared by every sampling step in one user turn.
#[derive(Debug)]
pub struct TurnContext {
    /// Stable identifier for the active turn.
    pub(crate) sub_id: String,
    /// One-based user-turn ordinal within the session.
    pub(crate) turn: usize,
    /// Interaction mode admitted when the turn started.
    pub(crate) mode: types::InteractionMode,
    /// Permission profile admitted when the turn started.
    pub(crate) permission_profile: Option<String>,
    /// Project root admitted when the turn started.
    pub(crate) project_root: Option<PathBuf>,
    /// User input steered into the active task, consumed before the next sampling request.
    input_state: Mutex<TurnInputState>,
    input_notify: Notify,
    #[cfg(test)]
    preparing_reservation_notify: Notify,
}

impl TurnContext {
    pub(crate) fn new(
        sub_id: String,
        turn: usize,
        mode: types::InteractionMode,
        permission_profile: Option<String>,
        project_root: Option<PathBuf>,
    ) -> Self {
        Self {
            sub_id,
            turn,
            mode,
            permission_profile,
            project_root,
            input_state: Mutex::new(TurnInputState {
                pending: Vec::new(),
                readiness: TurnInputReadiness::Preparing,
                in_flight_admissions: 0,
            }),
            input_notify: Notify::new(),
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

    pub fn permission_profile(&self) -> Option<&str> {
        self.permission_profile.as_deref()
    }

    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    /// Wait until initial turn preparation either succeeds or closes admission.
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

    #[cfg(test)]
    pub(crate) async fn wait_for_preparing_reservation(&self) {
        self.preparing_reservation_notify.notified().await;
    }

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

    pub(crate) fn close_input_admission(&self) {
        {
            let mut state = self
                .input_state
                .lock()
                .expect("turn input state mutex poisoned");
            state.readiness = TurnInputReadiness::Closed;
            state.pending.clear();
        }
        self.input_notify.notify_waiters();
    }

    pub(crate) fn take_pending_input(&self) -> Vec<QueuedTurnInput> {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        std::mem::take(&mut state.pending)
    }

    /// Atomically take queued input, or close steering if the queue is empty.
    pub(crate) async fn take_pending_input_or_close(&self) -> Vec<QueuedTurnInput> {
        loop {
            let notified = self.input_notify.notified();
            {
                let mut state = self
                    .input_state
                    .lock()
                    .expect("turn input state mutex poisoned");
                if !state.pending.is_empty() {
                    return std::mem::take(&mut state.pending);
                }
                match state.readiness {
                    TurnInputReadiness::Preparing => {}
                    TurnInputReadiness::Accepting if state.in_flight_admissions == 0 => {
                        state.readiness = TurnInputReadiness::Closed;
                        return Vec::new();
                    }
                    TurnInputReadiness::Closed => return Vec::new(),
                    TurnInputReadiness::Accepting => {}
                }
            }
            notified.await;
        }
    }
}

impl TurnInputReservation {
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
        TurnInput::UserInput {
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
}
