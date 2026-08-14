//! 编排 spawn 请求类型。

use types::ChatTarget;

pub struct OrchestrationSpawnRequest {
    pub orchestration_id: String,
    pub parent_agent_id: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    /// 含 primary 的聊天 fallback 链；空则由四字段合成。
    pub chat_targets: Vec<ChatTarget>,
    /// 发起方当前嵌套深度（顶层 0）。
    pub caller_depth: u32,
    /// 允许发起嵌套的最大 caller depth（默认 1）。
    pub max_spawn_depth: u32,
    /// 为 true 时允许认领崩溃留下的 `running`（进程重启续跑）。
    pub allow_reclaim: bool,
}
