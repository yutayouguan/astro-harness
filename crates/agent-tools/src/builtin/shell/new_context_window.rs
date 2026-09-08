//! 新上下文窗口请求：触发上下文压缩，在下一轮开始前 compact 当前会话。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `new_context_window` 工具的参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct NewContextWindowArgs {
    /// 请求上下文压缩的可选理由。
    #[serde(default)]
    pub reason: Option<String>,
}

/// 向注册表注册 `new_context_window` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "new_context_window".to_string(),
        toolset: "system".to_string(),
        description: "Queue current-turn context compaction. The runtime processes it after the entire tool batch is persisted, before the next sampling. Queued is not completed; read notes for compaction_status."
            .to_string(),
        schema: schema_for_args::<NewContextWindowArgs>(),
        check_fn: None,
        icon: "refresh-cw",
        ..ToolEntry::lifecycle_defaults().exclusive()
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
    ctx: &ToolContext<'_>,
    args: &NewContextWindowArgs,
) -> anyhow::Result<String> {
    let reason = args.reason.as_deref().unwrap_or("").trim();
    let turn_id = ctx
        .turn_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("context compaction requires an active turn"))?;
    ctx.sessions
        .request_context_compaction(&ctx.session_id, turn_id, reason)
        .await?;
    Ok(format!(
        "Context compaction queued, not yet completed. {}The runtime will process it after this tool batch is persisted, before the next sampling. Check notes action=read for compaction_status; failed requests do not discard history.",
        if reason.is_empty() {
            String::new()
        } else {
            format!("Reason: {reason}. ")
        }
    ))
}
