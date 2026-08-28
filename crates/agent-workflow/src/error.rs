use crate::model::NodeType;

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("DAG 中存在环路，无法执行")]
    CycleDetected,

    #[error("节点 {label} ({node_id}) 无可用执行器: {node_type:?}")]
    NoExecutor {
        node_id: String,
        label: String,
        node_type: NodeType,
    },

    #[error("节点 {label} ({node_id}) 执行失败: {source}")]
    NodeExecFailed {
        node_id: String,
        label: String,
        source: anyhow::Error,
    },

    #[error("工作流执行超时（{timeout_secs}秒），可在工作流变量中设置 timeout_seconds 调整")]
    Timeout { timeout_secs: u64 },

    #[error("子工作流递归深度超过限制 ({max_depth})")]
    MaxDepthExceeded { max_depth: u32 },

    #[error("子工作流 {workflow_id} 不存在")]
    SubWorkflowNotFound { workflow_id: String },

    #[error("节点配置缺失: {field}")]
    MissingConfig { field: String },

    #[error(transparent)]
    Store(#[from] StoreError),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("读取 workflows.json 失败: {0}")]
    ReadFailed(#[source] std::io::Error),

    #[error("解析 workflows.json 失败: {0}")]
    ParseFailed(#[source] serde_json::Error),

    #[error("写入工作流文件失败: {0}")]
    WriteFailed(#[source] std::io::Error),

    #[error("工作流校验失败: {0}")]
    ValidationFailed(String),

    #[error(transparent)]
    Db(#[from] agent_db::sqlx::Error),
}
