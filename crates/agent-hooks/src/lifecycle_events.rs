//! Codex-style typed contracts for session and turn lifecycle hooks.

use std::sync::atomic::{AtomicU64, Ordering};

use agent_protocol::HookPromptFragment;

use crate::{CommandHookDecision, HookInput, HookOutcome, HookRuntime};

static NEXT_PLUGIN_HOOK_RUN_ID: AtomicU64 = AtomicU64::new(1);

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
        canonical_path: String,
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
        canonical_path: String,
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
    pub continuation_fragments: Vec<HookPromptFragment>,
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
                canonical_path,
            } => {
                let mut payload = session_payload(&request);
                payload.turn_id = Some(turn_id);
                payload.agent_id = Some(agent_id);
                payload.agent_type = Some(agent_type);
                payload.detail = format!("path={canonical_path}");
                (crate::SUBAGENT_START, payload)
            }
        };
        if event == crate::SUBAGENT_START {
            let (plugin, commands) = self.dispatch_subagent_start_parts(&payload);
            subagent_start_outcome(plugin, commands)
        } else {
            let (plugin, commands) = self.dispatch_parts(event, &payload);
            admission_outcome(plugin, commands)
        }
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
        let (plugin, commands) = self.dispatch_parts(crate::USER_PROMPT_SUBMIT, &payload);
        let outcome = admission_outcome(plugin, commands);
        UserPromptSubmitOutcome {
            should_stop: outcome.should_stop,
            stop_reason: outcome.stop_reason,
            additional_contexts: outcome.additional_contexts,
        }
    }

    pub fn run_pre_compact(&self, request: PreCompactRequest) -> PreCompactOutcome {
        let payload = compact_payload(request);
        let (plugin, commands) = self.dispatch_parts(crate::PRE_COMPACT, &payload);
        let outcome = admission_outcome(plugin, commands);
        PreCompactOutcome {
            should_stop: outcome.should_stop,
            stop_reason: outcome.stop_reason,
        }
    }

    pub fn run_post_compact(&self, request: PostCompactRequest) -> StatelessHookOutcome {
        let payload = compact_payload(request);
        let (plugin, commands) = self.dispatch_parts(crate::POST_COMPACT, &payload);
        let outcome = admission_outcome(plugin, commands);
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
        let (plugin, commands) = match request.target {
            StopHookTarget::SubagentStop {
                agent_id,
                agent_type,
                canonical_path,
                agent_transcript_path,
            } => {
                payload.agent_id = Some(agent_id);
                payload.agent_type = Some(agent_type);
                payload.detail = format!("path={canonical_path}");
                payload.agent_transcript_path = agent_transcript_path;
                self.dispatch_subagent_stop_parts(&payload)
            }
            StopHookTarget::Stop | StopHookTarget::MemoryConsolidation => {
                self.dispatch_parts(crate::STOP, &payload)
            }
        };
        stop_outcome(plugin, commands)
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

fn admission_outcome(
    plugin: HookOutcome,
    commands: Vec<CommandHookDecision>,
) -> SessionStartOutcome {
    let mut outcome = SessionStartOutcome::default();
    match plugin {
        HookOutcome::Block(reason) => {
            outcome.should_stop = true;
            outcome.stop_reason = Some(reason);
        }
        HookOutcome::InjectContext(context) => outcome.additional_contexts.push(context),
        _ => {}
    }
    for command in commands {
        if outcome.stop_reason.is_none() {
            outcome.stop_reason = command.stop_reason.or(command.block_reason);
        }
        outcome
            .additional_contexts
            .extend(command.additional_context);
    }
    outcome.should_stop = outcome.stop_reason.is_some();
    outcome
}

fn subagent_start_outcome(
    plugin: Option<String>,
    commands: Vec<CommandHookDecision>,
) -> SessionStartOutcome {
    let mut outcome = SessionStartOutcome::default();
    outcome.additional_contexts.extend(plugin);
    outcome.additional_contexts.extend(
        commands
            .into_iter()
            .filter_map(|command| command.additional_context),
    );
    outcome
}

fn stop_outcome(plugin: HookOutcome, commands: Vec<CommandHookDecision>) -> StopOutcome {
    let mut stop_reason = None;
    let mut blocking_reasons = Vec::new();
    let mut continuation_fragments = Vec::new();
    match plugin {
        HookOutcome::Block(reason) | HookOutcome::KeepGoing(reason) => {
            blocking_reasons.push(reason.clone());
            continuation_fragments.push(HookPromptFragment::from_single_hook(
                reason,
                format!(
                    "plugin-hook-run-{}",
                    NEXT_PLUGIN_HOOK_RUN_ID.fetch_add(1, Ordering::Relaxed)
                ),
            ));
        }
        _ => {}
    }
    for command in commands {
        if stop_reason.is_none() {
            stop_reason = command.stop_reason;
        }
        if let Some(reason) = command.keep_going.or(command.block_reason) {
            blocking_reasons.push(reason.clone());
            if let Some(hook_run_id) = command.hook_run_id {
                continuation_fragments
                    .push(HookPromptFragment::from_single_hook(reason, hook_run_id));
            }
        }
    }
    if let Some(stop_reason) = stop_reason {
        return StopOutcome {
            should_stop: true,
            stop_reason: Some(stop_reason),
            ..Default::default()
        };
    }
    StopOutcome {
        should_block: !blocking_reasons.is_empty(),
        block_reason: (!blocking_reasons.is_empty()).then(|| blocking_reasons.join("\n\n")),
        continuation_fragments,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{HookHandlerConfig, HooksFile, MatcherGroup, PluginHookBus};

    fn runtime() -> HookRuntime {
        HookRuntime::with_plugin_bus(Arc::new(PluginHookBus::new()))
    }

    fn runtime_with_command(event: &str, command: &str) -> HookRuntime {
        let mut runtime = runtime();
        runtime.command = Arc::new(
            crate::CommandHookRunner::from_file(
                HooksFile {
                    hooks: std::collections::HashMap::from([(
                        event.to_string(),
                        vec![MatcherGroup {
                            matcher: None,
                            hooks: vec![HookHandlerConfig::Command {
                                command: command.to_string(),
                                command_windows: None,
                                timeout_sec: Some(2),
                                r#async: false,
                                status_message: None,
                                additional_context_limit: None,
                            }],
                        }],
                    )]),
                    ..Default::default()
                },
                std::path::Path::new("hooks.json"),
            )
            .unwrap(),
        );
        runtime
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
    fn session_start_keeps_context_when_another_handler_stops() {
        let runtime = runtime_with_command(
            crate::SESSION_START,
            r#"printf '%s' '{"continue":false,"stopReason":"policy stop"}'"#,
        );
        runtime.plugin.register(crate::SESSION_START, |_| {
            HookOutcome::InjectContext("startup context".into())
        });

        let outcome = runtime.run_session_start(session_start_request());

        assert!(outcome.should_stop);
        assert_eq!(outcome.stop_reason.as_deref(), Some("policy stop"));
        assert_eq!(outcome.additional_contexts, ["startup context"]);
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
                canonical_path: "/root/child".into(),
                agent_transcript_path: None,
            },
        });

        assert_eq!(outcome.continuation_fragments.len(), 1);
        assert_eq!(outcome.continuation_fragments[0].text, "verify tests");
        assert!(outcome.continuation_fragments[0]
            .hook_run_id
            .starts_with("plugin-hook-run-"));
        assert!(outcome.should_block);
        assert_eq!(outcome.block_reason.as_deref(), Some("verify tests"));
    }

    #[test]
    fn stop_continue_false_is_terminal_not_a_continuation() {
        let runtime = runtime_with_command(
            crate::STOP,
            r#"printf '%s' '{"continue":false,"stopReason":"terminate"}'"#,
        );

        let outcome = runtime.run_stop(StopRequest {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            cwd: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            transcript_path: None,
            model: "openai/gpt-5".into(),
            permission_mode: "workspace-write".into(),
            stop_hook_active: false,
            last_assistant_message: Some("done".into()),
            target: StopHookTarget::Stop,
        });

        assert!(outcome.should_stop);
        assert_eq!(outcome.stop_reason.as_deref(), Some("terminate"));
        assert!(!outcome.should_block);
        assert!(outcome.continuation_fragments.is_empty());
    }

    #[test]
    fn stop_block_returns_feedback_and_continuation() {
        let runtime = runtime_with_command(
            crate::STOP,
            r#"printf '%s' '{"decision":"block","reason":"keep working"}'"#,
        );

        let outcome = runtime.run_stop(StopRequest {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            cwd: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            transcript_path: None,
            model: "openai/gpt-5".into(),
            permission_mode: "workspace-write".into(),
            stop_hook_active: false,
            last_assistant_message: Some("done".into()),
            target: StopHookTarget::Stop,
        });

        assert!(!outcome.should_stop);
        assert!(outcome.should_block);
        assert_eq!(outcome.block_reason.as_deref(), Some("keep working"));
        assert_eq!(outcome.continuation_fragments.len(), 1);
        assert_eq!(outcome.continuation_fragments[0].text, "keep working");
        let runs = runtime.command.recent_runs();
        assert_eq!(runs.len(), 1);
        assert_eq!(outcome.continuation_fragments[0].hook_run_id, runs[0].id);
    }
}
