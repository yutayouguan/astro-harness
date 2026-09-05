use anyhow::Result;
use async_trait::async_trait;

use crate::engine::executor::{NodeExecutor, NodeResult};
use crate::engine::variables::VariableContext;
use crate::model::WorkflowNode;

// ── Conditional ──────────────────────────────────────────────────────

pub struct ConditionalExec;

#[async_trait]
impl NodeExecutor for ConditionalExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let conditions = node.config.get("conditions").and_then(|v| v.as_array());
        let mut active_handles = Vec::new();

        if let Some(branches) = conditions {
            let mut matched = false;
            for branch in branches {
                let id = branch
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let expr = branch
                    .get("expression")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if expr.is_empty() {
                    // else 分支：仅当前面没有匹配时激活
                    if !matched {
                        active_handles.push(id.to_string());
                    }
                } else if !matched && ctx.evaluate_condition(expr)? {
                    active_handles.push(id.to_string());
                    matched = true;
                }
            }
        }

        Ok(NodeResult::Branch(active_handles))
    }
}

// ── MultiBranch ──────────────────────────────────────────────────────

pub struct MultiBranchExec;

#[async_trait]
impl NodeExecutor for MultiBranchExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let branches = node.config.get("branches").and_then(|v| v.as_array());
        let mut active_handles = Vec::new();

        if let Some(branches) = branches {
            // 两轮评估：先评估条件分支，再决定默认分支
            let mut default_ids = Vec::new();
            for branch in branches {
                let id = branch
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let cond = branch.get("condition").and_then(|v| v.as_str());
                match cond {
                    Some(expr) if !expr.is_empty() => {
                        if ctx.evaluate_condition(expr)? {
                            active_handles.push(id.to_string());
                        }
                    }
                    _ => {
                        default_ids.push(id.to_string());
                    }
                }
            }
            // 无条件分支命中时激活所有默认分支
            if active_handles.is_empty() {
                active_handles = default_ids;
            }
        }

        Ok(NodeResult::Branch(active_handles))
    }
}

// ── Filter ───────────────────────────────────────────────────────────

pub struct FilterExec;

#[async_trait]
impl NodeExecutor for FilterExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let condition = node
            .config
            .get("condition")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if condition.is_empty() || ctx.evaluate_condition(condition)? {
            Ok(NodeResult::Success(serde_json::json!({ "passed": true })))
        } else {
            Ok(NodeResult::Filtered)
        }
    }
}

// ── Merge ────────────────────────────────────────────────────────────

pub struct MergeExec;

#[async_trait]
impl NodeExecutor for MergeExec {
    async fn execute(&self, _node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        // Merge 节点：拓扑序天然保证所有上游已完成（wait_all 语义由引擎层处理）
        Ok(NodeResult::Success(serde_json::json!({ "merged": true })))
    }
}

// ── Loop ─────────────────────────────────────────────────────────────

pub struct LoopExec;

#[async_trait]
impl NodeExecutor for LoopExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let max_iter = node
            .config
            .get("max_iterations")
            .and_then(|v| v.as_u64())
            .unwrap_or(10);
        let break_cond = node
            .config
            .get("break_condition")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // Loop 节点的真正循环由引擎层驱动；这里仅返回配置信息
        Ok(NodeResult::Success(serde_json::json!({
            "loop": true,
            "max_iterations": max_iter,
            "break_condition": break_cond,
        })))
    }
}

// ── HumanApproval ────────────────────────────────────────────────────

pub struct HumanApprovalExec;

#[async_trait]
impl NodeExecutor for HumanApprovalExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let prompt = node
            .config
            .get("prompt_template")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let resolved_prompt = ctx.interpolate(prompt);
        let auto_approve = node
            .config
            .get("auto_approve")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if auto_approve || ctx.human_approval_granted() {
            tracing::info!(prompt = %resolved_prompt, "人工审批节点：自动放行");
            return Ok(NodeResult::Approved);
        }
        tracing::info!(prompt = %resolved_prompt, "人工审批节点：等待用户审批");
        Ok(NodeResult::PendingApproval {
            prompt: resolved_prompt,
        })
    }
}
