//! Tool invocation and per-step execution runtime.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::{Session, StepContext, ToolCallError};

/// One validated tool invocation bound to the sampling step that advertised it.
#[derive(Clone)]
pub(crate) struct ToolInvocation {
    pub(crate) session: Arc<Session>,
    pub(crate) step_context: Arc<StepContext>,
    pub(crate) cancellation_token: CancellationToken,
    pub(crate) call_id: String,
    pub(crate) tool_name: String,
    pub(crate) payload: serde_json::Value,
}

/// Executes tool calls against one immutable [`StepContext`].
#[derive(Clone)]
pub(crate) struct ToolCallRuntime {
    session: Arc<Session>,
    step_context: Arc<StepContext>,
}

impl ToolCallRuntime {
    pub(crate) fn new(session: Arc<Session>, step_context: Arc<StepContext>) -> Self {
        Self {
            session,
            step_context,
        }
    }

    pub(crate) fn handle_tool_call(
        &self,
        call: types::ParsedToolCall,
        cancellation_token: CancellationToken,
    ) -> Result<types::ToolOutput, ToolCallError> {
        if let Some(denial) = Self::hardline_denial(&call.name, &call.arguments) {
            return Ok(denial);
        }
        let invocation = ToolInvocation {
            session: Arc::clone(&self.session),
            step_context: Arc::clone(&self.step_context),
            cancellation_token,
            call_id: call.id,
            tool_name: call.name,
            payload: call.arguments,
        };
        let session = Arc::clone(&invocation.session);
        session.handle_tool_invocation(invocation)
    }

    /// Defense in depth for callers that accidentally bypass serial approval routing.
    pub(crate) fn hardline_denial(
        tool_name: &str,
        payload: &serde_json::Value,
    ) -> Option<types::ToolOutput> {
        if tool_name != "exec_command" {
            return None;
        }
        let command = payload.get("command").and_then(serde_json::Value::as_str)?;
        let description = tools::is_hardline_blocked(command)?;
        Some(
            format!(
                "Command denied by policy (dangerous: {description}). Do not retry without changing the command."
            )
            .into(),
        )
    }
}
