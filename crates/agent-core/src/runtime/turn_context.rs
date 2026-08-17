//! Turn-scoped immutable runtime state.
//!
//! The naming and ownership boundary follow Codex's `TurnContext`: values in
//! this structure remain fixed for one user turn, while request-scoped values
//! belong to [`super::StepContext`].

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::tasks::TurnInput;

#[derive(Debug, Default)]
struct TurnInputState {
    pending: Vec<TurnInput>,
    accepting: bool,
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
            }),
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

    pub(crate) fn push_input(&self, input: TurnInput) -> bool {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        if !state.accepting {
            return false;
        }
        state.pending.push(input);
        true
    }

    pub(crate) fn take_pending_input(&self) -> Vec<TurnInput> {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        std::mem::take(&mut state.pending)
    }

    /// Atomically take queued input, or close steering if the queue is empty.
    pub(crate) fn take_pending_input_or_close(&self) -> Vec<TurnInput> {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        if state.pending.is_empty() {
            state.accepting = false;
            Vec::new()
        } else {
            std::mem::take(&mut state.pending)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(text: &str) -> TurnInput {
        TurnInput::UserInput {
            content: text.to_string(),
            image_data_urls: Vec::new(),
        }
    }

    #[test]
    fn closing_an_empty_input_queue_rejects_late_steer() {
        let turn_context = TurnContext::new(
            "turn-1".into(),
            1,
            types::InteractionMode::Agent,
            None,
            None,
        );
        assert!(turn_context.push_input(input("first")));
        assert_eq!(turn_context.take_pending_input(), vec![input("first")]);
        assert!(turn_context.take_pending_input_or_close().is_empty());
        assert!(!turn_context.push_input(input("late")));
    }
}
