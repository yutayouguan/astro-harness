//! 向运行中的 exec_command 会话写入 stdin 并返回最近输出。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

fn default_yield_time_ms() -> i64 {
    250
}

fn default_max_output_tokens() -> usize {
    10000
}

/// Arguments for the `write_stdin` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WriteStdinArgs {
    /// ID of the running exec_command session.
    pub session_id: u64,
    /// Bytes to write to stdin. Empty or omitted = poll without writing.
    #[serde(default)]
    pub chars: Option<String>,
    /// Wait time in ms before yielding output.
    /// Non-empty writes default to 250 ms; empty polls default to 5000 ms.
    #[serde(default = "default_yield_time_ms")]
    pub yield_time_ms: i64,
    /// Output token budget. Defaults to 10000.
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: usize,
}

/// 向注册表注册 `write_stdin` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "write_stdin".to_string(),
        toolset: "exec_command".to_string(),
        description:
            "Writes characters to an existing exec_command session and returns recent output. Use empty chars to poll."
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

/// 向 exec_command 会话写入 stdin 并返回输出（stub 实现）。
pub async fn dispatch(
    _ctx: &ToolContext<'_>,
    args: &WriteStdinArgs,
) -> anyhow::Result<String> {
    let action = if args.chars.as_ref().is_some_and(|c| !c.is_empty()) {
        "write"
    } else {
        "poll"
    };
    Ok(format!(
        "write_stdin: session management pending. Session ID: {}, action: {}, yield_time_ms: {}, max_output_tokens: {}",
        args.session_id, action, args.yield_time_ms, args.max_output_tokens
    ))
}
