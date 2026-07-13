//! 信息卡工具：向聊天展示只读 A2UI 卡片（不 interrupt）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// `present_ui` 工具参数：快捷字段或完整 operations。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PresentUiArgs {
    /// 卡片标题（快捷模式）。
    #[serde(default)]
    pub title: Option<String>,
    /// 卡片正文（快捷模式）。
    #[serde(default)]
    pub body: Option<String>,
    /// 可选图片 URL（快捷模式）。
    #[serde(default)]
    pub image_url: Option<String>,
    /// 完整 A2UI operations 数组（优先于快捷字段）。
    #[serde(default)]
    pub operations: Option<Value>,
}

/// 向注册表登记 `present_ui`。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "present_ui".to_string(),
        toolset: "present_ui".to_string(),
        description: "Present a read-only informational UI card in chat (no interrupt). Prefer shortcut fields title/body/image_url, or pass full A2UI v0.9 operations[] with catalogId astro://a2ui/catalog/v2. Allowed components: Text Icon Divider Card Column Row Button TextField ChoicePicker CheckBox Image List Badge Chip Metric Avatar Callout Spacer. Root should be Card. Use variant for semantics; never put hex colors in JSON. Example metric row: Metric{label,value,hint} inside Column inside Card."
            .to_string(),
        schema: schema_for_args::<PresentUiArgs>(),
        check_fn: None,
        icon: "layout-panel-top",
    });
}

/// 校验并返回 `astro_ui` 载荷。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &Value) -> anyhow::Result<String> {
    let parsed: PresentUiArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("present_ui 参数无效: {e}"))?;

    let operations = if let Some(ops) = parsed.operations {
        let arr = ops
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("operations 须为数组"))?
            .clone();
        a2ui::validate_operations(&arr)
            .map_err(|e| anyhow::anyhow!("present_ui A2UI 无效: {e}"))?;
        arr
    } else {
        let title = parsed
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("present_ui 需要 title 或 operations"))?;
        let body = parsed
            .body
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("present_ui 需要 body 或 operations"))?;
        let surface_id = format!("info-{}", Uuid::new_v4());
        let ops = a2ui::templates::build_info_surface(
            &surface_id,
            title,
            body,
            parsed.image_url.as_deref(),
        );
        a2ui::validate_operations(&ops)
            .map_err(|e| anyhow::anyhow!("present_ui A2UI 无效: {e}"))?;
        ops
    };

    let summary = parsed
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("info card")
        .to_string();

    Ok(json!({
        "astro_ui": true,
        "summary": summary,
        "operations": operations,
    })
    .to_string())
}
