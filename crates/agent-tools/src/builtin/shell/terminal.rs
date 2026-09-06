//! 终端工具：在 Agent 工作区内执行 Shell 命令，并管理后台任务。
//!
//! - `action=run`（默认）：通过 `sh -c` 运行命令
//! - `action=list|status|wait|kill`：管理 `background=true` 启动的后台任务
//!
//! 默认工作目录为 workspace；可选 `cwd` 指定相对子目录。
//! 默认超时 60 秒（`timeout_secs`，上限 900s）。stdout/stderr 有 64KiB 截断。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// 统一 `terminal` 工具的参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TerminalArgs {
    /// `run`（默认）| `list` | `status` | `wait` | `kill`。
    #[serde(default)]
    pub action: Option<String>,
    /// Shell 命令（`run` 时必需）。
    #[serde(default)]
    pub command: Option<String>,
    /// 可选的工作区相对子目录（`run` 时使用）。
    #[serde(default)]
    pub cwd: Option<String>,
    /// 超时秒数：`run` 默认 60 上限 900；`wait` 默认 30 上限 600。
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// `run` 时设为 true 则在后台启动任务并立即返回 id。
    #[serde(default)]
    pub background: Option<bool>,
    /// 任务 id，用于 `status` / `wait` / `kill`。
    #[serde(default)]
    pub id: Option<String>,
    /// `status` / `wait` 输出分页的字节偏移量。
    #[serde(default)]
    pub offset: Option<usize>,
    /// 使用共享 PTY 终端；用户可在 Desktop Terminal Dock 中看到并接管。
    #[serde(default)]
    pub tty: Option<bool>,
    /// PTY 命令写入后等待输出的毫秒数。
    #[serde(default)]
    pub yield_time_ms: Option<i64>,
    /// PTY 返回输出的 token 预算。
    #[serde(default)]
    pub max_output_tokens: Option<usize>,
}

/// `timeout_secs` 上限，防止命令永久挂起占用执行器。
const MAX_TIMEOUT_SECS: u64 = 900;

/// 默认超时（未显式指定 `timeout_secs` 时）。
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// 向注册表注册 `terminal` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "terminal".to_string(),
        toolset: "terminal".to_string(),
        description: "Run a shell command or manage background jobs. \
action=run (default): command required; cwd=project_root or workspace; timeout_secs max 900; \
background=true returns job id; tty=true runs in the shared Desktop terminal session. \
action=list|status|wait|kill: manage background jobs (id required except list; \
status/wait support offset; wait timeout_secs default 30 max 600). \
stdout/stderr capped at 64KiB (run) / 60KiB per poll (jobs)."
            .to_string(),
        schema: schema_for_args::<TerminalArgs>(),
        check_fn: None,
        icon: "terminal",
        ..ToolEntry::lifecycle_defaults().sandboxable()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["terminal"],
    async_ctx: dispatch,
}

/// 分发 `run` 或后台任务管理。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: TerminalArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("terminal 参数无效: {e}"))?;
    let action = parsed
        .action
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("run")
        .to_ascii_lowercase();

    if matches!(
        action.as_str(),
        "list" | "status" | "poll" | "wait" | "kill"
    ) {
        return super::jobs::dispatch_job_action(
            ctx,
            &action,
            parsed.id.as_deref(),
            parsed.offset,
            parsed.timeout_secs,
        )
        .await;
    }
    if action != "run" {
        anyhow::bail!("未知 action: {action}（应为 run|list|status|wait|kill）");
    }

    dispatch_run(ctx, args, &parsed).await
}

async fn dispatch_run(
    ctx: &ToolContext<'_>,
    args: &serde_json::Value,
    parsed: &TerminalArgs,
) -> anyhow::Result<String> {
    use std::process::Stdio;
    use std::time::Duration;

    let command = parsed
        .command
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("terminal run 需要 command"))?;

    let root = ctx.ensure_project_or_workspace()?;
    let cwd = if let Some(rel) = parsed
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        crate::path_safe::resolve_safe_in_roots(&root, &ctx.workspace_roots, rel)?
    } else {
        root
    };
    std::fs::create_dir_all(&cwd)?;
    let audit = ctx.sandbox_audit_metadata("terminal");

    if parsed.tty.unwrap_or(false) {
        if parsed.background.unwrap_or(false) {
            anyhow::bail!("PTY terminal sessions cannot run with background=true");
        }
        return super::exec_command::run_in_shared_terminal(
            ctx,
            command,
            &root,
            &cwd,
            parsed.yield_time_ms,
            parsed.max_output_tokens,
        )
        .await;
    }

    if parsed.background.unwrap_or(false) {
        if ctx.managed_network.is_some() {
            anyhow::bail!(
                "managed network does not support background jobs; use foreground terminal instead"
            );
        }
        let cwd_display = parsed
            .cwd
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(".");
        let policy = ctx.command_sandbox_policy().inspect_err(|_error| {
            audit.record(
                sandbox::SandboxAuditKind::Denied,
                None,
                "sh",
                "policy_resolution_failed",
                None,
            );
        })?;
        let id = super::jobs::spawn_background_sandboxed(
            &ctx.session_id,
            command,
            &cwd,
            cwd_display,
            &policy,
            Some(&audit),
        )?;
        return Ok(format!(
            "已在后台启动任务 {id}。\n用 terminal action=status id={id} 轮询输出，action=wait 等待完成，action=kill 终止。"
        ));
    }

    let policy = ctx.command_sandbox_policy().inspect_err(|_error| {
        audit.record(
            sandbox::SandboxAuditKind::Denied,
            None,
            "sh",
            "policy_resolution_failed",
            None,
        );
    })?;
    let spawn_started = std::time::Instant::now();
    let mut sandboxed_command = match sandbox::SandboxRunner.tokio_command(&policy, "sh") {
        Ok(command) => command,
        Err(error) => {
            audit.record_prepare_error(
                Some(&policy),
                "sh",
                &error,
                Some(spawn_started.elapsed().as_millis() as u64),
            );
            return Err(error.into());
        }
    };
    sandboxed_command
        .arg("-c")
        .arg(command)
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if ctx.managed_network.is_some() {
        let prepared = ctx
            .prepare_managed_network_env(std::env::vars().collect())
            .expect("managed network lease checked above");
        sandboxed_command.env_clear().envs(prepared.env);
    }
    let child = sandboxed_command.spawn().inspect_err(|_error| {
        audit.record(
            sandbox::SandboxAuditKind::Denied,
            Some(&policy),
            "sh",
            "spawn_failed",
            Some(spawn_started.elapsed().as_millis() as u64),
        );
    })?;
    audit.record(
        sandbox::SandboxAuditKind::Spawned,
        Some(&policy),
        "sh",
        "spawned",
        Some(spawn_started.elapsed().as_millis() as u64),
    );

    let timeout_secs = parsed
        .timeout_secs
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS);
    let timeout = Duration::from_secs(timeout_secs);
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("命令超时（{timeout_secs}s）"))??;

    let code = output.status.code().unwrap_or(-1);
    let output = sandbox::ExecToolCallOutput::new(
        code,
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    );
    if let Some(decision) = ctx.take_managed_network_denial() {
        audit.record(
            sandbox::SandboxAuditKind::Denied,
            Some(&policy),
            "sh",
            "network_policy_denied",
            Some(spawn_started.elapsed().as_millis() as u64),
        );
        return Err(sandbox::SandboxErr::Denied {
            output: Box::new(output),
            network_policy_decision: Some(decision),
        }
        .into());
    }
    let sandbox_denied = sandbox::is_likely_sandbox_denied(policy.mode, &output);
    let body = output.render_text();
    let body = if let Some(bus) = &ctx.hook_bus {
        let outcome = bus.fire(
            hooks::TRANSFORM_TERMINAL_OUTPUT,
            &hooks::HookPayload {
                session_id: ctx.session_id.clone(),
                turn_id: ctx.turn_id.clone(),
                tool_name: Some("terminal".to_string()),
                tool_input: Some(args.clone()),
                tool_response: Some(serde_json::Value::String(body.clone())),
                ..Default::default()
            },
        );
        match outcome {
            hooks::HookOutcome::ReplaceText(s) => s,
            _ => body,
        }
    } else {
        body
    };
    if sandbox_denied {
        audit.record(
            sandbox::SandboxAuditKind::Denied,
            Some(&policy),
            "sh",
            "sandbox_denied",
            Some(spawn_started.elapsed().as_millis() as u64),
        );
        return Err(sandbox::SandboxErr::Denied {
            output: Box::new(output.with_aggregated_output(body)),
            network_policy_decision: None,
        }
        .into());
    }
    Ok(types::truncate_tool_result(
        &body,
        types::MAX_TOOL_RESULT_BYTES,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ImageGenTargets, ToolContext};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn test_ctx<'a>(
        dir: &'a tempfile::TempDir,
        memory: &'a std::sync::RwLock<memory::MemoryManager>,
        sessions: &'a session::SessionStore,
        targets: &'a ImageGenTargets,
        creds: &'a crate::context::ModelCredentials,
        session_id: &str,
    ) -> ToolContext<'a> {
        let workspace = dir.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        ToolContext {
            memory,
            sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: workspace,
            project_root: None,
            workspace_roots: Vec::new(),
            image_gen_targets: targets,
            session_id: session_id.into(),
            turn_id: None,
            credentials: creds,
            service_tier: None,
            model_targets: &[],
            execution: None,
            permission_profile: None,
            skill_config_overrides: &[],
            hook_bus: None,
            hook_runtime: None,
            workspace_write_grant: false,
            sandbox_policy: None,
            managed_network: None,
            context_window: None,
            context_tokens_used: None,
            tool_registry: None,
        }
    }

    async fn enable_managed_network(ctx: &mut ToolContext<'_>) -> String {
        let started = Arc::new(
            network_proxy::StartedNetworkProxy::start(Arc::new(
                network_proxy::NetworkProxyState::new(types::NetworkPolicy {
                    enabled: true,
                    domains: BTreeMap::from([(
                        "allowed.example".into(),
                        types::NetworkAccess::Allow,
                    )]),
                    ..Default::default()
                })
                .unwrap(),
            ))
            .await
            .unwrap(),
        );
        let prepared = started.proxy().prepare(Default::default());
        let endpoint = prepared.env["HTTPS_PROXY"].clone();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::WorkspaceWrite,
            ctx.project_or_workspace(),
            Vec::new(),
            false,
        )
        .unwrap()
        .with_managed_network(prepared.sandbox_context);
        ctx.sandbox_policy = Some(policy);
        ctx.managed_network = Some(started);
        endpoint
    }

    #[tokio::test]
    async fn terminal_uses_managed_proxy_environment() {
        let dir = tempfile::tempdir().unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("data"))
            .await
            .unwrap();
        let memory = std::sync::RwLock::new(memory);
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let mut ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds, "managed-env");
        let endpoint = enable_managed_network(&mut ctx).await;

        let output = dispatch(
            &ctx,
            &serde_json::json!({"command": "printf '%s' \"$HTTPS_PROXY\""}),
        )
        .await
        .unwrap();

        assert!(output.contains(&endpoint), "{output}");
    }

    #[tokio::test]
    async fn terminal_managed_network_rejects_background_before_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("data"))
            .await
            .unwrap();
        let memory = std::sync::RwLock::new(memory);
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let mut ctx = test_ctx(
            &dir,
            &memory,
            &sessions,
            &targets,
            &creds,
            "managed-background",
        );
        enable_managed_network(&mut ctx).await;

        let error = dispatch(
            &ctx,
            &serde_json::json!({"command": "sleep 30", "background": true}),
        )
        .await
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("managed network does not support background jobs"),
            "{error}"
        );
        let jobs = dispatch(&ctx, &serde_json::json!({"action": "list"}))
            .await
            .unwrap();
        assert!(jobs.contains("当前会话没有后台任务"), "{jobs}");
    }

    #[tokio::test]
    async fn terminal_managed_network_denial_is_typed() {
        let dir = tempfile::tempdir().unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("data"))
            .await
            .unwrap();
        let memory = std::sync::RwLock::new(memory);
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let mut ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds, "managed-denial");
        enable_managed_network(&mut ctx).await;
        let script = r#"python3 - <<'PY'
import os, socket
endpoint = os.environ['HTTPS_PROXY'].removeprefix('http://')
host, port = endpoint.rsplit(':', 1)
sock = socket.create_connection((host, int(port)))
sock.sendall(b'CONNECT 127.0.0.1:9 HTTP/1.1\r\nHost: 127.0.0.1:9\r\n\r\n')
print(sock.recv(4096).decode())
PY"#;

        let error = dispatch(&ctx, &serde_json::json!({"command": script}))
            .await
            .unwrap_err();
        let Some(sandbox::SandboxErr::Denied {
            network_policy_decision: Some(decision),
            ..
        }) = error.downcast_ref::<sandbox::SandboxErr>()
        else {
            panic!("expected typed managed-network denial: {error}");
        };
        assert_eq!(decision.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(decision.port, Some(9));
        assert_eq!(decision.decision, types::NetworkPolicyDecision::Deny);
        assert_eq!(
            decision.source,
            types::NetworkDecisionSource::BaselinePolicy
        );
    }

    #[tokio::test]
    async fn terminal_without_managed_network_keeps_inherited_environment() {
        let dir = tempfile::tempdir().unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("data"))
            .await
            .unwrap();
        let memory = std::sync::RwLock::new(memory);
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds, "unmanaged-env");

        let output = dispatch(
            &ctx,
            &serde_json::json!({"command": "printf '%s' \"${HOME:+present}\""}),
        )
        .await
        .unwrap();
        assert!(output.contains("present"), "{output}");
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn foreground_denial_returns_typed_sandbox_error() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        memory::set_permission_preset(dir.path(), types::PermissionPreset::ReadOnly).unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("data"))
            .await
            .unwrap();
        let memory = std::sync::RwLock::new(memory);
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let bus = std::sync::Arc::new(hooks::PluginHookBus::new());
        bus.register(hooks::TRANSFORM_TERMINAL_OUTPUT, |_payload| {
            hooks::HookOutcome::ReplaceText("[redacted-denial]".into())
        });
        let ctx = ToolContext {
            memory: &memory,
            sessions: &sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: ws.clone(),
            project_root: None,
            workspace_roots: Vec::new(),
            image_gen_targets: &targets,
            session_id: "test".into(),
            turn_id: None,
            credentials: &creds,
            model_targets: &[],
            execution: None,
            permission_profile: Some(types::READ_ONLY_PROFILE.into()),
            skill_config_overrides: &[],
            hook_bus: Some(bus),
            hook_runtime: None,
            workspace_write_grant: false,
            sandbox_policy: None,
            managed_network: None,
            context_window: None,
            context_tokens_used: None,
            tool_registry: None,
        };

        let error = dispatch(&ctx, &serde_json::json!({"command": "touch denied.txt"}))
            .await
            .unwrap_err();
        let Some(sandbox::SandboxErr::Denied { output, .. }) =
            error.downcast_ref::<sandbox::SandboxErr>()
        else {
            panic!("expected typed sandbox denial: {error}");
        };
        assert_ne!(output.exit_code, 0);
        assert_eq!(output.aggregated_output, "[redacted-denial]");
        assert!(!ws.join("denied.txt").exists());
    }

    #[tokio::test]
    async fn large_stdout_is_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("data"))
            .await
            .unwrap();
        let memory = std::sync::RwLock::new(memory);
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let ctx = ToolContext {
            memory: &memory,
            sessions: &sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: ws,
            project_root: None,
            workspace_roots: Vec::new(),
            image_gen_targets: &targets,
            session_id: "test".into(),
            turn_id: None,
            credentials: &creds,
            model_targets: &[],
            execution: None,
            permission_profile: None,
            skill_config_overrides: &[],
            hook_bus: None,
            hook_runtime: None,
            workspace_write_grant: false,
            sandbox_policy: None,
            managed_network: None,
            context_window: None,
            context_tokens_used: None,
            tool_registry: None,
        };

        let n = types::MAX_TOOL_RESULT_BYTES + 8 * 1024;
        let args = serde_json::json!({
            "command": format!("awk 'BEGIN{{for(i=0;i<{n};i++)printf \"a\"}}'"),
        });
        let out = dispatch(&ctx, &args).await.unwrap();
        assert!(out.contains("[truncated]"), "{out}");
        assert!(out.len() < n + 200);
        let audits = sandbox::list_recent_sandbox_audits(dir.path(), 10).unwrap();
        assert!(audits.iter().any(|event| {
            event.event == sandbox::SandboxAuditKind::Spawned
                && event.tool_name == "terminal"
                && event.target == "sh"
        }));
    }

    #[tokio::test]
    async fn transform_terminal_output_hook_replaces_before_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("data"))
            .await
            .unwrap();
        let memory = std::sync::RwLock::new(memory);
        let targets = ImageGenTargets::default();
        let bus = std::sync::Arc::new(hooks::PluginHookBus::new());
        bus.register(hooks::TRANSFORM_TERMINAL_OUTPUT, |_payload| {
            hooks::HookOutcome::ReplaceText("[redacted-terminal-output]".to_string())
        });
        let creds = crate::context::ModelCredentials::default();
        let ctx = ToolContext {
            memory: &memory,
            sessions: &sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: ws,
            project_root: None,
            workspace_roots: Vec::new(),
            image_gen_targets: &targets,
            session_id: "test".into(),
            turn_id: None,
            credentials: &creds,
            model_targets: &[],
            execution: None,
            permission_profile: None,
            skill_config_overrides: &[],
            hook_bus: Some(bus),
            hook_runtime: None,
            workspace_write_grant: false,
            sandbox_policy: None,
            managed_network: None,
            context_window: None,
            context_tokens_used: None,
            tool_registry: None,
        };

        let n = types::MAX_TOOL_RESULT_BYTES + 8 * 1024;
        let args = serde_json::json!({
            "command": format!("awk 'BEGIN{{for(i=0;i<{n};i++)printf \"a\"}}'"),
        });
        let out = dispatch(&ctx, &args).await.unwrap();
        assert_eq!(out, "[redacted-terminal-output]");
        assert!(!out.contains("[truncated]"), "{out}");
        assert!(out.len() < n);
    }
}
