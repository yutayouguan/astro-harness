//! 确认/授权工具：敏感操作前向用户请求批准。
//!
//! 返回带 `astro_hitl` 标记的 JSON，由 Agent 流层转为 A2UI activity + interrupt。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// `confirm` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ConfirmArgs {
    /// 确认卡标题。
    pub title: String,
    /// 确认卡正文说明。
    pub body: String,
}

/// 向注册表登记 `confirm` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "confirm".to_string(),
        toolset: "confirm".to_string(),
        description: "Ask the user to approve or deny a sensitive action before proceeding."
            .to_string(),
        schema: schema_for_args::<ConfirmArgs>(),
        check_fn: None,
        icon: "shield-check",
    });
}

/// 构建 HITL confirm 载荷（A2UI operations + response schema）。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: ConfirmArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("confirm 参数无效: {e}"))?;
    let title = parsed.title.trim();
    let body = parsed.body.trim();
    if title.is_empty() {
        anyhow::bail!("confirm 需要 title");
    }
    if body.is_empty() {
        anyhow::bail!("confirm 需要 body");
    }

    let surface_id = format!("confirm-{}", Uuid::new_v4());
    let operations = a2ui::templates::build_confirm_surface(&surface_id, title, body);
    a2ui::validate_operations(&operations)
        .map_err(|e| anyhow::anyhow!("confirm A2UI 无效: {e}"))?;

    let payload = json!({
        "astro_hitl": true,
        "reason": "confirmation",
        "message": title,
        "operations": operations,
        "response_schema": {
            "type": "object",
            "properties": { "approved": { "type": "boolean" } },
            "required": ["approved"]
        }
    });
    Ok(payload.to_string())
}
