//! 钩子载荷与返回动作。

use serde::Serialize;
use serde_json::Value;

/// 传入钩子回调的上下文快照。
#[derive(Debug, Clone, Default, Serialize)]
pub struct HookInput {
    pub session_id: String,
    pub transcript_path: Option<String>,
    pub cwd: String,
    pub hook_event_name: String,
    pub model: String,
    /// 当前流式回合 id（= agent `current_turn_id` / run_id）；与 `turn`（轮次计数）不同。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_transcript_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_hook_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<String>,

    #[serde(skip)]
    pub detail: String,
    #[serde(skip)]
    pub system_prompt_chars: Option<usize>,
    #[serde(skip)]
    pub assistant_chars: Option<usize>,
    #[serde(skip)]
    pub turn: Option<usize>,
    #[serde(skip)]
    pub error: Option<String>,
}

impl HookInput {
    pub fn normalized_for_event(&self, event_name: &str) -> Self {
        let mut normalized = self.clone();
        normalized.hook_event_name =
            crate::event::canonical_hook_event_name(event_name).into_owned();
        normalized
    }
}

pub type HookPayload = HookInput;

/// 钩子返回值；观察型应返回 [`Continue`](Self::Continue)。
#[derive(Debug, Clone, Default)]
pub enum HookOutcome {
    #[default]
    Continue,
    /// `pre_tool_call`：阻断工具执行。
    Block(String),
    /// `pre_tool_call`：替换参数。
    Modify(Value),
    /// `pre_llm_call`：注入本轮附加上下文。
    InjectContext(String),
    /// `pre_gateway_dispatch`：放行。
    Allow,
    /// `pre_gateway_dispatch`：跳过入队。
    Skip(String),
    /// `pre_gateway_dispatch`：改写用户消息。
    Rewrite(String),
    /// transform 类钩子：替换文本。
    ReplaceText(String),
    /// `pre_verify`：继续本轮并注入提示。
    KeepGoing(String),
}

impl HookOutcome {
    pub fn is_continue(&self) -> bool {
        matches!(self, Self::Continue | Self::Allow)
    }
}

#[cfg(test)]
mod wire_tests {
    use serde_json::{json, to_value};

    use super::{HookInput, HookPayload};

    #[test]
    fn pre_tool_use_has_exact_codex_wire_shape() {
        let input = HookInput {
            session_id: "session-1".into(),
            cwd: "/workspace".into(),
            hook_event_name: "PreToolUse".into(),
            model: "gpt-5.6-sol".into(),
            tool_name: Some("terminal".into()),
            tool_use_id: Some("call-1".into()),
            tool_input: Some(json!({"command": "pwd"})),
            ..Default::default()
        };

        assert_eq!(
            to_value(input).unwrap(),
            json!({
                "session_id": "session-1",
                "transcript_path": null,
                "cwd": "/workspace",
                "hook_event_name": "PreToolUse",
                "model": "gpt-5.6-sol",
                "tool_name": "terminal",
                "tool_use_id": "call-1",
                "tool_input": {"command": "pwd"}
            })
        );
    }

    #[test]
    fn astro_ui_fields_are_not_serialized() {
        let input = HookInput {
            detail: "visible only in Astro UI".into(),
            system_prompt_chars: Some(12),
            assistant_chars: Some(34),
            turn: Some(5),
            error: Some("failed".into()),
            ..Default::default()
        };

        assert_eq!(
            to_value(input).unwrap(),
            json!({
                "session_id": "",
                "transcript_path": null,
                "cwd": "",
                "hook_event_name": "",
                "model": ""
            })
        );
    }

    #[test]
    fn hook_payload_is_a_hook_input_alias() {
        let input = HookInput::default();
        let payload: HookPayload = input.clone();
        let exported_input: crate::HookInput = payload;
        let _: crate::HookPayload = exported_input;
    }

    #[test]
    fn normalized_for_event_only_canonicalizes_name() {
        let input = HookInput::default();

        let normalized = input.normalized_for_event("pre_tool_call");

        assert_eq!(normalized.hook_event_name, "PreToolUse");
        assert!(input.hook_event_name.is_empty());
        assert!(input.tool_input.is_none());
        assert!(input.tool_response.is_none());
        assert!(input.prompt.is_none());
    }

    #[test]
    fn normalized_for_event_preserves_explicit_canonical_values() {
        let input = HookInput {
            tool_input: Some(json!({"canonical": true})),
            tool_response: Some(json!({"status": "canonical"})),
            prompt: Some("canonical prompt".into()),
            ..Default::default()
        };

        let normalized = input.normalized_for_event("post_tool_call");

        assert_eq!(normalized.hook_event_name, "PostToolUse");
        assert_eq!(normalized.tool_input, input.tool_input);
        assert_eq!(normalized.tool_response, input.tool_response);
        assert_eq!(normalized.prompt, input.prompt);
    }
}
