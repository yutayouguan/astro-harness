//! 统一呈现工具 `present`：只读 A2UI 卡片（不 interrupt）。
//!
//! `kind=ui|callout|metrics|result`（省略时按字段推断，优先 `ui`）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::builtin::present::present_shared::dispatch_present;
use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// Card kind for `present`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PresentKind {
    /// Info card: title/body/image_url or raw operations[].
    Ui,
    /// Callout with variant warn|info.
    Callout,
    /// Metrics list.
    Metrics,
    /// Result with status badge.
    Result,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MetricItem {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub hint: Option<String>,
}

/// Arguments for the unified `present` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PresentArgs {
    /// `ui` | `callout` | `metrics` | `result`. Omit to infer from fields.
    #[serde(default)]
    pub kind: Option<PresentKind>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    /// Full A2UI operations (ui kind).
    #[serde(default)]
    pub operations: Option<Value>,
    /// Callout variant: warn|info.
    #[serde(default)]
    pub variant: Option<String>,
    /// Result status: success|warn|danger|info.
    #[serde(default)]
    pub status: Option<String>,
    /// Metrics rows (metrics kind).
    #[serde(default)]
    pub metrics: Vec<MetricItem>,
}

/// 向注册表登记 `present`。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "present".to_string(),
        toolset: "present".to_string(),
        description: "Present a read-only UI card in chat (no interrupt). \
kind=ui|callout|metrics|result (omit to infer). \
ui: title/body/image_url or operations[]; callout: title+body+variant; \
metrics: title+metrics[]; result: title+body+status. \
Do not use for workspace media — put ![image](path) etc. in the reply body instead."
            .to_string(),
        schema: schema_for_args::<PresentArgs>(),
        check_fn: None,
        icon: "layout-panel-top",
        ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["present"],
    sync_ctx: dispatch,
}

fn resolve_kind(parsed: &PresentArgs) -> anyhow::Result<PresentKind> {
    if let Some(k) = parsed.kind {
        return Ok(k);
    }
    if parsed.operations.is_some() {
        return Ok(PresentKind::Ui);
    }
    if !parsed.metrics.is_empty() {
        return Ok(PresentKind::Metrics);
    }
    if parsed
        .status
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Ok(PresentKind::Result);
    }
    if parsed
        .variant
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Ok(PresentKind::Callout);
    }
    // Default shortcut card
    Ok(PresentKind::Ui)
}

/// 按 kind 构建 `astro_ui` 载荷。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &Value) -> anyhow::Result<String> {
    let parsed: PresentArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("present 参数无效: {e}"))?;
    match resolve_kind(&parsed)? {
        PresentKind::Ui => dispatch_ui(&parsed),
        PresentKind::Callout => dispatch_callout(&parsed),
        PresentKind::Metrics => dispatch_metrics(&parsed),
        PresentKind::Result => dispatch_result(&parsed),
    }
}

fn dispatch_ui(parsed: &PresentArgs) -> anyhow::Result<String> {
    let operations = if let Some(ops) = &parsed.operations {
        let arr = ops
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("operations 须为数组"))?
            .clone();
        a2ui::validate_operations(&arr)
            .map_err(|e| anyhow::anyhow!("present ui A2UI 无效: {e}"))?;
        arr
    } else {
        let title = parsed
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("present ui 需要 title 或 operations"))?;
        let body = parsed
            .body
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("present ui 需要 body 或 operations"))?;
        let surface_id = format!("info-{}", Uuid::new_v4());
        let ops = a2ui::templates::build_info_surface(
            &surface_id,
            title,
            body,
            parsed.image_url.as_deref(),
        );
        a2ui::validate_operations(&ops)
            .map_err(|e| anyhow::anyhow!("present ui A2UI 无效: {e}"))?;
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

fn dispatch_callout(parsed: &PresentArgs) -> anyhow::Result<String> {
    let title = parsed
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("present callout 需要 title"))?
        .to_string();
    let body = parsed
        .body
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("present callout 需要 body"))?
        .to_string();
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

fn dispatch_metrics(parsed: &PresentArgs) -> anyhow::Result<String> {
    let title = parsed
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("present metrics 需要 title"))?
        .to_string();
    if parsed.metrics.is_empty() {
        anyhow::bail!("present metrics 需要至少一条 metric");
    }
    let rows: Vec<(String, String, Option<String>)> = parsed
        .metrics
        .iter()
        .map(|m| (m.label.clone(), m.value.clone(), m.hint.clone()))
        .collect();
    dispatch_present("metrics", &title, |sid| {
        a2ui::templates::build_metrics_surface(sid, &title, &rows)
    })
}

fn dispatch_result(parsed: &PresentArgs) -> anyhow::Result<String> {
    let title = parsed
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("present result 需要 title"))?
        .to_string();
    let body = parsed
        .body
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("present result 需要 body"))?
        .to_string();
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
