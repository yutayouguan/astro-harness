//! 新上下文窗口请求：触发上下文压缩，在下一轮开始前 compact 当前会话。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// Arguments for the `new_context_window` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct NewContextWindowArgs {
    /// Optional reason for requesting context compaction.
    #[serde(default)]
    pub reason: Option<String>,
}

/// 向注册表注册 `new_context_window` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "new_context_window".to_string(),
        toolset: "system".to_string(),
        description: "Request a new context window by compacting the current conversation context before the next turn."
            .to_string(),
        schema: schema_for_args::<NewContextWindowArgs>(),
        check_fn: None,
        icon: "refresh-cw",
        ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["new_context_window"],
    async_ctx: dispatch,
    args: NewContextWindowArgs,
}

/// 请求上下文压缩并返回确认信息。
pub async fn dispatch(
    _ctx: &ToolContext<'_>,
    args: &NewContextWindowArgs,
) -> anyhow::Result<String> {
    let reason = args.reason.as_deref().unwrap_or("").trim();
    Ok(format!(
        "Context compaction requested. {}The conversation context will be compacted before the next turn.",
        if reason.is_empty() {
            String::new()
        } else {
            format!("Reason: {reason}. ")
        }
    ))
}
