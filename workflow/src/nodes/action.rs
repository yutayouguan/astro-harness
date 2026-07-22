use anyhow::{bail, Result};
use async_trait::async_trait;

use crate::engine::executor::{NodeExecutor, NodeResult};
use crate::engine::variables::VariableContext;
use crate::model::WorkflowNode;

// ── HttpRequest ──────────────────────────────────────────────────────

pub struct HttpRequestExec;

#[async_trait]
impl NodeExecutor for HttpRequestExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let method = node.config.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
        let url_tpl = node.config.get("url_template").and_then(|v| v.as_str()).unwrap_or("");
        let url = ctx.interpolate(url_tpl);
        let timeout = node.config.get("timeout_seconds").and_then(|v| v.as_u64()).unwrap_or(30);

        let body_tpl = node.config.get("body_template").and_then(|v| v.as_str()).unwrap_or("");
        let body = if body_tpl.is_empty() {
            None
        } else {
            Some(ctx.interpolate(body_tpl))
        };

        let headers_val = node.config.get("headers").cloned().unwrap_or_default();
        let mut header_map = Vec::new();
        if let Some(arr) = headers_val.as_array() {
            for pair in arr {
                if let Some(inner) = pair.as_array() {
                    if inner.len() >= 2 {
                        let k = ctx.interpolate(inner[0].as_str().unwrap_or_default());
                        let v = ctx.interpolate(inner[1].as_str().unwrap_or_default());
                        header_map.push((k, v));
                    }
                }
            }
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout))
            .build()?;

        let mut req = match method.to_uppercase().as_str() {
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "PATCH" => client.patch(&url),
            "DELETE" => client.delete(&url),
            _ => client.get(&url),
        };

        for (k, v) in &header_map {
            req = req.header(k.as_str(), v.as_str());
        }

        if let Some(body) = body {
            req = req.body(body);
        }

        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let resp_text = resp.text().await.unwrap_or_default();

        let resp_body: serde_json::Value = serde_json::from_str(&resp_text)
            .unwrap_or(serde_json::Value::String(resp_text));

        Ok(NodeResult::Success(serde_json::json!({
            "status": status,
            "body": resp_body,
        })))
    }
}

// ── RunLoop (子工作流) ───────────────────────────────────────────────

pub struct RunLoopExec;

#[async_trait]
impl NodeExecutor for RunLoopExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        // 子工作流执行由引擎层 execute_sub_workflow 直接处理（绕过此 executor）
        // 此处仅作 fallback 标记
        let workflow_id = node.config.get("workflow_id").and_then(|v| v.as_str()).unwrap_or("");
        Ok(NodeResult::Success(serde_json::json!({ "sub_workflow": workflow_id })))
    }
}

// ── DelayWait ────────────────────────────────────────────────────────

pub struct DelayWaitExec;

#[async_trait]
impl NodeExecutor for DelayWaitExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let seconds = node.config.get("seconds").and_then(|v| v.as_u64()).unwrap_or(1);
        tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
        Ok(NodeResult::Success(serde_json::json!({
            "waited_seconds": seconds,
        })))
    }
}

// ── Output ───────────────────────────────────────────────────────────

pub struct OutputExec;

#[async_trait]
impl NodeExecutor for OutputExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        // 收集 fields 中定义的输出字段
        let fields = node.config.get("fields").and_then(|v| v.as_array());
        let mut out = serde_json::Map::new();
        if let Some(fields) = fields {
            for f in fields {
                let name = f.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                if let Some(val) = ctx.resolve(name) {
                    out.insert(name.to_string(), val);
                }
            }
        }
        if out.is_empty() {
            // 无显式字段定义时，返回 output 标记
            out.insert("completed".into(), serde_json::Value::Bool(true));
        }
        Ok(NodeResult::Success(serde_json::Value::Object(out)))
    }
}

// ── AudioProcessing ──────────────────────────────────────────────────

pub struct AudioProcessingExec;

#[async_trait]
impl NodeExecutor for AudioProcessingExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let op = node.config.get("operation").and_then(|v| v.as_str()).unwrap_or("convert");
        bail!("音频处理功能正在开发中 — 待接入 ffmpeg/rodio 引擎。操作: {}", op)
    }
}
