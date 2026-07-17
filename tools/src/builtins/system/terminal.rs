//! 终端工具：在 Agent 工作区内执行 Shell 命令。
//!
//! 通过 `sh -c` 运行命令，默认工作目录为 workspace；可选 `cwd` 指定
//! workspace 内的相对子目录。默认超时 60 秒，可用 `timeout_secs` 调整（上限 900s），
//! 便于构建 / 测试 / 装依赖等长任务。
//!
//! **注意**：仅默认 cwd 落在 workspace，命令本身可访问整机路径；stdout/stderr
//! 有 64KiB 截断以防撑爆上下文。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `terminal` 工具的参数结构。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TerminalArgs {
    /// 要执行的 Shell 命令字符串。
    pub command: String,
    /// 可选：相对于 workspace 的工作子目录。
    #[serde(default)]
    pub cwd: Option<String>,
    /// 可选：超时秒数（默认 60，钳制到 1..=900）；用于构建 / 测试等长任务。
    #[serde(default)]
    pub timeout_secs: Option<u64>,
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
        description: "Run a shell command. Default cwd is project_root when set (e.g. delegated git worktree), else the agent memory workspace (not a jail—commands can still touch paths outside it). Default timeout 60s, override with timeout_secs (max 900s) for builds/tests/installs. stdout/stderr capped at 64KiB; for large files use file_ops read with offset/limit."
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

/// 在 workspace（或指定子目录）下执行 Shell 命令并返回退出码、stdout、stderr。
///
/// `cwd` 经 `resolve_safe` 校验；命令为空时立即报错。输出经统一截断。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    use std::process::Stdio;
    use std::time::Duration;

    let parsed: TerminalArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("terminal 参数无效: {e}"))?;
    let command = parsed.command.trim();
    if command.is_empty() {
        anyhow::bail!("terminal 需要 command");
    }

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
