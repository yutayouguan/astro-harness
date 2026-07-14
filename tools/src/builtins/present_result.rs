//! 结果卡工具：向聊天展示带状态 Badge 的操作结果 A2UI 卡片（不 interrupt）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

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
    });
}

pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: PresentResultArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("present_result 参数无效: {e}"))?;

    let title = parsed.title.trim();
    let body = parsed.body.trim();
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
        .unwrap_or("success");

    let surface_id = format!("result-{}", Uuid::new_v4());
    let ops = a2ui::templates::build_result_surface(&surface_id, title, body, status);
    a2ui::validate_operations(&ops)
        .map_err(|e| anyhow::anyhow!("present_result A2UI 无效: {e}"))?;

    Ok(json!({
        "astro_ui": true,
        "summary": title,
        "operations": ops,
    })
    .to_string())
}
