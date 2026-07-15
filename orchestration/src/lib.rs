//! 多 Agent 编排：状态库、spawn 请求类型与协作洞察。
//!
//! - [`db`]：`~/.astro/orchestration.db`
//! - [`spawn`]：spawn 请求 / spawner 回调类型（执行在 `agent`）
//! - [`collab_insights`]：编排列表 + usage handoff 图

pub mod collab_insights;
pub mod db;
pub mod spawn;

pub use collab_insights::{
    query_collaboration_insights, CollaborationEdge, CollaborationGraph, CollaborationInsights,
    CollaborationInsightsQuery, CollaborationNode, CollaborationOrchestration, CollaborationStep,
    COLLAB_LIST_LIMIT, COLLAB_OUTPUT_MAX_BYTES,
};
pub use db::{
    orchestration_db_path, NewOrchestration, NewOrchestrationStep, OrchestrationDb,
    OrchestrationRow, OrchestrationStatus, StepRow, StepStatus,
};
pub use spawn::{OrchestrationSpawnRequest, OrchestrationSpawner};
