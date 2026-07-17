//! 结果卡工具：向聊天展示带状态 Badge 的操作结果 A2UI 卡片（不 interrupt）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;
use crate::builtins::present::present_shared::dispatch_present;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PresentResultArgs {
    pub title: String,
    pub body: String,
    /// 状态语义：`"success"` / `"warn"` / `"danger"` / `"info"`（默认 `"success"`）。
    #[serde(default)]
    pub status: Option<String>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "present_result".to_string(),
        toolset: "present_ui".to_string(),
        description: "Present an operation result card in chat with a title, body, and status badge. status: \"success\" | \"warn\" | \"danger\" | \"info\" (default \"success\").".to_string(),
        schema: schema_for_args::<PresentResultArgs>(),
        check_fn: None,
        icon: "check-circle",
            ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["present_result"],
    sync_ctx: dispatch,
}

pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: PresentResultArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("present_result 参数无效: {e}"))?;

    let title = parsed.title.trim().to_string();
    let body = parsed.body.trim().to_string();
    if title.is_empty() {
        anyhow::bail!("present_result 需要 title");
    }
    if body.is_empty() {
        anyhow::bail!("present_result 需要 body");
    }

    let status = parsed
        .status
        .as_deref()
        .filter(|s| matches!(*s, "success" | "warn" | "danger" | "info"))
        .unwrap_or("success")
        .to_string();

    dispatch_present("result", &title, |sid| {
        a2ui::templates::build_result_surface(sid, &title, &body, &status)
    })
}
