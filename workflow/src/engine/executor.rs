use anyhow::Result;
use async_trait::async_trait;

use crate::model::WorkflowNode;
use super::variables::VariableContext;

/// 节点执行结果
#[derive(Debug, Clone)]
pub enum NodeResult {
    /// 正常完成，输出一个 JSON 值
    Success(serde_json::Value),
    /// 条件/分支节点：激活哪些输出端口（source_handle id）
    Branch(Vec<String>),
    /// 过滤节点：条件不满足，阻断下游
    Filtered,
    /// 人工审批：暂停执行（当前实现直接放行）
    Approved,
}

/// 节点执行器 trait —— 每种 NodeType 实现一个
#[async_trait]
pub trait NodeExecutor: Send + Sync {
    async fn execute(
        &self,
        node: &WorkflowNode,
        ctx: &VariableContext,
    ) -> Result<NodeResult>;
}
