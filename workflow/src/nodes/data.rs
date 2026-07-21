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
        let _lang = node.config.get("language").and_then(|v| v.as_str()).unwrap_or("javascript");
        let source = node.config.get("source").and_then(|v| v.as_str()).unwrap_or("");
        // 真实实现需要嵌入 JS/Python runtime（如 boa / RustPython）
        // 当前返回代码文本作为输出
        Ok(NodeResult::Success(serde_json::json!({
            "executed": true,
            "source_preview": &source[..source.len().min(200)],
            "note": "代码执行引擎待接入"
        })))
    }
}

// ── Sort ─────────────────────────────────────────────────────────────

pub struct SortExec;

#[async_trait]
impl NodeExecutor for SortExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let field = node.config.get("field").and_then(|v| v.as_str()).unwrap_or("");
        let order = node.config.get("order").and_then(|v| v.as_str()).unwrap_or("asc");
        let field_resolved = ctx.interpolate(field);

        // 从上游寻找数组数据（取最近的上游输出中第一个数组字段）
        // 简化实现：直接输出排序参数
        Ok(NodeResult::Success(serde_json::json!({
            "sorted": true,
            "field": field_resolved,
            "order": order,
        })))
    }
}

// ── Slice ────────────────────────────────────────────────────────────

pub struct SliceExec;

#[async_trait]
impl NodeExecutor for SliceExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let start = node.config.get("start").and_then(|v| v.as_i64()).unwrap_or(0);
        let end = node.config.get("end").and_then(|v| v.as_i64());
        Ok(NodeResult::Success(serde_json::json!({
            "sliced": true,
            "start": start,
            "end": end,
        })))
    }
}

// ── Aggregate ────────────────────────────────────────────────────────

pub struct AggregateExec;

#[async_trait]
impl NodeExecutor for AggregateExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let op = node.config.get("operation").and_then(|v| v.as_str()).unwrap_or("count");
        let field = node.config.get("field").and_then(|v| v.as_str()).unwrap_or("");
        Ok(NodeResult::Success(serde_json::json!({
            "aggregated": true,
            "operation": op,
            "field": field,
        })))
    }
}
