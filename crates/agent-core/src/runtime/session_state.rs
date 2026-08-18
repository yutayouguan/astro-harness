//! Session-wide mutable runtime state.
//!
//! This boundary follows Codex's `SessionState`: mutable state that survives
//! across sampling steps lives together, while the active task registry remains
//! directly on [`super::Session`]. The container is intentionally not locked
//! yet; adding synchronization is a separate lifecycle change.

use std::sync::Arc;

use super::{compression_state, turn_budget, StepContext, TurnContext};

/// Persistent mutable state previously stored directly on [`super::Session`].
pub(crate) struct SessionState {
    pub(crate) compression: compression_state::CompressionState,
    pub(crate) turn: turn_budget::TurnState,
    pub(crate) pending_inject_context: Option<String>,
    pub(crate) pending_learning_nudge: Option<String>,
    pub(crate) interaction_mode: types::InteractionMode,
    pub(crate) current_turn_context: Option<Arc<TurnContext>>,
    pub(crate) current_step_context: Option<Arc<StepContext>>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            compression: compression_state::CompressionState::default(),
            turn: turn_budget::TurnState::default(),
            pending_inject_context: None,
            pending_learning_nudge: None,
            interaction_mode: types::InteractionMode::Agent,
            current_turn_context: None,
            current_step_context: None,
        }
    }
}
