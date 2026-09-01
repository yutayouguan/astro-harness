//! Codex-style typed contracts for tool lifecycle hooks.

use serde_json::Value;

use crate::{HookInput, HookOutcome, HookRuntime, PermissionRequestDecision};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentHookContext {
    pub agent_id: String,
    pub agent_type: String,
    pub agent_transcript_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PreToolUseRequest {
    pub session_id: String,
    pub turn_id: String,
    pub subagent: Option<SubagentHookContext>,
    pub cwd: String,
    pub transcript_path: Option<String>,
    pub model: String,
    pub permission_mode: String,
    pub tool_name: String,
    pub matcher_aliases: Vec<String>,
    pub tool_use_id: String,
    pub tool_input: Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreToolUseOutcome {
    pub should_block: bool,
    pub block_reason: Option<String>,
    pub additional_contexts: Vec<String>,
    pub updated_input: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct PermissionRequestRequest {
    pub session_id: String,
    pub turn_id: String,
    pub subagent: Option<SubagentHookContext>,
    pub cwd: String,
    pub transcript_path: Option<String>,
    pub model: String,
    pub permission_mode: String,
    pub tool_name: String,
    pub matcher_aliases: Vec<String>,
    pub run_id_suffix: String,
    pub tool_input: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionHookDecision {
    Allow,
    Deny { message: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionRequestOutcome {
    pub decision: Option<PermissionHookDecision>,
}

#[derive(Debug, Clone)]
pub struct PostToolUseRequest {
    pub session_id: String,
    pub turn_id: String,
    pub subagent: Option<SubagentHookContext>,
    pub cwd: String,
    pub transcript_path: Option<String>,
    pub model: String,
    pub permission_mode: String,
    pub tool_name: String,
    pub matcher_aliases: Vec<String>,
    pub tool_use_id: String,
    pub tool_input: Value,
    pub tool_response: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PostToolUseOutcome {
    pub should_block: bool,
    pub additional_contexts: Vec<String>,
    pub feedback_message: Option<String>,
}

impl HookRuntime {
    pub fn run_pre_tool_use(&self, request: PreToolUseRequest) -> PreToolUseOutcome {
        let outcome = self.dispatch(
            crate::PRE_TOOL_USE,
            &tool_payload(
                request.session_id,
                request.turn_id,
                request.subagent,
                request.cwd,
                request.transcript_path,
                request.model,
                request.permission_mode,
                request.tool_name,
                request.tool_use_id,
                request.tool_input,
                None,
                request.matcher_aliases,
            ),
        );
        match outcome {
            HookOutcome::Block(reason) => PreToolUseOutcome {
                should_block: true,
                block_reason: Some(reason),
                ..Default::default()
            },
            HookOutcome::Modify(updated_input) => PreToolUseOutcome {
                updated_input: Some(updated_input),
                ..Default::default()
            },
            HookOutcome::InjectContext(context) => PreToolUseOutcome {
                additional_contexts: vec![context],
                ..Default::default()
            },
            _ => PreToolUseOutcome::default(),
        }
    }

    pub fn run_permission_request(
        &self,
        request: PermissionRequestRequest,
    ) -> PermissionRequestOutcome {
        let decision = self.dispatch_permission_request(&tool_payload(
            request.session_id,
            request.turn_id,
            request.subagent,
            request.cwd,
            request.transcript_path,
            request.model,
            request.permission_mode,
            request.tool_name,
            request.run_id_suffix,
            request.tool_input,
            None,
            request.matcher_aliases,
        ));
        PermissionRequestOutcome {
            decision: match decision {
                PermissionRequestDecision::Abstain => None,
                PermissionRequestDecision::Allow => Some(PermissionHookDecision::Allow),
                PermissionRequestDecision::Deny(message) => {
                    Some(PermissionHookDecision::Deny { message })
                }
            },
        }
    }

    pub fn run_post_tool_use(&self, request: PostToolUseRequest) -> PostToolUseOutcome {
        let outcome = self.dispatch_post_tool_use(&tool_payload(
            request.session_id,
            request.turn_id,
            request.subagent,
            request.cwd,
            request.transcript_path,
            request.model,
            request.permission_mode,
            request.tool_name,
            request.tool_use_id,
            request.tool_input,
            Some(request.tool_response),
            request.matcher_aliases,
        ));
        let mut feedback_messages = outcome.feedback_messages;
        if let Some(reason) = outcome.block_reason.as_ref() {
            feedback_messages.insert(0, reason.clone());
        }
        PostToolUseOutcome {
            should_block: outcome.block_reason.is_some(),
            additional_contexts: outcome.additional_contexts,
            feedback_message: (!feedback_messages.is_empty())
                .then(|| feedback_messages.join("\n\n")),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn tool_payload(
    session_id: String,
    turn_id: String,
    subagent: Option<SubagentHookContext>,
    cwd: String,
    transcript_path: Option<String>,
    model: String,
    permission_mode: String,
    tool_name: String,
    tool_use_id: String,
    tool_input: Value,
    tool_response: Option<Value>,
    matcher_aliases: Vec<String>,
) -> HookInput {
    let subagent = subagent.unwrap_or_else(|| SubagentHookContext {
        agent_id: String::new(),
        agent_type: String::new(),
        agent_transcript_path: None,
    });
    HookInput {
        session_id,
        transcript_path,
        cwd,
        model,
        turn_id: Some(turn_id),
        permission_mode: Some(permission_mode),
        tool_name: Some(tool_name.clone()),
        matcher_aliases: matcher_aliases.clone(),
        tool_use_id: Some(tool_use_id),
        tool_input: Some(tool_input),
        tool_response,
        agent_id: (!subagent.agent_id.is_empty()).then_some(subagent.agent_id),
        agent_type: (!subagent.agent_type.is_empty()).then_some(subagent.agent_type),
        agent_transcript_path: subagent.agent_transcript_path,
        detail: if matcher_aliases.is_empty() {
            tool_name
        } else {
            format!("{tool_name} aliases={}", matcher_aliases.join(","))
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::PluginHookBus;

    fn runtime() -> HookRuntime {
        HookRuntime::with_plugin_bus(Arc::new(PluginHookBus::new()))
    }

    fn pre_request() -> PreToolUseRequest {
        PreToolUseRequest {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            subagent: None,
            cwd: "/tmp".into(),
            transcript_path: None,
            model: "openai/gpt-5".into(),
            permission_mode: "workspace-write".into(),
            tool_name: "terminal".into(),
            matcher_aliases: vec!["Bash".into()],
            tool_use_id: "call-1".into(),
            tool_input: json!({"command": "pwd"}),
        }
    }

    #[test]
    fn pre_tool_use_has_event_specific_output() {
        let runtime = runtime();
        runtime.plugin.register(crate::PRE_TOOL_USE, |payload| {
            assert_eq!(payload.tool_use_id.as_deref(), Some("call-1"));
            HookOutcome::Modify(json!({"command": "git status"}))
        });

        let outcome = runtime.run_pre_tool_use(pre_request());

        assert!(!outcome.should_block);
        assert_eq!(
            outcome.updated_input,
            Some(json!({"command": "git status"}))
        );
    }

    #[test]
    fn permission_abstain_is_not_an_implicit_allow() {
        let runtime = runtime();
        let pre = pre_request();
        let outcome = runtime.run_permission_request(PermissionRequestRequest {
            session_id: pre.session_id,
            turn_id: pre.turn_id,
            subagent: pre.subagent,
            cwd: pre.cwd,
            transcript_path: pre.transcript_path,
            model: pre.model,
            permission_mode: pre.permission_mode,
            tool_name: pre.tool_name,
            matcher_aliases: pre.matcher_aliases,
            run_id_suffix: pre.tool_use_id,
            tool_input: pre.tool_input,
        });

        assert_eq!(outcome.decision, None);
    }

    #[test]
    fn post_tool_use_keeps_block_feedback() {
        let runtime = runtime();
        runtime.plugin.register(crate::POST_TOOL_USE, |_| {
            HookOutcome::Block("redact".into())
        });
        let pre = pre_request();
        let outcome = runtime.run_post_tool_use(PostToolUseRequest {
            session_id: pre.session_id,
            turn_id: pre.turn_id,
            subagent: pre.subagent,
            cwd: pre.cwd,
            transcript_path: pre.transcript_path,
            model: pre.model,
            permission_mode: pre.permission_mode,
            tool_name: pre.tool_name,
            matcher_aliases: pre.matcher_aliases,
            tool_use_id: pre.tool_use_id,
            tool_input: pre.tool_input,
            tool_response: json!("secret"),
        });

        assert!(outcome.should_block);
        assert_eq!(outcome.feedback_message.as_deref(), Some("redact"));
    }
}
