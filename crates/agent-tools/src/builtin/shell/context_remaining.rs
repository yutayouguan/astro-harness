//! 上下文窗口余量查询：返回当前上下文窗口的总容量、已用量和剩余量。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// Arguments for the `get_context_remaining` tool (no parameters).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct GetContextRemainingArgs {}

/// 向注册表注册 `get_context_remaining` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "get_context_remaining".to_string(),
        toolset: "system".to_string(),
        description: "Returns the remaining context window capacity: total tokens, used tokens, and remaining tokens."
            .to_string(),
        schema: schema_for_args::<GetContextRemainingArgs>(),
        check_fn: None,
        icon: "gauge",
        ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["get_context_remaining"],
    async_ctx: dispatch,
    args: GetContextRemainingArgs,
}

/// 返回上下文窗口容量信息。
pub async fn dispatch(
    ctx: &ToolContext<'_>,
    _args: &GetContextRemainingArgs,
) -> anyhow::Result<String> {
    match (ctx.context_window, ctx.context_tokens_used) {
        (Some(window), Some(used)) => {
            let remaining = window.saturating_sub(used);
            let pct = if window > 0 {
                (used as f64 / window as f64) * 100.0
            } else {
                0.0
            };
            Ok(format!(
                "Context window: {window} tokens total, {used} used, {remaining} remaining ({pct:.0}% used)"
            ))
        }
        (Some(window), None) => Ok(format!(
            "Context window: {window} tokens total, usage unknown"
        )),
        _ => Ok("Context window information is not available.".to_string()),
    }
}
