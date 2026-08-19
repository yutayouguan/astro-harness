//! Session-wide mutable runtime state.
//!
//! This boundary follows Codex's `SessionState`: mutable state that survives
//! across sampling steps lives together, while the active task registry remains
//! directly on [`super::Session`]. [`super::Session`] owns this container behind
//! a Tokio mutex so callers never expose references tied to a state guard.

use std::sync::Arc;
use types::message::Message;

use super::{compression_state, turn_budget, StepContext, TurnContext};

/// Persistent mutable state previously stored directly on [`super::Session`].
pub(crate) struct SessionState {
    pub(crate) history: Vec<Message>,
    pub(crate) pending_session_start_source: Option<String>,
    pub(crate) compression: compression_state::CompressionState,
    pub(crate) turn: turn_budget::TurnState,
    pub(crate) pending_inject_context: Option<String>,
    pub(crate) pending_learning_nudge: Option<String>,
    pub(crate) interaction_mode: types::InteractionMode,
    pub(crate) current_turn_context: Option<Arc<TurnContext>>,
    pub(crate) current_step_context: Option<Arc<StepContext>>,
}

impl SessionState {
    pub(crate) fn new(history: Vec<Message>) -> Self {
        let session_start_source = if history.is_empty() {
            "startup"
        } else {
            "resume"
        };
        Self {
            history,
            pending_session_start_source: Some(session_start_source.to_string()),
            compression: compression_state::CompressionState::default(),
            turn: turn_budget::TurnState::default(),
            pending_inject_context: None,
            pending_learning_nudge: None,
            interaction_mode: types::InteractionMode::Agent,
            current_turn_context: None,
            current_step_context: None,
        }
    }

    pub(crate) fn record_items<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = Message>,
    {
        self.history.extend(items);
    }

    pub(crate) fn clone_history(&self) -> Vec<Message> {
        self.history.clone()
    }

    #[cfg(test)]
    pub(crate) fn replace_history(&mut self, history: Vec<Message>) {
        self.history = history;
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}
