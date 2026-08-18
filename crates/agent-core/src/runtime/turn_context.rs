//! Turn-scoped immutable runtime state.
//!
//! The naming and ownership boundary follow Codex's `TurnContext`: values in
//! this structure remain fixed for one user turn, while request-scoped values
//! belong to [`super::StepContext`].

use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug)]
struct PendingInputSignal {
    mailbox_message_id: String,
}

#[derive(Debug, Default)]
struct TurnInputState {
    pending: Vec<PendingInputSignal>,
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

    /// Reserve the stable mailbox identity before its durable write. Using the
    /// same identity in both places makes delivery acknowledgement race-free.
    pub(crate) fn reserve_mailbox_input(&self) -> Option<String> {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        if !state.accepting {
            return None;
        }
        let mailbox_message_id = uuid::Uuid::new_v4().to_string();
        state.pending.push(PendingInputSignal {
            mailbox_message_id: mailbox_message_id.clone(),
        });
        Some(mailbox_message_id)
    }

    /// Remove only signals whose durable mailbox identities were delivered.
    /// Unrelated generations are intentionally kept.
    pub(crate) fn acknowledge_mailbox_inputs(&self, delivered_message_ids: &[String]) -> usize {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        let before = state.pending.len();
        state.pending.retain(|pending| {
            !delivered_message_ids
                .iter()
                .any(|delivered| delivered == &pending.mailbox_message_id)
        });
        before - state.pending.len()
    }

    pub(crate) fn retract_input(&self, mailbox_message_id: &str) {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        state
            .pending
            .retain(|pending| pending.mailbox_message_id != mailbox_message_id);
    }

    /// Atomically close steering only when no durable-delivery signal remains.
    pub(crate) fn close_if_no_pending_input(&self) -> bool {
        let mut state = self
            .input_state
            .lock()
            .expect("turn input state mutex poisoned");
        if state.pending.is_empty() {
            state.accepting = false;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_an_empty_input_queue_rejects_late_steer() {
        let turn_context = TurnContext::new(
            "turn-1".into(),
            1,
            types::InteractionMode::Agent,
            None,
            None,
        );
        let first = turn_context.reserve_mailbox_input().unwrap();
        assert!(!turn_context.close_if_no_pending_input());
        assert_eq!(turn_context.acknowledge_mailbox_inputs(&[first]), 1);
        assert!(turn_context.close_if_no_pending_input());
        assert!(turn_context.reserve_mailbox_input().is_none());
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
