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
    /// Provider backend identifier for LLM/API telemetry events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Sampling attempt ordinal within the current turn (1-based).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<usize>,
    /// Elapsed wall-clock time for the observed phase.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Stable telemetry status such as `started`, `succeeded`, or `failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
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
    #[serde(skip)]
    pub matcher_aliases: Vec<String>,
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
    pub fn for_event(&self, event_name: &str) -> Self {
        let mut input = self.clone();
        input.hook_event_name = event_name.to_owned();
        input
    }
}

pub type HookPayload = HookInput;

/// `PermissionRequest` 聚合后的决定。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PermissionRequestDecision {
    #[default]
    Abstain,
    Allow,
    Deny(String),
}

/// `PostToolUse` 聚合后的模型可见控制结果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PostToolUseDecision {
    pub block_reason: Option<String>,
    pub additional_contexts: Vec<String>,
    pub feedback_messages: Vec<String>,
}

/// 钩子返回值；观察型应返回 [`Continue`](Self::Continue)。
#[derive(Debug, Clone, Default)]
pub enum HookOutcome {
    #[default]
    Continue,
    /// `PreToolUse`：阻断工具执行。
    Block(String),
    /// `PreToolUse`：替换参数。
    Modify(Value),
    /// `PreLlmCall`：注入本轮附加上下文。
    InjectContext(String),
    /// `PreGatewayDispatch`：放行。
    Allow,
    /// `PreGatewayDispatch`：跳过入队。
    Skip(String),
    /// `PreGatewayDispatch`：改写用户消息。
    Rewrite(String),
    /// transform 类钩子：替换文本。
    ReplaceText(String),
    /// `Stop`：继续本轮并注入提示。
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
    fn pre_tool_use_has_exact_wire_shape() {
        let input = HookInput {
            session_id: "session-1".into(),
            cwd: "/workspace".into(),
            hook_event_name: "PreToolUse".into(),
            model: "gpt-5.6-sol".into(),
            tool_name: Some("exec_command".into()),
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
                "tool_name": "exec_command",
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
    fn llm_telemetry_fields_are_serialized_when_present() {
        let input = HookInput {
            session_id: "session-1".into(),
            cwd: "/workspace".into(),
            hook_event_name: "PostLlmCall".into(),
            model: "gpt-5.6-sol".into(),
            provider: Some("openai".into()),
            attempt: Some(2),
            duration_ms: Some(1234),
            status: Some("succeeded".into()),
            ..Default::default()
        };

        assert_eq!(
            to_value(input).unwrap(),
            json!({
                "session_id": "session-1",
                "transcript_path": null,
                "cwd": "/workspace",
                "hook_event_name": "PostLlmCall",
                "model": "gpt-5.6-sol",
                "provider": "openai",
                "attempt": 2,
                "duration_ms": 1234,
                "status": "succeeded"
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
    fn for_event_sets_exact_name_only() {
        let input = HookInput::default();

        let event_input = input.for_event("PreToolUse");

        assert_eq!(event_input.hook_event_name, "PreToolUse");
        assert!(input.hook_event_name.is_empty());
        assert!(input.tool_input.is_none());
        assert!(input.tool_response.is_none());
        assert!(input.prompt.is_none());
    }

    #[test]
    fn for_event_preserves_explicit_values() {
        let input = HookInput {
            tool_input: Some(json!({"canonical": true})),
            tool_response: Some(json!({"status": "canonical"})),
            prompt: Some("canonical prompt".into()),
            ..Default::default()
        };

        let event_input = input.for_event("PostToolUse");

        assert_eq!(event_input.hook_event_name, "PostToolUse");
        assert_eq!(event_input.tool_input, input.tool_input);
        assert_eq!(event_input.tool_response, input.tool_response);
        assert_eq!(event_input.prompt, input.prompt);
    }
}
