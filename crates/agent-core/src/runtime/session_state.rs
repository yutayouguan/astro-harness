//! Session-wide mutable runtime state.
//!
//! This boundary follows Codex's `SessionState`: mutable state that survives
//! across sampling steps lives together, while the active task registry remains
//! directly on [`super::Session`]. [`super::Session`] owns this container behind
//! a short-lived mutex so callers never expose references tied to a state guard.

use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use types::message::Message;

use super::{compression_state, model_ctx, turn_budget, StepContext, TurnContext};

/// Persistent mutable state previously stored directly on [`super::Session`].
pub(crate) struct SessionState {
    pub(crate) history: Vec<Message>,
    pub(crate) pending_session_start_source: Option<String>,
    pub(crate) model_ctx: model_ctx::ModelContext,
    pub(crate) compression: compression_state::CompressionState,
    pub(crate) turn: turn_budget::TurnState,
    pub(crate) pending_inject_context: Option<String>,
    pub(crate) pending_learning_nudge: Option<String>,
    /// Latest durable role-bearing context baseline used for WorldState diffing.
    pub(crate) prompt_context_snapshot: Option<Value>,
    /// Model-visible initial context plus source-level updates, kept outside chat storage.
    pub(crate) prompt_context_history: Vec<crate::prompt::context_state::PromptContextEvent>,
    pub(crate) interaction_mode: types::InteractionMode,
    pub(crate) current_turn_context: Option<Arc<TurnContext>>,
    pub(crate) current_step_context: Option<Arc<StepContext>>,
    pub(crate) mcp_config_override: Vec<mcp::McpServerConfig>,
    pub(crate) mcp_instructions: Vec<mcp::McpServerInstructions>,
    pub(crate) project_root: Option<PathBuf>,
    pub(crate) workspace_roots: Vec<PathBuf>,
    pub(crate) permission_profile: Option<String>,
    pub(crate) skill_config_overrides: Vec<(PathBuf, bool)>,
    pub(crate) temperature: f32,
    pub(crate) additional_params: Value,
}

impl SessionState {
    pub(crate) fn new(history: Vec<Message>, project_root: Option<PathBuf>) -> Self {
        let session_start_source = if history.is_empty() {
            "startup"
        } else {
            "resume"
        };
        Self {
            history,
            pending_session_start_source: Some(session_start_source.to_string()),
            model_ctx: model_ctx::ModelContext::default(),
            compression: compression_state::CompressionState::default(),
            turn: turn_budget::TurnState::default(),
            pending_inject_context: None,
            pending_learning_nudge: None,
            prompt_context_snapshot: None,
            prompt_context_history: Vec::new(),
            interaction_mode: types::InteractionMode::Agent,
            current_turn_context: None,
            current_step_context: None,
            mcp_config_override: Vec::new(),
            mcp_instructions: Vec::new(),
            workspace_roots: project_root.iter().cloned().collect(),
            project_root,
            permission_profile: None,
            skill_config_overrides: Vec::new(),
            temperature: 0.7,
            additional_params: Value::Null,
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

    pub(crate) fn tail_history(&self, n: usize) -> Vec<Message> {
        let len = self.history.len();
        if n >= len {
            self.history.clone()
        } else {
            self.history[len - n..].to_vec()
        }
    }

    pub(crate) fn replace_history(&mut self, history: Vec<Message>) {
        self.history = history;
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new(Vec::new(), None)
    }
}
