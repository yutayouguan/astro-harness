//! Sampling-step immutable runtime state.
//!
//! A `StepContext` captures the exact conversation history and tool specs
//! advertised for one model request. Later phases will bind the same snapshot
//! to `ToolCallRuntime`; introducing the boundary first removes the old tuple
//! return from `prepare_llm_context` without changing provider behavior.

use std::sync::Arc;

use types::message::Message;

use super::{ToolRouter, TurnContext};

/// Request-scoped state captured immediately before one model sampling call.
#[derive(Debug)]
pub(crate) struct StepContext {
    pub(crate) turn: Arc<TurnContext>,
    pub(crate) history: Vec<Message>,
    pub(crate) tool_router: Arc<ToolRouter>,
}

impl StepContext {
    pub(crate) fn new(
        turn: Arc<TurnContext>,
        history: Vec<Message>,
        tool_router: Arc<ToolRouter>,
    ) -> Self {
        Self {
            turn,
            history,
            tool_router,
        }
    }

    /// Whether the model was offered this tool in the exact sampling request.
    pub(crate) fn advertises_tool(&self, name: &str) -> bool {
        self.tool_router.has_tool(name)
    }
}
