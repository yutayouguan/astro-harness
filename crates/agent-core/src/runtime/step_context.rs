//! Sampling-step immutable runtime state.
//!
//! A `StepContext` captures the exact conversation history and tool specs
//! advertised for one model request. Later phases will bind the same snapshot
//! to `ToolCallRuntime`; introducing the boundary first removes the old tuple
//! return from `prepare_llm_context` without changing provider behavior.

use std::sync::Arc;

use serde_json::Value;
use types::message::Message;

use super::TurnContext;

/// Request-scoped state captured immediately before one model sampling call.
#[derive(Debug)]
pub(crate) struct StepContext {
    pub(crate) turn: Arc<TurnContext>,
    pub(crate) history: Vec<Message>,
    pub(crate) tool_specs: Vec<Value>,
}

impl StepContext {
    pub(crate) fn new(
        turn: Arc<TurnContext>,
        history: Vec<Message>,
        tool_specs: Vec<Value>,
    ) -> Self {
        Self {
            turn,
            history,
            tool_specs,
        }
    }
}
