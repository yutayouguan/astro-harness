//! 钩子名常量（与生命周期文档对齐）。

pub const ON_SESSION_START: &str = "on_session_start";
pub const PRE_LLM_CALL: &str = "pre_llm_call";
pub const PRE_API_REQUEST: &str = "pre_api_request";
pub const POST_API_REQUEST: &str = "post_api_request";
pub const PRE_TOOL_CALL: &str = "pre_tool_call";
pub const POST_TOOL_CALL: &str = "post_tool_call";
pub const POST_LLM_CALL: &str = "post_llm_call";
pub const ON_SESSION_END: &str = "on_session_end";
pub const ON_SESSION_FINALIZE: &str = "on_session_finalize";
pub const ON_SESSION_RESET: &str = "on_session_reset";
pub const SUBAGENT_STOP: &str = "subagent_stop";
pub const PRE_GATEWAY_DISPATCH: &str = "pre_gateway_dispatch";
pub const PRE_VERIFY: &str = "pre_verify";
pub const SUBAGENT_START: &str = "subagent_start";
pub const PRE_APPROVAL_REQUEST: &str = "pre_approval_request";
pub const POST_APPROVAL_RESPONSE: &str = "post_approval_response";
pub const TRANSFORM_TOOL_RESULT: &str = "transform_tool_result";
pub const TRANSFORM_TERMINAL_OUTPUT: &str = "transform_terminal_output";
pub const TRANSFORM_LLM_OUTPUT: &str = "transform_llm_output";

/// Gateway 外壳事件。
pub const GATEWAY_STARTUP: &str = "gateway:startup";
pub const SESSION_START: &str = "session:start";
pub const AGENT_END: &str = "agent:end";
pub const COMMAND_NEW_CHAT: &str = "command:new_chat";

/// 可影响流程的钩子（其余为观察型）。
pub fn is_mutating_hook(name: &str) -> bool {
    matches!(
        name,
        PRE_LLM_CALL
            | PRE_TOOL_CALL
            | PRE_GATEWAY_DISPATCH
            | PRE_VERIFY
            | TRANSFORM_TOOL_RESULT
            | TRANSFORM_TERMINAL_OUTPUT
            | TRANSFORM_LLM_OUTPUT
    )
}
