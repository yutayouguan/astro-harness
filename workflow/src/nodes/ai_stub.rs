use anyhow::Result;
use async_trait::async_trait;

use crate::engine::executor::{NodeExecutor, NodeResult};
use crate::engine::variables::VariableContext;
use crate::model::WorkflowNode;

pub struct AiAgentTaskExec;

#[async_trait]
impl NodeExecutor for AiAgentTaskExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let prompt_tpl = node.config.get("prompt_template").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        let agent_id = node.config.get("agent_id").and_then(|v| v.as_str()).unwrap_or("default");
        Ok(NodeResult::Success(serde_json::json!({
            "agent_id": agent_id,
            "prompt": prompt,
            "note": "AI 执行待接入 AgentLoop"
        })))
    }
}

pub struct ParameterExtractionExec;

#[async_trait]
impl NodeExecutor for ParameterExtractionExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let prompt_tpl = node.config.get("prompt_template").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        Ok(NodeResult::Success(serde_json::json!({
            "prompt": prompt,
            "note": "参数提取待接入 LLM Extractor"
        })))
    }
}

pub struct QuestionClassificationExec;

#[async_trait]
impl NodeExecutor for QuestionClassificationExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let classes = node.config.get("classes").and_then(|v| v.as_array());
        let first_id = classes
            .and_then(|c| c.first())
            .and_then(|c| c.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("default");
        // 桩实现：直接走第一个分类
        Ok(NodeResult::Branch(vec![first_id.to_string()]))
    }
}
