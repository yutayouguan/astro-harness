//! Codex-style typed contracts for session and turn lifecycle hooks.

use crate::{HookInput, HookOutcome, HookRuntime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStartSource {
    Startup,
    Resume,
    Clear,
    Compact,
}

impl SessionStartSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Resume => "resume",
            Self::Clear => "clear",
            Self::Compact => "compact",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartHookTarget {
    SessionStart {
        source: SessionStartSource,
    },
    SubagentStart {
        turn_id: String,
        agent_id: String,
        agent_type: String,
    },
}

#[derive(Debug, Clone)]
pub struct SessionStartRequest {
    pub session_id: String,
    pub cwd: String,
    pub transcript_path: Option<String>,
    pub model: String,
    pub permission_mode: String,
    pub target: StartHookTarget,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionStartOutcome {
    pub should_stop: bool,
    pub stop_reason: Option<String>,
    pub additional_contexts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionEndRequest {
    pub session_id: String,
    pub turn_id: String,
    pub cwd: String,
    pub transcript_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionEndOutcome;

#[derive(Debug, Clone)]
pub struct UserPromptSubmitRequest {
    pub session_id: String,
    pub turn_id: String,
    pub subagent: Option<crate::SubagentHookContext>,
    pub cwd: String,
    pub transcript_path: Option<String>,
    pub model: String,
    pub permission_mode: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserPromptSubmitOutcome {
    pub should_stop: bool,
    pub stop_reason: Option<String>,
    pub additional_contexts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PreCompactRequest {
    pub session_id: String,
    pub turn_id: String,
    pub subagent: Option<crate::SubagentHookContext>,
    pub cwd: String,
    pub transcript_path: Option<String>,
    pub model: String,
    pub trigger: String,
}

pub type PostCompactRequest = PreCompactRequest;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreCompactOutcome {
    pub should_stop: bool,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatelessHookOutcome {
    pub should_stop: bool,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopHookTarget {
    Stop,
    MemoryConsolidation,
    SubagentStop {
        agent_id: String,
        agent_type: String,
        agent_transcript_path: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct StopRequest {
    pub session_id: String,
    pub turn_id: String,
    pub cwd: String,
    pub transcript_path: Option<String>,
    pub model: String,
    pub permission_mode: String,
    pub stop_hook_active: bool,
    pub last_assistant_message: Option<String>,
    pub target: StopHookTarget,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StopOutcome {
    pub should_stop: bool,
    pub stop_reason: Option<String>,
    pub should_block: bool,
    pub block_reason: Option<String>,
    pub continuation_fragments: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct InterruptRequest {
    pub session_id: String,
    pub turn_id: String,
    pub cwd: String,
    pub transcript_path: Option<String>,
    pub model: String,
    pub permission_mode: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InterruptOutcome;

impl HookRuntime {
    pub fn run_session_start(&self, request: SessionStartRequest) -> SessionStartOutcome {
        let (event, payload) = match request.target.clone() {
            StartHookTarget::SessionStart { source } => {
                let mut payload = session_payload(&request);
                payload.source = Some(source.as_str().into());
                (crate::SESSION_START, payload)
            }
            StartHookTarget::SubagentStart {
                turn_id,
                agent_id,
                agent_type,
            } => {
                let mut payload = session_payload(&request);
                payload.turn_id = Some(turn_id);
                payload.agent_id = Some(agent_id);
                payload.agent_type = Some(agent_type);
                (crate::SUBAGENT_START, payload)
            }
        };
        let outcome = if event == crate::SUBAGENT_START {
            self.dispatch_subagent_start(&payload)
                .map(HookOutcome::InjectContext)
                .unwrap_or_default()
        } else {
            self.dispatch(event, &payload)
        };
        admission_outcome(outcome)
    }

    pub fn run_session_end(&self, request: SessionEndRequest) -> SessionEndOutcome {
        let _ = self.dispatch(
            crate::SESSION_END,
            &HookInput {
                session_id: request.session_id,
                turn_id: Some(request.turn_id),
                cwd: request.cwd,
                transcript_path: request.transcript_path,
                reason: Some("other".into()),
                ..Default::default()
            },
        );
        SessionEndOutcome
    }

    pub fn run_user_prompt_submit(
        &self,
        request: UserPromptSubmitRequest,
    ) -> UserPromptSubmitOutcome {
        let mut payload = turn_payload(
            request.session_id,
            request.turn_id,
            request.subagent,
            request.cwd,
            request.transcript_path,
            request.model,
            request.permission_mode,
        );
        payload.prompt = Some(request.prompt);
        let outcome = admission_outcome(self.dispatch(crate::USER_PROMPT_SUBMIT, &payload));
        UserPromptSubmitOutcome {
            should_stop: outcome.should_stop,
            stop_reason: outcome.stop_reason,
            additional_contexts: outcome.additional_contexts,
        }
    }

    pub fn run_pre_compact(&self, request: PreCompactRequest) -> PreCompactOutcome {
        let payload = compact_payload(request);
        let outcome = admission_outcome(self.dispatch(crate::PRE_COMPACT, &payload));
        PreCompactOutcome {
            should_stop: outcome.should_stop,
            stop_reason: outcome.stop_reason,
        }
    }

    pub fn run_post_compact(&self, request: PostCompactRequest) -> StatelessHookOutcome {
        let payload = compact_payload(request);
        let outcome = admission_outcome(self.dispatch(crate::POST_COMPACT, &payload));
        StatelessHookOutcome {
            should_stop: outcome.should_stop,
            stop_reason: outcome.stop_reason,
        }
    }

    pub fn run_stop(&self, request: StopRequest) -> StopOutcome {
        let mut payload = HookInput {
            session_id: request.session_id,
            turn_id: Some(request.turn_id),
            cwd: request.cwd,
            transcript_path: request.transcript_path,
            model: request.model,
            permission_mode: Some(request.permission_mode),
            stop_hook_active: Some(request.stop_hook_active),
            last_assistant_message: request.last_assistant_message,
            ..Default::default()
        };
        let outcome = match request.target {
            StopHookTarget::SubagentStop {
                agent_id,
                agent_type,
                agent_transcript_path,
            } => {
                payload.agent_id = Some(agent_id);
                payload.agent_type = Some(agent_type);
                payload.agent_transcript_path = agent_transcript_path;
                self.dispatch_subagent_stop(&payload)
            }
            StopHookTarget::Stop | StopHookTarget::MemoryConsolidation => {
                self.dispatch(crate::STOP, &payload)
            }
        };
        stop_outcome(outcome)
    }

    pub fn run_interrupt(&self, request: InterruptRequest) -> InterruptOutcome {
        let _ = self.dispatch(
            crate::INTERRUPT,
            &HookInput {
                session_id: request.session_id,
                turn_id: Some(request.turn_id),
                cwd: request.cwd,
                transcript_path: request.transcript_path,
                model: request.model,
                permission_mode: Some(request.permission_mode),
                ..Default::default()
            },
        );
        InterruptOutcome
    }
}

fn session_payload(request: &SessionStartRequest) -> HookInput {
    HookInput {
        session_id: request.session_id.clone(),
        cwd: request.cwd.clone(),
        transcript_path: request.transcript_path.clone(),
        model: request.model.clone(),
        permission_mode: Some(request.permission_mode.clone()),
        ..Default::default()
    }
}

fn turn_payload(
    session_id: String,
    turn_id: String,
    subagent: Option<crate::SubagentHookContext>,
    cwd: String,
    transcript_path: Option<String>,
    model: String,
    permission_mode: String,
) -> HookInput {
    let mut payload = HookInput {
        session_id,
        turn_id: Some(turn_id),
        cwd,
        transcript_path,
        model,
        permission_mode: Some(permission_mode),
        ..Default::default()
    };
    if let Some(subagent) = subagent {
        payload.agent_id = Some(subagent.agent_id);
        payload.agent_type = Some(subagent.agent_type);
        payload.agent_transcript_path = subagent.agent_transcript_path;
    }
    payload
}

fn compact_payload(request: PreCompactRequest) -> HookInput {
    let mut payload = turn_payload(
        request.session_id,
        request.turn_id,
        request.subagent,
        request.cwd,
        request.transcript_path,
        request.model,
        String::new(),
    );
    payload.trigger = Some(request.trigger);
    payload
}

fn admission_outcome(outcome: HookOutcome) -> SessionStartOutcome {
    match outcome {
        HookOutcome::Block(reason) => SessionStartOutcome {
            should_stop: true,
            stop_reason: Some(reason),
            additional_contexts: Vec::new(),
        },
        HookOutcome::InjectContext(context) => SessionStartOutcome {
            additional_contexts: vec![context],
            ..Default::default()
        },
        _ => SessionStartOutcome::default(),
    }
}

fn stop_outcome(outcome: HookOutcome) -> StopOutcome {
    match outcome {
        HookOutcome::Block(reason) => StopOutcome {
            should_block: true,
            block_reason: Some(reason),
            ..Default::default()
        },
        HookOutcome::KeepGoing(prompt) => StopOutcome {
            continuation_fragments: vec![prompt],
            ..Default::default()
        },
        _ => StopOutcome::default(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::PluginHookBus;

    fn runtime() -> HookRuntime {
        HookRuntime::with_plugin_bus(Arc::new(PluginHookBus::new()))
    }

    fn session_start_request() -> SessionStartRequest {
        SessionStartRequest {
            session_id: "session-1".into(),
            cwd: "/tmp".into(),
            transcript_path: None,
            model: "openai/gpt-5".into(),
            permission_mode: "workspace-write".into(),
            target: StartHookTarget::SessionStart {
                source: SessionStartSource::Resume,
            },
        }
    }

    #[test]
    fn session_start_preserves_source_and_context_outcome() {
        let runtime = runtime();
        runtime.plugin.register(crate::SESSION_START, |payload| {
            assert_eq!(payload.source.as_deref(), Some("resume"));
            HookOutcome::InjectContext("restored context".into())
        });

        let outcome = runtime.run_session_start(session_start_request());

        assert_eq!(outcome.additional_contexts, ["restored context"]);
        assert!(!outcome.should_stop);
    }

    #[test]
    fn post_compact_exposes_stop_decision() {
        let runtime = runtime();
        runtime
            .plugin
            .register(crate::POST_COMPACT, |_| HookOutcome::Block("stop".into()));
        let outcome = runtime.run_post_compact(PreCompactRequest {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            subagent: None,
            cwd: "/tmp".into(),
            transcript_path: None,
            model: "openai/gpt-5".into(),
            trigger: "auto".into(),
        });

        assert!(outcome.should_stop);
        assert_eq!(outcome.stop_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn subagent_stop_returns_continuation_fragment() {
        let runtime = runtime();
        runtime.plugin.register(crate::SUBAGENT_STOP, |_| {
            HookOutcome::KeepGoing("verify tests".into())
        });
        let outcome = runtime.run_stop(StopRequest {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            cwd: "/tmp".into(),
            transcript_path: None,
            model: "openai/gpt-5".into(),
            permission_mode: "workspace-write".into(),
            stop_hook_active: false,
            last_assistant_message: Some("done".into()),
            target: StopHookTarget::SubagentStop {
                agent_id: "child".into(),
                agent_type: "worker".into(),
                agent_transcript_path: None,
            },
        });

        assert_eq!(outcome.continuation_fragments, ["verify tests"]);
    }
}
