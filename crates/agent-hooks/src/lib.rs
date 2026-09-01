//! Astro Hook runtime: in-process plugins plus configured command/MCP handlers.
//! Gateway manifests and legacy shell commands remain observational transports.
//!
//! 注册风格对齐：`ctx.register_hook("PostToolUse", callback)`。

pub mod command;
pub mod config;
pub mod context;
pub mod event;
pub mod gateway;
pub mod lifecycle_events;
pub mod mcp;
pub mod names;
pub mod outcome;
pub mod plugin;
pub mod run;
pub mod shell;
pub mod tool_events;
pub mod ui;

pub use command::{
    CommandHookDecision, CommandHookRunner, CommandHookScope, CommandHookSourceSummary,
    CommandHookSummary, CommandHookTrust, HookHandlerConfig, HooksFile, MatcherGroup,
    PermissionVote,
};
pub use config::{default_astro_root, load_config, load_config_or_default, AstroConfig};
pub use context::PluginContext;
pub use event::HookEvent;
pub use gateway::{DiscoveredHook, GatewayHookRegistry, HookManifest};
pub use lifecycle_events::{
    InterruptOutcome, InterruptRequest, PostCompactRequest, PreCompactOutcome, PreCompactRequest,
    SessionEndOutcome, SessionEndRequest, SessionStartOutcome, SessionStartRequest,
    SessionStartSource, StatelessHookOutcome, StopHookTarget, StopOutcome, StopRequest,
    UserPromptSubmitOutcome, UserPromptSubmitRequest,
};
pub use mcp::{HookMcpCall, HookMcpExecutor};
pub use names::*;
pub use outcome::{
    HookInput, HookOutcome, HookPayload, PermissionRequestDecision, PostToolUseDecision,
};
pub use plugin::PluginHookBus;
pub use run::{
    HookExecutionMode, HookHandlerType, HookOutputEntry, HookOutputEntryKind,
    HookRunLifecycleEvent, HookRunObserver, HookRunRecord, HookRunStatus, HookRunStore, HookScope,
    HookSource, HookTrustStatus,
};
pub use shell::{load_shell_runner, ShellHookRunner};
pub use tool_events::{
    PermissionHookDecision, PermissionRequestOutcome, PermissionRequestRequest, PostToolUseOutcome,
    PostToolUseRequest, PreToolUseOutcome, PreToolUseRequest, SubagentHookContext,
};
pub use ui::{
    install_recording, install_ui_timeline, UiHookEvent, UiTimelineGeneration, UiTimelineSlot,
};

use std::sync::Arc;

/// 进程级钩子运行时：三套体系的聚合句柄。
#[derive(Clone)]
pub struct HookRuntime {
    pub plugin: Arc<PluginHookBus>,
    pub gateway: Arc<GatewayHookRegistry>,
    pub shell: Arc<std::sync::Mutex<ShellHookRunner>>,
    pub command: Arc<CommandHookRunner>,
    /// 进程 plugin bus 上的 UI 时间线槽（每轮 chat 热替换 sender）。
    pub ui_slot: UiTimelineSlot,
}

impl Default for HookRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl HookRuntime {
    pub fn new() -> Self {
        Self::with_plugin_bus(Arc::new(PluginHookBus::new()))
    }

    pub fn with_plugin_bus(plugin: Arc<PluginHookBus>) -> Self {
        let ui_slot = UiTimelineSlot::new();
        ui_slot.install(&plugin);
        Self {
            plugin,
            gateway: Arc::new(GatewayHookRegistry::default()),
            shell: Arc::new(std::sync::Mutex::new(ShellHookRunner::default())),
            command: Arc::new(CommandHookRunner::default()),
            ui_slot,
        }
    }

    /// Bind the session-scoped MCP transport used by `mcp_tool` hook handlers.
    pub fn with_mcp_executor(&self, executor: Arc<dyn HookMcpExecutor>) -> Self {
        Self {
            plugin: Arc::clone(&self.plugin),
            gateway: Arc::clone(&self.gateway),
            shell: Arc::clone(&self.shell),
            command: Arc::new(self.command.as_ref().clone().with_mcp_executor(executor)),
            ui_slot: self.ui_slot.clone(),
        }
    }

    /// Load configured command/MCP hooks and discover gateway manifests.
    pub fn bootstrap_from_root(root: &std::path::Path) -> anyhow::Result<Self> {
        let cfg = load_config(root)?;
        let rt = Self::new();
        rt.gateway.discover(root)?;
        rt.gateway.install_logging_fallbacks();
        if let Ok(mut sh) = rt.shell.lock() {
            *sh = ShellHookRunner::from_map(cfg.hooks);
        }
        let command = match CommandHookRunner::load(root) {
            Ok(command) => Arc::new(command),
            Err(error) => {
                tracing::warn!(%error, root = %root.display(), "failed to load hooks.json");
                Arc::new(CommandHookRunner::default())
            }
        };
        Ok(Self { command, ..rt })
    }

    pub fn plugin_context(&self) -> PluginContext<'_> {
        PluginContext::new(&self.plugin, &self.gateway)
    }

    pub fn list_command_hooks(&self) -> Vec<CommandHookSummary> {
        self.command.list()
    }

    pub fn recent_command_hook_runs(&self) -> Vec<HookRunRecord> {
        self.command.recent_runs()
    }

    pub fn set_run_observer(&self, session_id: impl Into<String>, observer: HookRunObserver) {
        self.command.run_store().set_observer(session_id, observer);
    }

    pub fn remove_run_observer(&self, session_id: &str) {
        self.command.run_store().remove_observer(session_id);
    }

    pub fn command_hook_sources(&self) -> Vec<CommandHookSourceSummary> {
        self.command.sources()
    }

    pub fn with_project_commands(
        &self,
        astro_home: &std::path::Path,
        cwd: &std::path::Path,
    ) -> anyhow::Result<Self> {
        let runs = self.command.run_store();
        Ok(Self {
            command: Arc::new(
                CommandHookRunner::load_for_project(astro_home, cwd)?.share_run_store(runs),
            ),
            ..self.clone()
        })
    }

    /// 统一向 Plugin、Gateway、Shell 三套 transport 投递事件。
    pub fn dispatch(&self, name: &str, payload: &HookPayload) -> HookOutcome {
        let (plugin, commands) = self.dispatch_parts(name, payload);
        aggregate_outcomes(plugin, commands)
    }

    pub(crate) fn dispatch_parts(
        &self,
        name: &str,
        payload: &HookPayload,
    ) -> (HookOutcome, Vec<CommandHookDecision>) {
        let payload = payload.for_event(name);
        let plugin = self.plugin.fire(name, &payload);
        self.gateway.fire(name, &payload);
        if let Ok(sh) = self.shell.lock() {
            sh.fire_async(name, &payload);
        }
        let commands = run_command_hooks(&self.command, name, &payload);
        (plugin, commands)
    }

    pub fn dispatch_permission_request(&self, payload: &HookPayload) -> PermissionRequestDecision {
        let payload = payload.for_event(PERMISSION_REQUEST);
        let mut decision = self.plugin.fire_permission_request(&payload);
        self.gateway.fire(PERMISSION_REQUEST, &payload);
        if let Ok(shell) = self.shell.lock() {
            shell.fire_async(PERMISSION_REQUEST, &payload);
        }
        for command in run_command_hooks(&self.command, PERMISSION_REQUEST, &payload) {
            if command.permission == Some(PermissionVote::Deny)
                || command.block_reason.is_some()
                || command.stop_reason.is_some()
            {
                return PermissionRequestDecision::Deny(
                    command
                        .block_reason
                        .or(command.stop_reason)
                        .unwrap_or_else(|| "denied by command hook".into()),
                );
            }
            if command.permission == Some(PermissionVote::Allow) {
                decision = PermissionRequestDecision::Allow;
            }
        }
        decision
    }

    pub fn dispatch_post_tool_use(&self, payload: &HookPayload) -> PostToolUseDecision {
        let payload = payload.for_event(POST_TOOL_USE);
        let mut decision = self.plugin.fire_post_tool_use(&payload);
        self.gateway.fire(POST_TOOL_USE, &payload);
        if let Ok(shell) = self.shell.lock() {
            shell.fire_async(POST_TOOL_USE, &payload);
        }
        for command in run_command_hooks(&self.command, POST_TOOL_USE, &payload) {
            if decision.block_reason.is_none() {
                decision.block_reason = command.block_reason.or(command.stop_reason);
            }
            if let Some(context) = command.additional_context {
                decision.additional_contexts.push(context);
            }
            if let Some(feedback) = command.feedback {
                decision.feedback_messages.push(feedback);
            }
        }
        decision
    }

    pub fn dispatch_subagent_start(&self, payload: &HookPayload) -> Option<String> {
        let (plugin, commands) = self.dispatch_subagent_start_parts(payload);
        let mut contexts = plugin.into_iter().collect::<Vec<_>>();
        contexts.extend(
            commands
                .into_iter()
                .filter_map(|decision| decision.additional_context),
        );
        (!contexts.is_empty()).then(|| contexts.join("\n\n"))
    }

    pub(crate) fn dispatch_subagent_start_parts(
        &self,
        payload: &HookPayload,
    ) -> (Option<String>, Vec<CommandHookDecision>) {
        let payload = payload.for_event(SUBAGENT_START);
        let plugin = self.plugin.fire_subagent_start(&payload);
        self.gateway.fire(SUBAGENT_START, &payload);
        if let Ok(shell) = self.shell.lock() {
            shell.fire_async(SUBAGENT_START, &payload);
        }
        let commands = run_command_hooks(&self.command, SUBAGENT_START, &payload);
        (plugin, commands)
    }

    pub fn dispatch_subagent_stop(&self, payload: &HookPayload) -> HookOutcome {
        let (plugin, commands) = self.dispatch_subagent_stop_parts(payload);
        aggregate_outcomes(plugin, commands)
    }

    pub(crate) fn dispatch_subagent_stop_parts(
        &self,
        payload: &HookPayload,
    ) -> (HookOutcome, Vec<CommandHookDecision>) {
        let payload = payload.for_event(SUBAGENT_STOP);
        let plugin = self.plugin.fire_subagent_stop(&payload);
        self.gateway.fire(SUBAGENT_STOP, &payload);
        if let Ok(shell) = self.shell.lock() {
            shell.fire_async(SUBAGENT_STOP, &payload);
        }
        let commands = run_command_hooks(&self.command, SUBAGENT_STOP, &payload);
        (plugin, commands)
    }

    /// 兼容包装：统一向三套 transport 投递事件。
    pub fn fire_plugin(&self, name: &str, payload: &HookPayload) -> HookOutcome {
        self.dispatch(name, payload)
    }

    /// 兼容包装：统一向三套 transport 投递事件。
    pub fn fire_gateway(&self, event: &str, payload: &HookPayload) {
        let _ = self.dispatch(event, payload);
    }
}

fn run_command_hooks(
    runner: &Arc<CommandHookRunner>,
    event: &str,
    payload: &HookPayload,
) -> Vec<CommandHookDecision> {
    if runner.is_empty() {
        return Vec::new();
    }
    let runner = Arc::clone(runner);
    let event = event.to_string();
    let payload = payload.clone();
    match std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map(|runtime| runtime.block_on(runner.run(&event, &payload)))
    })
    .join()
    {
        Ok(Ok(decisions)) => decisions,
        Ok(Err(error)) => {
            tracing::warn!(%error, "failed to initialize command hook runtime");
            Vec::new()
        }
        Err(_) => {
            tracing::warn!("command hook runtime panicked");
            Vec::new()
        }
    }
}

fn aggregate_outcomes(plugin: HookOutcome, commands: Vec<CommandHookDecision>) -> HookOutcome {
    let mut block = None;
    let mut update = None;
    let mut update_completion_order = None;
    let mut contexts = Vec::new();
    let mut continuations = Vec::new();
    match plugin {
        HookOutcome::Block(reason) => block = Some(reason),
        HookOutcome::Modify(value) => update = Some(value),
        HookOutcome::InjectContext(context) => contexts.push(context),
        HookOutcome::KeepGoing(prompt) => continuations.push(prompt),
        other @ (HookOutcome::ReplaceText(_) | HookOutcome::Skip(_) | HookOutcome::Rewrite(_)) => {
            return other;
        }
        HookOutcome::Continue | HookOutcome::Allow => {}
    }
    for command in commands {
        if block.is_none() {
            block = command.block_reason.or(command.stop_reason);
        }
        if let Some(candidate) = command.updated_input {
            let completion_order = command.completion_order.unwrap_or_default();
            if update_completion_order.is_none_or(|current| completion_order >= current) {
                update = Some(candidate);
                update_completion_order = Some(completion_order);
            }
        }
        contexts.extend(command.additional_context);
        continuations.extend(command.keep_going);
    }
    if let Some(reason) = block {
        return HookOutcome::Block(reason);
    }
    if let Some(update) = update {
        return HookOutcome::Modify(update);
    }
    if !continuations.is_empty() {
        return HookOutcome::KeepGoing(continuations.join("\n\n"));
    }
    if !contexts.is_empty() {
        return HookOutcome::InjectContext(contexts.join("\n\n"));
    }
    HookOutcome::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn runtime_with_command(event: &str, command: &str) -> HookRuntime {
        let mut runtime = HookRuntime::new();
        runtime.command = Arc::new(
            CommandHookRunner::from_file(
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

    #[test]
    fn command_pre_tool_use_can_rewrite_input_through_unified_dispatch() {
        let runtime = runtime_with_command(
            PRE_TOOL_USE,
            r#"printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"safe":true}}}'"#,
        );
        let outcome = runtime.dispatch(
            PRE_TOOL_USE,
            &HookPayload {
                cwd: std::env::current_dir()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                tool_name: Some("terminal".into()),
                ..Default::default()
            },
        );
        assert!(matches!(
            outcome,
            HookOutcome::Modify(value) if value == serde_json::json!({"safe": true})
        ));
    }

    #[test]
    fn command_permission_denial_wins_over_plugin_allow() {
        let runtime = runtime_with_command(
            PERMISSION_REQUEST,
            r#"printf '%s' '{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"deny","message":"policy"}}}'"#,
        );
        runtime
            .plugin
            .register(PERMISSION_REQUEST, |_| HookOutcome::Allow);
        let decision = runtime.dispatch_permission_request(&HookPayload {
            cwd: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            tool_name: Some("terminal".into()),
            ..Default::default()
        });
        assert_eq!(decision, PermissionRequestDecision::Deny("policy".into()));
    }

    #[test]
    fn latest_completed_command_rewrite_wins() {
        let outcome = aggregate_outcomes(
            HookOutcome::Modify(serde_json::json!({"source":"plugin"})),
            vec![
                CommandHookDecision {
                    updated_input: Some(serde_json::json!({"source":"late"})),
                    completion_order: Some(1),
                    ..Default::default()
                },
                CommandHookDecision {
                    updated_input: Some(serde_json::json!({"source":"early"})),
                    completion_order: Some(0),
                    ..Default::default()
                },
            ],
        );

        assert!(matches!(
            outcome,
            HookOutcome::Modify(value) if value == serde_json::json!({"source":"late"})
        ));
    }

    #[tokio::test]
    async fn runtime_does_not_bridge_legacy_shell_keys_to_canonical_events() {
        let rt = HookRuntime::new();
        let seen = Arc::new(std::sync::Mutex::new(None));
        let capture = Arc::clone(&seen);
        rt.plugin.register(PRE_TOOL_USE, move |input| {
            *capture.lock().unwrap() = Some(input.hook_event_name.clone());
            HookOutcome::Continue
        });
        *rt.shell.lock().unwrap() = ShellHookRunner::new(std::collections::HashMap::from([(
            "pre_tool_call".to_string(),
            "true".to_string(),
        )]));

        let _ = rt.fire_plugin(PRE_TOOL_USE, &HookInput::default());

        assert_eq!(seen.lock().unwrap().as_deref(), Some(PRE_TOOL_USE));
        let shell_schedule = rt.shell.lock().unwrap().scheduled();
        assert!(shell_schedule.is_empty());
    }

    #[tokio::test]
    async fn dispatch_reaches_all_transports_with_canonical_name() {
        let dir = tempfile::tempdir().unwrap();
        let hook_dir = dir.path().join("hooks").join("audit");
        std::fs::create_dir_all(&hook_dir).unwrap();
        std::fs::write(
            hook_dir.join("HOOK.yaml"),
            "name: audit\nevents:\n  - PreToolUse\n",
        )
        .unwrap();

        let rt = HookRuntime::new();
        rt.gateway.discover(dir.path()).unwrap();
        let gateway_hits = Arc::new(AtomicUsize::new(0));
        let gateway_hit_count = Arc::clone(&gateway_hits);
        rt.gateway.register_handler("audit", move |event, input| {
            assert_eq!(event, PRE_TOOL_USE);
            assert_eq!(input.hook_event_name, PRE_TOOL_USE);
            gateway_hit_count.fetch_add(1, Ordering::SeqCst);
        });
        let plugin_hits = Arc::new(AtomicUsize::new(0));
        let plugin_hit_count = Arc::clone(&plugin_hits);
        rt.plugin.register(PRE_TOOL_USE, move |input| {
            assert_eq!(input.hook_event_name, PRE_TOOL_USE);
            plugin_hit_count.fetch_add(1, Ordering::SeqCst);
            HookOutcome::Block("blocked".into())
        });
        *rt.shell.lock().unwrap() = ShellHookRunner::new(std::collections::HashMap::from([(
            PRE_TOOL_USE.to_string(),
            "true".to_string(),
        )]));

        let out = rt.dispatch(PRE_TOOL_USE, &HookInput::default());

        assert!(matches!(out, HookOutcome::Block(ref reason) if reason == "blocked"));
        assert_eq!(plugin_hits.load(Ordering::SeqCst), 1);
        assert_eq!(gateway_hits.load(Ordering::SeqCst), 1);
        let shell_schedule = rt.shell.lock().unwrap().scheduled();
        assert_eq!(shell_schedule.len(), 1);
        assert_eq!(shell_schedule[0].0, PRE_TOOL_USE);
        assert_eq!(shell_schedule[0].1.hook_event_name, PRE_TOOL_USE);
    }

    #[tokio::test]
    async fn runtime_routes_gateway_and_shell_by_canonical_name() {
        let dir = tempfile::tempdir().unwrap();
        let hook_dir = dir.path().join("hooks").join("audit");
        std::fs::create_dir_all(&hook_dir).unwrap();
        std::fs::write(
            hook_dir.join("HOOK.yaml"),
            "name: audit\nevents:\n  - GatewayStartup\n",
        )
        .unwrap();
        let rt = HookRuntime::new();
        rt.gateway.discover(dir.path()).unwrap();
        let seen = Arc::new(std::sync::Mutex::new(None));
        let capture = Arc::clone(&seen);
        rt.gateway.register_handler("audit", move |event, input| {
            assert_eq!(input.hook_event_name, GATEWAY_STARTUP);
            *capture.lock().unwrap() = Some(event.to_owned());
        });
        *rt.shell.lock().unwrap() = ShellHookRunner::new(std::collections::HashMap::from([(
            GATEWAY_STARTUP.to_string(),
            "true".to_string(),
        )]));

        rt.fire_gateway(GATEWAY_STARTUP, &HookInput::default());

        assert_eq!(seen.lock().unwrap().as_deref(), Some(GATEWAY_STARTUP));
        assert!(rt.shell.lock().unwrap().has_event(GATEWAY_STARTUP));
    }

    #[test]
    fn fire_gateway_invokes_handler_and_shell_map() {
        let rt = HookRuntime::new();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        // 无清单时 fire 不会调 handler；先手工注入 discovered+handler 路径：
        // 用 register_handler + 直接 gateway.fire 已在 gateway 模块测过。
        // 这里验证 HookRuntime 聚合：plugin 旁路不误伤 + shell 不 panic。
        rt.gateway.register_handler("noop", move |_, _| {
            h.fetch_add(1, Ordering::SeqCst);
        });
        // 无 discovered 清单时 handler 不会被调；仅确保 fire_gateway 可调用。
        rt.fire_gateway(
            GATEWAY_STARTUP,
            &HookPayload {
                detail: "test".into(),
                ..Default::default()
            },
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        let _ = rt.fire_plugin(SESSION_RESET, &HookPayload::default());
    }

    #[test]
    fn pre_gateway_dispatch_skip_short_circuits() {
        let rt = HookRuntime::new();
        rt.plugin.register(PRE_GATEWAY_DISPATCH, |_| {
            HookOutcome::Skip("blocked-by-test".into())
        });
        let out = rt.fire_plugin(
            PRE_GATEWAY_DISPATCH,
            &HookPayload {
                prompt: Some("hello".into()),
                ..Default::default()
            },
        );
        assert!(matches!(out, HookOutcome::Skip(ref s) if s == "blocked-by-test"));
    }

    #[test]
    fn pre_gateway_dispatch_rewrite() {
        let rt = HookRuntime::new();
        rt.plugin.register(PRE_GATEWAY_DISPATCH, |_| {
            HookOutcome::Rewrite("rewritten".into())
        });
        let out = rt.fire_plugin(PRE_GATEWAY_DISPATCH, &HookPayload::default());
        assert!(matches!(out, HookOutcome::Rewrite(ref s) if s == "rewritten"));
    }
}
