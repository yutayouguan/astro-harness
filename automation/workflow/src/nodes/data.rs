use anyhow::Result;
use async_trait::async_trait;

use crate::engine::executor::{NodeExecutor, NodeResult};
use crate::engine::variables::VariableContext;
use crate::model::WorkflowNode;

// ── SetFields ────────────────────────────────────────────────────────

pub struct SetFieldsExec;

#[async_trait]
impl NodeExecutor for SetFieldsExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let assignments = node.config.get("assignments").cloned().unwrap_or_default();
        let mut out = serde_json::Map::new();
        if let Some(arr) = assignments.as_array() {
            for item in arr {
                let field = item.get("field").and_then(|v| v.as_str()).unwrap_or_default();
                let value = item.get("value").cloned().unwrap_or(serde_json::Value::Null);
                let resolved = ctx.interpolate_value(&value);
                out.insert(field.to_string(), resolved);
            }
        }
        Ok(NodeResult::Success(serde_json::Value::Object(out)))
    }
}

// ── FormatText ───────────────────────────────────────────────────────

pub struct FormatTextExec;

#[async_trait]
impl NodeExecutor for FormatTextExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let template = node.config.get("template").and_then(|v| v.as_str()).unwrap_or("");
        let text = ctx.interpolate(template);
        Ok(NodeResult::Success(serde_json::json!({ "text": text })))
    }
}

// ── JSON ─────────────────────────────────────────────────────────────

pub struct JsonExec;

#[async_trait]
impl NodeExecutor for JsonExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let mode = node.config.get("mode").and_then(|v| v.as_str()).unwrap_or("parse");
        let expr = node.config.get("expression").and_then(|v| v.as_str()).unwrap_or("");
        let expr_resolved = ctx.interpolate(expr);

        match mode {
            "parse" => {
                let parsed: serde_json::Value = serde_json::from_str(&expr_resolved)
                    .map_err(|e| anyhow::anyhow!("JSON 解析失败: {e}"))?;
                Ok(NodeResult::Success(parsed))
            }
            "stringify" => {
                let input = ctx.interpolate(expr);
                Ok(NodeResult::Success(serde_json::json!({ "text": input })))
            }
            _ => {
                // transform: 简单透传（完整 JQ 引擎超出范围）
                let parsed: serde_json::Value = serde_json::from_str(&expr_resolved)
                    .unwrap_or(serde_json::Value::String(expr_resolved));
                Ok(NodeResult::Success(parsed))
            }
        }
    }
}

// ── Code ─────────────────────────────────────────────────────────────

pub struct CodeExec;

#[async_trait]
impl NodeExecutor for CodeExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let lang = node.config.get("language").and_then(|v| v.as_str()).unwrap_or("javascript");
        let _source = node.config.get("source").and_then(|v| v.as_str()).unwrap_or("");
        anyhow::bail!("代码执行引擎正在开发中 — {} 运行时待集成 (boa/RustPython)", lang)
    }
}

// ── Sort ─────────────────────────────────────────────────────────────

pub struct SortExec;

#[async_trait]
impl NodeExecutor for SortExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let field = node.config.get("field").and_then(|v| v.as_str()).unwrap_or("");
        let order = node.config.get("order").and_then(|v| v.as_str()).unwrap_or("asc");
        let input_ref = node.config.get("input").and_then(|v| v.as_str()).unwrap_or("");
        let field_resolved = ctx.interpolate(field);

        let data = if !input_ref.is_empty() { ctx.resolve(&ctx.interpolate(input_ref)) } else { None };
        match data {
            Some(serde_json::Value::Array(mut arr)) => {
                arr.sort_by(|a, b| {
                    let va = a.get(&field_resolved).and_then(|v| v.as_str()).unwrap_or("");
                    let vb = b.get(&field_resolved).and_then(|v| v.as_str()).unwrap_or("");
                    let cmp = match (va.parse::<f64>(), vb.parse::<f64>()) {
                        (Ok(fa), Ok(fb)) => fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal),
                        _ => va.cmp(vb),
                    };
                    if order == "desc" { cmp.reverse() } else { cmp }
                });
                Ok(NodeResult::Success(serde_json::Value::Array(arr)))
            }
            _ => Ok(NodeResult::Success(serde_json::json!({
                "sorted": true, "field": field_resolved, "order": order,
                "note": "未找到输入数组，请配置 input 引用上游数组字段"
            }))),
        }
    }
}

// ── Slice ────────────────────────────────────────────────────────────

pub struct SliceExec;

#[async_trait]
impl NodeExecutor for SliceExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let start = node.config.get("start").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
        let end = node.config.get("end").and_then(|v| v.as_i64());
        let input_ref = node.config.get("input").and_then(|v| v.as_str()).unwrap_or("");

        let data = if !input_ref.is_empty() { ctx.resolve(&ctx.interpolate(input_ref)) } else { None };
        match data {
            Some(serde_json::Value::Array(arr)) => {
                let end_idx = end.map(|e| (e.max(0) as usize).min(arr.len())).unwrap_or(arr.len());
                let sliced: Vec<_> = arr.into_iter().skip(start).take(end_idx.saturating_sub(start)).collect();
                Ok(NodeResult::Success(serde_json::Value::Array(sliced)))
            }
            _ => Ok(NodeResult::Success(serde_json::json!({
                "sliced": true, "start": start, "end": end,
                "note": "未找到输入数组，请配置 input 引用上游数组字段"
            }))),
        }
    }
}

// ── Aggregate ────────────────────────────────────────────────────────

pub struct AggregateExec;

#[async_trait]
impl NodeExecutor for AggregateExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let op = node.config.get("operation").and_then(|v| v.as_str()).unwrap_or("count");
        let field = node.config.get("field").and_then(|v| v.as_str()).unwrap_or("");
        let input_ref = node.config.get("input").and_then(|v| v.as_str()).unwrap_or("");

        let data = if !input_ref.is_empty() { ctx.resolve(&ctx.interpolate(input_ref)) } else { None };
        match data {
            Some(serde_json::Value::Array(arr)) => {
                let result = match op {
                    "count" => serde_json::json!(arr.len()),
                    "sum" | "avg" | "min" | "max" => {
                        let nums: Vec<f64> = arr.iter()
                            .filter_map(|v| if field.is_empty() { v.as_f64() } else { v.get(field).and_then(|f| f.as_f64()) })
                            .collect();
                        if nums.is_empty() { serde_json::json!(null) }
                        else { match op {
                            "sum" => serde_json::json!(nums.iter().sum::<f64>()),
                            "avg" => serde_json::json!(nums.iter().sum::<f64>() / nums.len() as f64),
                            "min" => serde_json::json!(nums.iter().cloned().fold(f64::INFINITY, f64::min)),
                            "max" => serde_json::json!(nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max)),
                            _ => serde_json::json!(null),
                        }}
                    }
                    _ => serde_json::json!({"error": format!("未知聚合操作: {}", op)}),
                };
                Ok(NodeResult::Success(serde_json::json!({ "result": result, "operation": op })))
            }
            _ => Ok(NodeResult::Success(serde_json::json!({
                "aggregated": true, "operation": op, "field": field,
                "note": "未找到输入数组，请配置 input 引用上游数组字段"
            }))),
        }
    }
}
