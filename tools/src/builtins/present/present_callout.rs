//! Callout 卡工具：向聊天展示带 Callout 提示的 A2UI 卡片（不 interrupt）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::builtins::present::present_shared::dispatch_present;
use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PresentCalloutArgs {
    pub title: String,
    pub body: String,
    /// Callout 语义：`"warn"` 或 `"info"`（默认 `"info"`）。
    #[serde(default)]
    pub variant: Option<String>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "present_callout".to_string(),
        toolset: "present_ui".to_string(),
        description: "Present a callout card in chat with a title, body, and optional variant (\"warn\" or \"info\"). Use for notices, warnings, or tips.".to_string(),
        schema: schema_for_args::<PresentCalloutArgs>(),
        check_fn: None,
        icon: "alert-circle",
            ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["present_callout"],
    sync_ctx: dispatch,
}

pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: PresentCalloutArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("present_callout 参数无效: {e}"))?;

    let title = parsed.title.trim().to_string();
    let body = parsed.body.trim().to_string();
    if title.is_empty() {
        anyhow::bail!("present_callout 需要 title");
    }
    if body.is_empty() {
        anyhow::bail!("present_callout 需要 body");
    }

    let variant = parsed
        .variant
        .as_deref()
        .filter(|v| matches!(*v, "warn" | "info"))
        .unwrap_or("info")
        .to_string();

    dispatch_present("callout", &title, |sid| {
        a2ui::templates::build_callout_surface(sid, &title, &body, &variant)
    })
}
