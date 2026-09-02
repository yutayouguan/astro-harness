//! 向运行中的 exec_command 会话写入 stdin 并返回最近输出。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

fn default_max_output_tokens() -> usize {
    10000
}

/// `write_stdin` 工具的参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WriteStdinArgs {
    /// 运行中的 exec_command 会话 ID；省略时使用当前项目的 Desktop 终端。
    pub session_id: Option<u64>,
    /// 写入 stdin 的内容。为空或省略时 = 仅轮询不写入。
    #[serde(default)]
    pub chars: Option<String>,
    /// 返回输出前的等待时间（毫秒）。
    /// 非空写入默认 250 ms；空轮询默认 5000 ms。
    #[serde(default)]
    pub yield_time_ms: Option<i64>,
    /// 输出 token 预算。默认 10000。
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: usize,
}

/// 向注册表注册 `write_stdin` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "write_stdin".to_string(),
        toolset: "exec_command".to_string(),
        description: "Writes characters to a shared exec_command/Desktop terminal session and returns new output. session_id is optional when the current project has an active terminal; use empty chars to poll."
            .to_string(),
        schema: schema_for_args::<WriteStdinArgs>(),
        check_fn: None,
        icon: "terminal",
        ..ToolEntry::lifecycle_defaults().deferred()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["write_stdin"],
    async_ctx: dispatch,
    args: WriteStdinArgs,
}

/// 向共享 PTY 会话写入 stdin 并返回 Agent 自己的增量输出。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &WriteStdinArgs) -> anyhow::Result<String> {
    let manager = crate::terminal_session::shared_terminal_sessions();
    let id = match args.session_id {
        Some(id) => id,
        None => {
            manager
                .active_for_scope(ctx.project_or_workspace())
                .ok_or_else(|| anyhow::anyhow!("no active shared terminal for this project"))?
                .id
        }
    };
    let root = ctx.project_or_workspace();
    let policy = crate::context::build_command_sandbox_policy_with_roots(
        &ctx.memory_dir,
        root,
        &ctx.workspace_roots,
        ctx.permission_profile.as_deref(),
        false,
        None,
    )?;
    manager.ensure_access(id, root, &policy)?;
    let has_input = args.chars.as_ref().is_some_and(|chars| !chars.is_empty());
    let wait_ms = args
        .yield_time_ms
        .unwrap_or(if has_input { 250 } else { 5_000 })
        .clamp(0, 30_000) as u64;
    let max_bytes = args.max_output_tokens.saturating_mul(4).clamp(1, 64 * 1024);
    let output = manager
        .interact_for_agent(
            id,
            args.chars.as_deref().map(str::as_bytes),
            false,
            max_bytes,
            wait_ms,
        )
        .await?;
    Ok(format!(
        "Terminal session {} (running={}, next_cursor={})\n{}",
        id,
        output.running,
        output.next_cursor,
        String::from_utf8_lossy(&output.data)
    ))
}
