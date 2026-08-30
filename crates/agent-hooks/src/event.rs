//! Hook 事件的 canonical 契约。

/// Hook 事件的类型化名称。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    PreToolUse,
    PermissionRequest,
    PostToolUse,
    PreCompact,
    PostCompact,
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    SubagentStart,
    SubagentStop,
    Stop,
    Interrupt,
    PreLlmCall,
    PreApiRequest,
    PostApiRequest,
    TransformTerminalOutput,
    TransformToolResult,
    TransformFinalLlmOutput,
    PostLlmCall,
    PostApprovalResponse,
    PreGatewayDispatch,
    SessionReset,
    GatewayStartup,
    AgentEnd,
    CommandNewChat,
}

impl HookEvent {
    /// Command hook 支持的 canonical 事件，顺序是对外契约的一部分。
    pub const COMMAND_HOOK_EVENTS: [Self; 12] = [
        Self::PreToolUse,
        Self::PermissionRequest,
        Self::PostToolUse,
        Self::PreCompact,
        Self::PostCompact,
        Self::SessionStart,
        Self::SessionEnd,
        Self::UserPromptSubmit,
        Self::SubagentStart,
        Self::SubagentStop,
        Self::Stop,
        Self::Interrupt,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PermissionRequest => "PermissionRequest",
            Self::PostToolUse => "PostToolUse",
            Self::PreCompact => "PreCompact",
            Self::PostCompact => "PostCompact",
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::SubagentStart => "SubagentStart",
            Self::SubagentStop => "SubagentStop",
            Self::Stop => "Stop",
            Self::Interrupt => "Interrupt",
            Self::PreLlmCall => "PreLlmCall",
            Self::PreApiRequest => "PreApiRequest",
            Self::PostApiRequest => "PostApiRequest",
            Self::TransformTerminalOutput => "TransformTerminalOutput",
            Self::TransformToolResult => "TransformToolResult",
            Self::TransformFinalLlmOutput => "TransformFinalLlmOutput",
            Self::PostLlmCall => "PostLlmCall",
            Self::PostApprovalResponse => "PostApprovalResponse",
            Self::PreGatewayDispatch => "PreGatewayDispatch",
            Self::SessionReset => "SessionReset",
            Self::GatewayStartup => "GatewayStartup",
            Self::AgentEnd => "AgentEnd",
            Self::CommandNewChat => "CommandNewChat",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::{
        is_mutating_hook, AGENT_END, COMMAND_NEW_CHAT, GATEWAY_STARTUP, INTERRUPT,
        PERMISSION_REQUEST, POST_API_REQUEST, POST_APPROVAL_RESPONSE, POST_COMPACT, POST_LLM_CALL,
        POST_TOOL_USE, PRE_API_REQUEST, PRE_COMPACT, PRE_GATEWAY_DISPATCH, PRE_LLM_CALL,
        PRE_TOOL_USE, SESSION_END, SESSION_RESET, SESSION_START, STOP, SUBAGENT_START,
        SUBAGENT_STOP, TRANSFORM_FINAL_LLM_OUTPUT, TRANSFORM_TERMINAL_OUTPUT,
        TRANSFORM_TOOL_RESULT, USER_PROMPT_SUBMIT,
    };
    #[test]
    fn command_hook_events_have_exact_names_and_order() {
        let events: [HookEvent; 12] = HookEvent::COMMAND_HOOK_EVENTS;
        let as_str: fn(HookEvent) -> &'static str = HookEvent::as_str;
        let names: Vec<_> = events.into_iter().map(as_str).collect();

        assert_eq!(
            names,
            [
                "PreToolUse",
                "PermissionRequest",
                "PostToolUse",
                "PreCompact",
                "PostCompact",
                "SessionStart",
                "SessionEnd",
                "UserPromptSubmit",
                "SubagentStart",
                "SubagentStop",
                "Stop",
                "Interrupt",
            ]
        );
    }

    #[test]
    fn canonical_constants_match_contract() {
        assert_eq!(PRE_TOOL_USE, "PreToolUse");
        assert_eq!(PERMISSION_REQUEST, "PermissionRequest");
        assert_eq!(POST_TOOL_USE, "PostToolUse");
        assert_eq!(PRE_COMPACT, "PreCompact");
        assert_eq!(POST_COMPACT, "PostCompact");
        assert_eq!(SESSION_START, "SessionStart");
        assert_eq!(SESSION_END, "SessionEnd");
        assert_eq!(USER_PROMPT_SUBMIT, "UserPromptSubmit");
        assert_eq!(SUBAGENT_START, "SubagentStart");
        assert_eq!(SUBAGENT_STOP, "SubagentStop");
        assert_eq!(STOP, "Stop");
        assert_eq!(INTERRUPT, "Interrupt");

        assert_eq!(PRE_LLM_CALL, "PreLlmCall");
        assert_eq!(PRE_API_REQUEST, "PreApiRequest");
        assert_eq!(POST_API_REQUEST, "PostApiRequest");
        assert_eq!(TRANSFORM_TERMINAL_OUTPUT, "TransformTerminalOutput");
        assert_eq!(TRANSFORM_TOOL_RESULT, "TransformToolResult");
        assert_eq!(TRANSFORM_FINAL_LLM_OUTPUT, "TransformFinalLlmOutput");
        assert_eq!(POST_LLM_CALL, "PostLlmCall");
        assert_eq!(POST_APPROVAL_RESPONSE, "PostApprovalResponse");
        assert_eq!(PRE_GATEWAY_DISPATCH, "PreGatewayDispatch");
        assert_eq!(SESSION_RESET, "SessionReset");
        assert_eq!(GATEWAY_STARTUP, "GatewayStartup");
        assert_eq!(AGENT_END, "AgentEnd");
        assert_eq!(COMMAND_NEW_CHAT, "CommandNewChat");
    }

    #[test]
    fn mutating_hook_detection_requires_canonical_names() {
        for canonical in [
            PRE_LLM_CALL,
            PRE_TOOL_USE,
            PERMISSION_REQUEST,
            POST_TOOL_USE,
            PRE_COMPACT,
            POST_COMPACT,
            SESSION_START,
            USER_PROMPT_SUBMIT,
            SUBAGENT_START,
            SUBAGENT_STOP,
            PRE_GATEWAY_DISPATCH,
            STOP,
            TRANSFORM_TOOL_RESULT,
            TRANSFORM_TERMINAL_OUTPUT,
            TRANSFORM_FINAL_LLM_OUTPUT,
        ] {
            assert!(is_mutating_hook(canonical), "{canonical}");
        }

        assert!(!is_mutating_hook("post_tool_call"));
        assert!(!is_mutating_hook("pre_tool_call"));
        assert!(!is_mutating_hook("TransformLlmOutput"));
        assert!(!is_mutating_hook("acme:custom_event"));
    }
}
