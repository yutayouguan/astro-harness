//! 指标卡工具：向聊天展示 Metric 列表 A2UI 卡片（不 interrupt）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;
use crate::builtins::present::present_shared::dispatch_present;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MetricItem {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PresentMetricsArgs {
    pub title: String,
    pub metrics: Vec<MetricItem>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "present_metrics".to_string(),
        toolset: "present_ui".to_string(),
        description: "Present a read-only metrics card in chat. Supply a title and an array of metrics, each with label, value, and optional hint.".to_string(),
        schema: schema_for_args::<PresentMetricsArgs>(),
        check_fn: None,
        icon: "chart-bar",
            ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool!(register);

pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: PresentMetricsArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("present_metrics 参数无效: {e}"))?;

    let title = parsed.title.trim().to_string();
    if title.is_empty() {
        anyhow::bail!("present_metrics 需要 title");
    }
    if parsed.metrics.is_empty() {
        anyhow::bail!("present_metrics 需要至少一条 metric");
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
