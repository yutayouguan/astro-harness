//! `request_mode_switch`：请求切换 Agent ↔ Plan（流结束后由前端授权条确认）。

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::context::ToolContext;
use crate::schema::schema_for_args;

#[derive(Debug, Deserialize, JsonSchema)]
struct ModeSwitchArgs {
    /// 目标模式：`plan` 或 `agent`（本工具不处理 ask/multitask）。
    to: String,
    /// 为何需要切换（给用户看）。
    reason: String,
    /// Plan→Agent 时附带的计划摘要，切换批准后注入下一轮。
    #[serde(default)]
    summary: Option<String>,
}

pub fn register(registry: &mut crate::registry::ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "request_mode_switch".to_string(),
        toolset: "request_mode_switch".to_string(),
        description: "Request switching the chat interaction mode between Agent and Plan. \
Use when a complex task needs a written plan first (Agent→Plan), or when a plan is ready to execute (Plan→Agent). \
The user confirms via a countdown authorize bar; do not assume the switch happened until they approve. \
Provide a clear `reason`; when going to Agent, put the confirmed plan into `summary`."
            .to_string(),
        schema: schema_for_args::<ModeSwitchArgs>(),
        check_fn: None,
        icon: "arrow-left-right",
        stop_after_tool_call: true,
        ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["request_mode_switch"],
    sync_ctx: dispatch,
}

pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: ModeSwitchArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("request_mode_switch 参数无效: {e}"))?;
    let to = parsed.to.trim().to_ascii_lowercase();
    if to != "plan" && to != "agent" {
        anyhow::bail!("request_mode_switch.to must be \"plan\" or \"agent\"");
    }
    let reason = parsed.reason.trim();
    if reason.is_empty() {
        anyhow::bail!("request_mode_switch.reason is required");
    }
    let summary = parsed
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let payload = json!({
        "astro_mode_switch": true,
        "to": to,
        "reason": reason,
        "summary": summary,
    });
    Ok(payload.to_string())
}
