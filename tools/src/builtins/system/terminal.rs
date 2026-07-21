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

/// Arguments for the unified `terminal` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TerminalArgs {
    /// `run` (default) | `list` | `status` | `wait` | `kill`.
    #[serde(default)]
    pub action: Option<String>,
    /// Shell command (required for `run`).
    #[serde(default)]
    pub command: Option<String>,
    /// Optional workspace-relative working subdirectory (`run`).
    #[serde(default)]
    pub cwd: Option<String>,
    /// Timeout seconds: `run` default 60 max 900; `wait` default 30 max 600.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// If true with `run`, start background job and return id immediately.
    #[serde(default)]
    pub background: Option<bool>,
    /// Job id for `status` / `wait` / `kill`.
    #[serde(default)]
    pub id: Option<String>,
    /// Byte offset for `status` / `wait` output paging.
    #[serde(default)]
    pub offset: Option<usize>,
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
background=true returns job id. \
action=list|status|wait|kill: manage background jobs (id required except list; \
status/wait support offset; wait timeout_secs default 30 max 600). \
stdout/stderr capped at 64KiB (run) / 60KiB per poll (jobs)."
            .to_string(),
        schema: schema_for_args::<TerminalArgs>(),
        check_fn: None,
        icon: "terminal",
        ..ToolEntry::lifecycle_defaults()
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
        crate::path_safe::resolve_safe(&root, rel)?
    } else {
        root
    };
    std::fs::create_dir_all(&cwd)?;

    if parsed.background.unwrap_or(false) {
        let cwd_display = parsed
            .cwd
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(".");
        let id = super::jobs::spawn_background(&ctx.session_id, command, &cwd, cwd_display)?;
        return Ok(format!(
            "已在后台启动任务 {id}。\n用 terminal action=status id={id} 轮询输出，action=wait 等待完成，action=kill 终止。"
        ));
    }

    let child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let timeout_secs = parsed
        .timeout_secs
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS);
    let timeout = Duration::from_secs(timeout_secs);
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("命令超时（{timeout_secs}s）"))??;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(-1);
    let body = format!("exit={code}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}");
    let body = if let Some(bus) = &ctx.hook_bus {
        let outcome = bus.fire(
            hooks::TRANSFORM_TERMINAL_OUTPUT,
            &hooks::HookPayload {
                session_id: ctx.session_id.clone(),
                turn_id: ctx.turn_id.clone(),
                tool_name: Some("terminal".to_string()),
                tool_args: Some(args.clone()),
                tool_result: Some(body.clone()),
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
    Ok(common::truncate_tool_result(
        &body,
        common::MAX_TOOL_RESULT_BYTES,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ImageGenTargets, ToolContext};

    #[tokio::test]
    async fn large_stdout_is_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
        let providers = providers::registry::ProviderRegistry::new();
        let targets = ImageGenTargets::default();
        let ctx = ToolContext {
            memory: &mut memory,
            sessions: &sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: ws,
            project_root: None,
            image_gen_targets: &targets,
            providers: &providers,
            session_id: "test".into(),
            turn_id: None,
            chat_api_key: String::new(),
            chat_base_url: String::new(),
            chat_provider: String::new(),
            chat_model: String::new(),
            chat_targets: vec![],
            delegate_runner: None,
            async_spawner: None,
            orchestration_spawner: None,
            hook_bus: None,
        };

        let n = common::MAX_TOOL_RESULT_BYTES + 8 * 1024;
        let args = serde_json::json!({
            "command": format!("awk 'BEGIN{{for(i=0;i<{n};i++)printf \"a\"}}'"),
        });
        let out = dispatch(&ctx, &args).await.unwrap();
        assert!(out.contains("[truncated]"), "{out}");
        assert!(out.len() < n + 200);
    }

    #[tokio::test]
    async fn transform_terminal_output_hook_replaces_before_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
        let providers = providers::registry::ProviderRegistry::new();
        let targets = ImageGenTargets::default();
        let bus = std::sync::Arc::new(hooks::PluginHookBus::new());
        bus.register(hooks::TRANSFORM_TERMINAL_OUTPUT, |_payload| {
            hooks::HookOutcome::ReplaceText("[redacted-terminal-output]".to_string())
        });
        let ctx = ToolContext {
            memory: &mut memory,
            sessions: &sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: ws,
            project_root: None,
            image_gen_targets: &targets,
            providers: &providers,
            session_id: "test".into(),
            turn_id: None,
            chat_api_key: String::new(),
            chat_base_url: String::new(),
            chat_provider: String::new(),
            chat_model: String::new(),
            chat_targets: vec![],
            delegate_runner: None,
            async_spawner: None,
            orchestration_spawner: None,
            hook_bus: Some(bus),
        };

        let n = common::MAX_TOOL_RESULT_BYTES + 8 * 1024;
        let args = serde_json::json!({
            "command": format!("awk 'BEGIN{{for(i=0;i<{n};i++)printf \"a\"}}'"),
        });
        let out = dispatch(&ctx, &args).await.unwrap();
        assert_eq!(out, "[redacted-terminal-output]");
        assert!(!out.contains("[truncated]"), "{out}");
        assert!(out.len() < n);
    }
}
