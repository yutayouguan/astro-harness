//! 钩子载荷与返回动作。

use serde::Serialize;
use serde_json::{Map, Value};

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

    pub(crate) fn command_input_for_event(&self, event: crate::HookEvent) -> Value {
        let mut input = Map::new();
        insert_string(&mut input, "session_id", &self.session_id);
        insert_nullable_string(
            &mut input,
            "transcript_path",
            self.transcript_path.as_deref(),
        );
        insert_string(&mut input, "cwd", &self.cwd);
        insert_string(&mut input, "hook_event_name", event.as_str());

        match event {
            crate::HookEvent::SessionStart => {
                insert_model_and_permission(&mut input, self);
                insert_string(
                    &mut input,
                    "source",
                    self.source.as_deref().unwrap_or_default(),
                );
            }
            crate::HookEvent::SessionEnd => {
                insert_string(
                    &mut input,
                    "reason",
                    self.reason.as_deref().unwrap_or_default(),
                );
            }
            crate::HookEvent::SubagentStart => {
                insert_turn(&mut input, self);
                insert_model_and_permission(&mut input, self);
                insert_string(
                    &mut input,
                    "agent_id",
                    self.agent_id.as_deref().unwrap_or_default(),
                );
                insert_string(
                    &mut input,
                    "agent_type",
                    self.agent_type.as_deref().unwrap_or_default(),
                );
            }
            crate::HookEvent::UserPromptSubmit => {
                insert_turn_and_optional_agent(&mut input, self);
                insert_model_and_permission(&mut input, self);
                insert_string(
                    &mut input,
                    "prompt",
                    self.prompt.as_deref().unwrap_or_default(),
                );
            }
            crate::HookEvent::PreCompact | crate::HookEvent::PostCompact => {
                insert_turn_and_optional_agent(&mut input, self);
                insert_string(&mut input, "model", &self.model);
                insert_string(
                    &mut input,
                    "trigger",
                    self.trigger.as_deref().unwrap_or_default(),
                );
            }
            crate::HookEvent::PreToolUse => {
                insert_tool_input(&mut input, self);
                insert_string(
                    &mut input,
                    "tool_use_id",
                    self.tool_use_id.as_deref().unwrap_or_default(),
                );
            }
            crate::HookEvent::PermissionRequest => insert_tool_input(&mut input, self),
            crate::HookEvent::PostToolUse => {
                insert_tool_input(&mut input, self);
                input.insert(
                    "tool_response".into(),
                    self.tool_response.clone().unwrap_or(Value::Null),
                );
                insert_string(
                    &mut input,
                    "tool_use_id",
                    self.tool_use_id.as_deref().unwrap_or_default(),
                );
            }
            crate::HookEvent::Stop => insert_stop_input(&mut input, self),
            crate::HookEvent::SubagentStop => {
                insert_stop_input(&mut input, self);
                insert_nullable_string(
                    &mut input,
                    "agent_transcript_path",
                    self.agent_transcript_path.as_deref(),
                );
                insert_string(
                    &mut input,
                    "agent_id",
                    self.agent_id.as_deref().unwrap_or_default(),
                );
                insert_string(
                    &mut input,
                    "agent_type",
                    self.agent_type.as_deref().unwrap_or_default(),
                );
            }
            crate::HookEvent::Interrupt => {
                insert_turn(&mut input, self);
                insert_model_and_permission(&mut input, self);
            }
            _ => {
                return serde_json::to_value(self.for_event(event.as_str())).unwrap_or(Value::Null)
            }
        }

        Value::Object(input)
    }
}

fn insert_string(input: &mut Map<String, Value>, key: &str, value: &str) {
    input.insert(key.into(), Value::String(value.to_string()));
}

fn insert_nullable_string(input: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    input.insert(
        key.into(),
        value.map_or(Value::Null, |value| Value::String(value.to_string())),
    );
}

fn insert_turn(input: &mut Map<String, Value>, hook: &HookInput) {
    insert_string(
        input,
        "turn_id",
        hook.turn_id.as_deref().unwrap_or_default(),
    );
}

fn insert_optional_agent(input: &mut Map<String, Value>, hook: &HookInput) {
    if let Some(agent_id) = hook.agent_id.as_deref() {
        insert_string(input, "agent_id", agent_id);
    }
    if let Some(agent_type) = hook.agent_type.as_deref() {
        insert_string(input, "agent_type", agent_type);
    }
}

fn insert_turn_and_optional_agent(input: &mut Map<String, Value>, hook: &HookInput) {
    insert_turn(input, hook);
    insert_optional_agent(input, hook);
}

fn insert_model_and_permission(input: &mut Map<String, Value>, hook: &HookInput) {
    insert_string(input, "model", &hook.model);
    insert_string(
        input,
        "permission_mode",
        hook.permission_mode.as_deref().unwrap_or_default(),
    );
}

fn insert_tool_input(input: &mut Map<String, Value>, hook: &HookInput) {
    insert_turn_and_optional_agent(input, hook);
    insert_model_and_permission(input, hook);
    insert_string(
        input,
        "tool_name",
        hook.tool_name.as_deref().unwrap_or_default(),
    );
    input.insert(
        "tool_input".into(),
        hook.tool_input.clone().unwrap_or(Value::Null),
    );
}

fn insert_stop_input(input: &mut Map<String, Value>, hook: &HookInput) {
    insert_turn(input, hook);
    insert_model_and_permission(input, hook);
    input.insert(
        "stop_hook_active".into(),
        Value::Bool(hook.stop_hook_active.unwrap_or_default()),
    );
    insert_nullable_string(
        input,
        "last_assistant_message",
        hook.last_assistant_message.as_deref(),
    );
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
    fn command_inputs_use_event_specific_codex_shapes() {
        let input = HookInput {
            session_id: "session-1".into(),
            transcript_path: Some("/tmp/rollout.jsonl".into()),
            cwd: "/workspace".into(),
            model: "gpt-5.6-sol".into(),
            turn_id: Some("turn-1".into()),
            permission_mode: Some("workspace-write".into()),
            source: Some("resume".into()),
            reason: Some("other".into()),
            prompt: Some("continue".into()),
            tool_name: Some("exec_command".into()),
            tool_use_id: Some("call-1".into()),
            tool_input: Some(json!({"cmd": "pwd"})),
            tool_response: Some(json!({"ok": true})),
            trigger: Some("auto".into()),
            agent_id: Some("child-1".into()),
            agent_type: Some("worker".into()),
            agent_transcript_path: Some("/tmp/child.jsonl".into()),
            stop_hook_active: Some(true),
            last_assistant_message: Some("done".into()),
            ..Default::default()
        };

        assert_eq!(
            input.command_input_for_event(crate::HookEvent::SessionEnd),
            json!({
                "session_id": "session-1",
                "transcript_path": "/tmp/rollout.jsonl",
                "cwd": "/workspace",
                "hook_event_name": "SessionEnd",
                "reason": "other"
            })
        );
        assert_eq!(
            input.command_input_for_event(crate::HookEvent::PreCompact),
            json!({
                "session_id": "session-1",
                "turn_id": "turn-1",
                "agent_id": "child-1",
                "agent_type": "worker",
                "transcript_path": "/tmp/rollout.jsonl",
                "cwd": "/workspace",
                "hook_event_name": "PreCompact",
                "model": "gpt-5.6-sol",
                "trigger": "auto"
            })
        );
        assert_eq!(
            input.command_input_for_event(crate::HookEvent::PermissionRequest),
            json!({
                "session_id": "session-1",
                "turn_id": "turn-1",
                "agent_id": "child-1",
                "agent_type": "worker",
                "transcript_path": "/tmp/rollout.jsonl",
                "cwd": "/workspace",
                "hook_event_name": "PermissionRequest",
                "model": "gpt-5.6-sol",
                "permission_mode": "workspace-write",
                "tool_name": "exec_command",
                "tool_input": {"cmd": "pwd"}
            })
        );
        assert_eq!(
            input.command_input_for_event(crate::HookEvent::SubagentStop),
            json!({
                "session_id": "session-1",
                "turn_id": "turn-1",
                "transcript_path": "/tmp/rollout.jsonl",
                "cwd": "/workspace",
                "hook_event_name": "SubagentStop",
                "model": "gpt-5.6-sol",
                "permission_mode": "workspace-write",
                "stop_hook_active": true,
                "last_assistant_message": "done",
                "agent_transcript_path": "/tmp/child.jsonl",
                "agent_id": "child-1",
                "agent_type": "worker"
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
