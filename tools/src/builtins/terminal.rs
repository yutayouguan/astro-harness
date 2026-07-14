//! 终端工具：在 Agent 工作区内执行 Shell 命令。
//!
//! 通过 `sh -c` 运行命令，默认工作目录为 workspace；可选 `cwd` 指定
//! workspace 内的相对子目录。超时 60 秒。
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
}

/// 向注册表注册 `terminal` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "terminal".to_string(),
        toolset: "terminal".to_string(),
        description: "Run a shell command. Default cwd is the agent workspace (not a jail—commands can still touch paths outside it). Timeout 60s. stdout/stderr capped at 64KiB; for large files use file_ops read with offset/limit."
            .to_string(),
        schema: schema_for_args::<TerminalArgs>(),
        check_fn: None,
        icon: "terminal",
    });
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

    ctx.ensure_workspace()?;
    let cwd = if let Some(rel) = parsed.cwd.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        crate::path_safe::resolve_safe(&ctx.workspace_dir, rel)?
    } else {
        ctx.workspace_dir.clone()
    };
    std::fs::create_dir_all(&cwd)?;

    let child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let timeout = Duration::from_secs(60);
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("命令超时（60s）"))??;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(-1);
    let body = format!("exit={code}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}");
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
        let providers = providers::registry::ProviderRegistry::new();
        let targets = ImageGenTargets::default();
        let ctx = ToolContext {
            memory: &mut memory,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: ws,
            image_gen_targets: &targets,
            providers: &providers,
            session_id: "test".into(),
            chat_api_key: String::new(),
            chat_base_url: String::new(),
            chat_provider: String::new(),
            chat_model: String::new(),
            chat_targets: vec![],
        };

        let n = common::MAX_TOOL_RESULT_BYTES + 8 * 1024;
        let args = serde_json::json!({
            "command": format!("awk 'BEGIN{{for(i=0;i<{n};i++)printf \"a\"}}'"),
        });
        let out = dispatch(&ctx, &args).await.unwrap();
        assert!(out.contains("[truncated]"), "{out}");
        assert!(out.len() < n + 200);
    }
}
