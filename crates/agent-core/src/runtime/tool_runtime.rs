//! 工具调用与单步执行运行时。

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::{Session, StepContext, ToolCallError};

/// 一次经过验证的工具调用，绑定到发布该工具的采样步骤。
#[derive(Clone)]
pub(crate) struct ToolInvocation {
    pub(crate) session: Arc<Session>,
    pub(crate) step_context: Arc<StepContext>,
    pub(crate) cancellation_token: CancellationToken,
    pub(crate) call_id: String,
    pub(crate) tool_name: String,
    pub(crate) tool_namespace: Option<String>,
    pub(crate) payload: serde_json::Value,
}

/// 针对一个不可变的 [`StepContext`] 执行工具调用。
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
        let registered_name = self
            .step_context
            .tool_router
            .registered_name(call.namespace.as_deref(), &call.name)
            .unwrap_or(&call.name);
        if let Some(denial) = Self::hardline_denial(registered_name, &call.arguments) {
            return Ok(denial);
        }
        let invocation = ToolInvocation {
            session: Arc::clone(&self.session),
            step_context: Arc::clone(&self.step_context),
            cancellation_token,
            call_id: call.id,
            tool_name: call.name,
            tool_namespace: call.namespace,
            payload: call.arguments,
        };
        let session = Arc::clone(&invocation.session);
        session.handle_tool_invocation(invocation)
    }

    /// 深度防御：防止调用方意外绕过串行审批路由。
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
