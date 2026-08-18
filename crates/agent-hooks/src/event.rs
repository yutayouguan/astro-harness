//! Hook 事件契约与历史名称归一化。

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

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
    PreLlmCall,
    PreApiRequest,
    PostApiRequest,
    TransformTerminalOutput,
    TransformToolResult,
    TransformLlmOutput,
    PostLlmCall,
    PostApprovalResponse,
    PreGatewayDispatch,
    SessionReset,
    SessionFinalize,
    GatewayStartup,
    AgentEnd,
    CommandNewChat,
}

impl HookEvent {
    /// Codex canonical 事件，顺序是对外契约的一部分。
    pub const CODEX: [Self; 11] = [
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
            Self::PreLlmCall => "PreLlmCall",
            Self::PreApiRequest => "PreApiRequest",
            Self::PostApiRequest => "PostApiRequest",
            Self::TransformTerminalOutput => "TransformTerminalOutput",
            Self::TransformToolResult => "TransformToolResult",
            Self::TransformLlmOutput => "TransformLlmOutput",
            Self::PostLlmCall => "PostLlmCall",
            Self::PostApprovalResponse => "PostApprovalResponse",
            Self::PreGatewayDispatch => "PreGatewayDispatch",
            Self::SessionReset => "SessionReset",
            Self::SessionFinalize => "SessionFinalize",
            Self::GatewayStartup => "GatewayStartup",
            Self::AgentEnd => "AgentEnd",
            Self::CommandNewChat => "CommandNewChat",
        }
    }
}

/// 将历史 hook 事件名转为 canonical 名称，自定义名保持不变。
pub fn canonical_hook_event_name(name: &str) -> Cow<'_, str> {
    let canonical = match name {
        "pre_tool_call" => HookEvent::PreToolUse.as_str(),
        "post_tool_call" => HookEvent::PostToolUse.as_str(),
        "pre_approval_request" => HookEvent::PermissionRequest.as_str(),
        "pre_verify" => HookEvent::Stop.as_str(),
        "on_session_start" | "session:start" => HookEvent::SessionStart.as_str(),
        "on_session_end" | "agent:end" => HookEvent::AgentEnd.as_str(),
        "subagent_start" => HookEvent::SubagentStart.as_str(),
        "subagent_stop" => HookEvent::SubagentStop.as_str(),
        "pre_llm_call" => HookEvent::PreLlmCall.as_str(),
        "pre_api_request" => HookEvent::PreApiRequest.as_str(),
        "post_api_request" => HookEvent::PostApiRequest.as_str(),
        "transform_terminal_output" => HookEvent::TransformTerminalOutput.as_str(),
        "transform_tool_result" => HookEvent::TransformToolResult.as_str(),
        "transform_llm_output" => HookEvent::TransformLlmOutput.as_str(),
        "post_llm_call" => HookEvent::PostLlmCall.as_str(),
        "post_approval_response" => HookEvent::PostApprovalResponse.as_str(),
        "pre_gateway_dispatch" => HookEvent::PreGatewayDispatch.as_str(),
        "on_session_reset" => HookEvent::SessionReset.as_str(),
        "on_session_finalize" => HookEvent::SessionFinalize.as_str(),
        "gateway:startup" => HookEvent::GatewayStartup.as_str(),
        "command:new_chat" => HookEvent::CommandNewChat.as_str(),
        _ => return Cow::Borrowed(name),
    };
    Cow::Borrowed(canonical)
}

/// 归一化 hook 事件名，并对每个已使用的历史名称最多警告一次。
pub fn normalize_hook_event_name(name: &str) -> Cow<'_, str> {
    static WARNED_LEGACY_NAMES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

    let canonical = canonical_hook_event_name(name);
    if canonical.as_ref() != name {
        let warned_names = WARNED_LEGACY_NAMES.get_or_init(|| Mutex::new(HashSet::new()));
        let should_warn = warned_names
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(name.to_owned());
        if should_warn {
            tracing::warn!(
                legacy_event = %name,
                canonical_event = %canonical.as_ref(),
                "legacy hook event name normalized"
            );
        }
    }
    canonical
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use crate::names::{
        is_mutating_hook, AGENT_END, COMMAND_NEW_CHAT, GATEWAY_STARTUP, ON_SESSION_END,
        ON_SESSION_FINALIZE, ON_SESSION_RESET, ON_SESSION_START, PERMISSION_REQUEST,
        POST_API_REQUEST, POST_APPROVAL_RESPONSE, POST_COMPACT, POST_LLM_CALL, POST_TOOL_CALL,
        POST_TOOL_USE, PRE_API_REQUEST, PRE_APPROVAL_REQUEST, PRE_COMPACT, PRE_GATEWAY_DISPATCH,
        PRE_LLM_CALL, PRE_TOOL_CALL, PRE_TOOL_USE, PRE_VERIFY, SESSION_END, SESSION_FINALIZE,
        SESSION_RESET, SESSION_START, STOP, SUBAGENT_START, SUBAGENT_STOP, TRANSFORM_LLM_OUTPUT,
        TRANSFORM_TERMINAL_OUTPUT, TRANSFORM_TOOL_RESULT, USER_PROMPT_SUBMIT,
    };
    #[test]
    fn codex_events_have_exact_names_and_order() {
        let codex: [HookEvent; 11] = HookEvent::CODEX;
        let as_str: fn(HookEvent) -> &'static str = HookEvent::as_str;
        let names: Vec<_> = codex.into_iter().map(as_str).collect();

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
            ]
        );
    }

    #[test]
    fn legacy_event_names_map_to_canonical_names() {
        let aliases = [
            ("pre_tool_call", PRE_TOOL_USE),
            ("post_tool_call", POST_TOOL_USE),
            ("pre_approval_request", PERMISSION_REQUEST),
            ("pre_verify", STOP),
            ("on_session_start", SESSION_START),
            ("session:start", SESSION_START),
            ("on_session_end", AGENT_END),
            ("agent:end", AGENT_END),
            ("subagent_start", SUBAGENT_START),
            ("subagent_stop", SUBAGENT_STOP),
            ("pre_llm_call", PRE_LLM_CALL),
            ("pre_api_request", PRE_API_REQUEST),
            ("post_api_request", POST_API_REQUEST),
            ("transform_terminal_output", TRANSFORM_TERMINAL_OUTPUT),
            ("transform_tool_result", TRANSFORM_TOOL_RESULT),
            ("transform_llm_output", TRANSFORM_LLM_OUTPUT),
            ("post_llm_call", POST_LLM_CALL),
            ("post_approval_response", POST_APPROVAL_RESPONSE),
            ("pre_gateway_dispatch", PRE_GATEWAY_DISPATCH),
            ("on_session_reset", SESSION_RESET),
            ("on_session_finalize", SESSION_FINALIZE),
            ("gateway:startup", GATEWAY_STARTUP),
            ("command:new_chat", COMMAND_NEW_CHAT),
        ];

        for (legacy, canonical) in aliases {
            assert_eq!(canonical_hook_event_name(legacy), canonical, "{legacy}");
        }
    }

    #[test]
    fn unknown_custom_event_name_is_unchanged() {
        for custom in [
            "acme:custom_event",
            "session_reset",
            "session_finalize",
            "gateway_startup",
            "agent_end",
            "command_new_chat",
        ] {
            let canonical = canonical_hook_event_name(custom);

            assert!(matches!(canonical, Cow::Borrowed(name) if name == custom));
        }
    }

    #[test]
    fn normalize_returns_canonical_name_for_aliases() {
        assert_eq!(normalize_hook_event_name("pre_tool_call"), PRE_TOOL_USE);
        assert_eq!(normalize_hook_event_name(PRE_TOOL_USE), PRE_TOOL_USE);
    }

    #[test]
    fn canonical_and_compatibility_constants_match_contract() {
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

        assert_eq!(PRE_LLM_CALL, "PreLlmCall");
        assert_eq!(PRE_API_REQUEST, "PreApiRequest");
        assert_eq!(POST_API_REQUEST, "PostApiRequest");
        assert_eq!(TRANSFORM_TERMINAL_OUTPUT, "TransformTerminalOutput");
        assert_eq!(TRANSFORM_TOOL_RESULT, "TransformToolResult");
        assert_eq!(TRANSFORM_LLM_OUTPUT, "TransformLlmOutput");
        assert_eq!(POST_LLM_CALL, "PostLlmCall");
        assert_eq!(POST_APPROVAL_RESPONSE, "PostApprovalResponse");
        assert_eq!(PRE_GATEWAY_DISPATCH, "PreGatewayDispatch");
        assert_eq!(SESSION_RESET, "SessionReset");
        assert_eq!(SESSION_FINALIZE, "SessionFinalize");
        assert_eq!(GATEWAY_STARTUP, "GatewayStartup");
        assert_eq!(AGENT_END, "AgentEnd");
        assert_eq!(COMMAND_NEW_CHAT, "CommandNewChat");

        assert_eq!(PRE_TOOL_CALL, PRE_TOOL_USE);
        assert_eq!(POST_TOOL_CALL, POST_TOOL_USE);
        assert_eq!(PRE_APPROVAL_REQUEST, PERMISSION_REQUEST);
        assert_eq!(PRE_VERIFY, STOP);
        assert_eq!(ON_SESSION_START, SESSION_START);
        assert_eq!(ON_SESSION_END, AGENT_END);
        assert_eq!(ON_SESSION_RESET, SESSION_RESET);
        assert_eq!(ON_SESSION_FINALIZE, SESSION_FINALIZE);
    }

    #[test]
    fn mutating_hook_detection_accepts_canonical_and_legacy_names() {
        for (canonical, legacy) in [
            (PRE_LLM_CALL, "pre_llm_call"),
            (PRE_TOOL_USE, "pre_tool_call"),
            (PRE_GATEWAY_DISPATCH, "pre_gateway_dispatch"),
            (STOP, "pre_verify"),
            (TRANSFORM_TOOL_RESULT, "transform_tool_result"),
            (TRANSFORM_TERMINAL_OUTPUT, "transform_terminal_output"),
            (TRANSFORM_LLM_OUTPUT, "transform_llm_output"),
        ] {
            assert!(is_mutating_hook(canonical), "{canonical}");
            assert!(is_mutating_hook(legacy), "{legacy}");
        }

        assert!(!is_mutating_hook(POST_TOOL_USE));
        assert!(!is_mutating_hook("post_tool_call"));
        assert!(!is_mutating_hook("acme:custom_event"));
    }
}
