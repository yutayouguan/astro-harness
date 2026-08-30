//! Hook 事件的 canonical 名称。

use crate::event::HookEvent;

pub const PRE_TOOL_USE: &str = HookEvent::PreToolUse.as_str();
pub const PERMISSION_REQUEST: &str = HookEvent::PermissionRequest.as_str();
pub const POST_TOOL_USE: &str = HookEvent::PostToolUse.as_str();
pub const PRE_COMPACT: &str = HookEvent::PreCompact.as_str();
pub const POST_COMPACT: &str = HookEvent::PostCompact.as_str();
pub const SESSION_START: &str = HookEvent::SessionStart.as_str();
pub const SESSION_END: &str = HookEvent::SessionEnd.as_str();
pub const USER_PROMPT_SUBMIT: &str = HookEvent::UserPromptSubmit.as_str();
pub const SUBAGENT_START: &str = HookEvent::SubagentStart.as_str();
pub const SUBAGENT_STOP: &str = HookEvent::SubagentStop.as_str();
pub const STOP: &str = HookEvent::Stop.as_str();
pub const INTERRUPT: &str = HookEvent::Interrupt.as_str();

pub const PRE_LLM_CALL: &str = HookEvent::PreLlmCall.as_str();
pub const PRE_API_REQUEST: &str = HookEvent::PreApiRequest.as_str();
pub const POST_API_REQUEST: &str = HookEvent::PostApiRequest.as_str();
pub const TRANSFORM_TERMINAL_OUTPUT: &str = HookEvent::TransformTerminalOutput.as_str();
pub const TRANSFORM_TOOL_RESULT: &str = HookEvent::TransformToolResult.as_str();
pub const TRANSFORM_FINAL_LLM_OUTPUT: &str = HookEvent::TransformFinalLlmOutput.as_str();
pub const POST_LLM_CALL: &str = HookEvent::PostLlmCall.as_str();
pub const POST_APPROVAL_RESPONSE: &str = HookEvent::PostApprovalResponse.as_str();
pub const PRE_GATEWAY_DISPATCH: &str = HookEvent::PreGatewayDispatch.as_str();
pub const SESSION_RESET: &str = HookEvent::SessionReset.as_str();
pub const GATEWAY_STARTUP: &str = HookEvent::GatewayStartup.as_str();
pub const AGENT_END: &str = HookEvent::AgentEnd.as_str();
pub const COMMAND_NEW_CHAT: &str = HookEvent::CommandNewChat.as_str();

/// 可影响流程的钩子（其余为观察型）。
pub fn is_mutating_hook(name: &str) -> bool {
    matches!(
        name,
        PRE_LLM_CALL
            | PRE_TOOL_USE
            | PERMISSION_REQUEST
            | POST_TOOL_USE
            | PRE_COMPACT
            | POST_COMPACT
            | SESSION_START
            | USER_PROMPT_SUBMIT
            | SUBAGENT_START
            | SUBAGENT_STOP
            | PRE_GATEWAY_DISPATCH
            | STOP
            | TRANSFORM_TOOL_RESULT
            | TRANSFORM_TERMINAL_OUTPUT
            | TRANSFORM_FINAL_LLM_OUTPUT
    )
}
