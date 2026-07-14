//! Callout 卡工具：向聊天展示带 Callout 提示的 A2UI 卡片（不 interrupt）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

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
    });
}

pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: PresentCalloutArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("present_callout 参数无效: {e}"))?;

    let title = parsed.title.trim();
    let body = parsed.body.trim();
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
        .unwrap_or("info");

    let surface_id = format!("callout-{}", Uuid::new_v4());
    let ops = a2ui::templates::build_callout_surface(&surface_id, title, body, variant);
    a2ui::validate_operations(&ops)
        .map_err(|e| anyhow::anyhow!("present_callout A2UI 无效: {e}"))?;

    Ok(json!({
        "astro_ui": true,
        "summary": title,
        "operations": ops,
    })
    .to_string())
}
