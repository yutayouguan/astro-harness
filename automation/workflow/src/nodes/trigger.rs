use anyhow::Result;
use async_trait::async_trait;

use crate::engine::executor::{NodeExecutor, NodeResult};
use crate::engine::variables::VariableContext;
use crate::model::WorkflowNode;

pub struct ManualTriggerExec;

#[async_trait]
impl NodeExecutor for ManualTriggerExec {
    async fn execute(&self, _node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let globals: serde_json::Value = serde_json::json!({
            "trigger_type": "manual",
            "triggered_at": chrono::Local::now().to_rfc3339(),
        });
        Ok(NodeResult::Success(globals))
    }
}

pub struct ScheduledTriggerExec;

#[async_trait]
impl NodeExecutor for ScheduledTriggerExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let schedule = node.config.get("schedule").and_then(|v| v.as_str()).unwrap_or("");
        Ok(NodeResult::Success(serde_json::json!({
            "trigger_type": "scheduled",
            "schedule": schedule,
            "triggered_at": chrono::Local::now().to_rfc3339(),
        })))
    }
}

pub struct WebhookTriggerExec;

#[async_trait]
impl NodeExecutor for WebhookTriggerExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let path = node.config.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let method = node.config.get("method").and_then(|v| v.as_str()).unwrap_or("POST");
        Ok(NodeResult::Success(serde_json::json!({
            "trigger_type": "webhook",
            "path": path,
            "method": method,
            "triggered_at": chrono::Local::now().to_rfc3339(),
        })))
    }
}

pub struct EmailTriggerExec;

#[async_trait]
impl NodeExecutor for EmailTriggerExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let host = node.config.get("host").and_then(|v| v.as_str()).unwrap_or("");
        let filter = node.config.get("filter").and_then(|v| v.as_str()).unwrap_or("");
        Ok(NodeResult::Success(serde_json::json!({
            "trigger_type": "email",
            "host": host,
            "filter": filter,
            "triggered_at": chrono::Local::now().to_rfc3339(),
            "note": "邮件触发待接入 IMAP/POP3 轮询"
        })))
    }
}

pub struct FileWatchTriggerExec;

#[async_trait]
impl NodeExecutor for FileWatchTriggerExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let path = node.config.get("watch_path").and_then(|v| v.as_str()).unwrap_or("");
        let event = node.config.get("event_type").and_then(|v| v.as_str()).unwrap_or("any");
        Ok(NodeResult::Success(serde_json::json!({
            "trigger_type": "file_watch",
            "watch_path": path,
            "event_type": event,
            "triggered_at": chrono::Local::now().to_rfc3339(),
            "note": "文件监控待接入 notify crate"
        })))
    }
}
