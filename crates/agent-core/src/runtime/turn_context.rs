//! Turn-scoped immutable runtime state.
//!
//! The naming and ownership boundary follow Codex's `TurnContext`: values in
//! this structure remain fixed for one user turn, while request-scoped values
//! belong to [`super::StepContext`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use crate::tasks::TurnInput;

#[derive(Debug, Default)]
struct TurnInputState {
    pending: Vec<TurnInput>,
    accepting: bool,
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
                accepting: true,
                in_flight_admissions: 0,
            }),
            input_notify: Notify::new(),
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

    pub(crate) fn reserve_input(self: &Arc<Self>) -> Option<TurnInputReservation> {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        if !state.accepting {
            return None;
        }
        state.in_flight_admissions += 1;
        Some(TurnInputReservation {
            turn_context: Arc::clone(self),
            finished: false,
        })
    }

    pub(crate) fn take_pending_input(&self) -> Vec<TurnInput> {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        std::mem::take(&mut state.pending)
    }

    /// Atomically take queued input, or close steering if the queue is empty.
    pub(crate) async fn take_pending_input_or_close(&self) -> Vec<TurnInput> {
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
                if state.in_flight_admissions == 0 {
                    state.accepting = false;
                    return Vec::new();
                }
            }
            notified.await;
        }
    }
}

impl TurnInputReservation {
    pub(crate) fn commit(mut self, input: TurnInput) {
        self.finish(Some(input));
    }

    fn finish(&mut self, input: Option<TurnInput>) {
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
        turn_context
            .reserve_input()
            .expect("initial reservation")
            .commit(input("first"));
        assert_eq!(turn_context.take_pending_input(), vec![input("first")]);
        assert!(turn_context.take_pending_input_or_close().await.is_empty());
        assert!(turn_context.reserve_input().is_none());
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
        let reservation = turn_context.reserve_input().expect("reservation");
        let mut close = Box::pin(turn_context.take_pending_input_or_close());

        tokio::select! {
            biased;
            value = &mut close => panic!("terminal close completed early: {value:?}"),
            _ = tokio::task::yield_now() => {}
        }
        reservation.commit(input("follow up"));

        assert_eq!(close.await, vec![input("follow up")]);
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
        let reservation = turn_context.reserve_input().expect("reservation");
        let mut close = Box::pin(turn_context.take_pending_input_or_close());

        tokio::select! {
            biased;
            value = &mut close => panic!("terminal close completed early: {value:?}"),
            _ = tokio::task::yield_now() => {}
        }
        drop(reservation);

        assert!(close.await.is_empty());
        assert!(turn_context.reserve_input().is_none());
    }
}
