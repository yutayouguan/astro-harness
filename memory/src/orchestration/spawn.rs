//! 由 agent 在启动时注入；tools 在 orchestration_run 成功落库后调用。

use std::sync::{Arc, OnceLock};

pub struct OrchestrationSpawnRequest {
    pub orchestration_id: String,
    pub parent_agent_id: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    /// 发起方当前嵌套深度（顶层 0）。
    pub caller_depth: u32,
    /// 允许发起嵌套的最大 caller depth（默认 1）。
    pub max_spawn_depth: u32,
    /// 为 true 时允许认领崩溃留下的 `running`（进程重启续跑）。
    pub allow_reclaim: bool,
}

pub type OrchestrationSpawner =
    Arc<dyn Fn(OrchestrationSpawnRequest) + Send + Sync + 'static>;

static SPAWNER: OnceLock<OrchestrationSpawner> = OnceLock::new();

pub fn set_orchestration_spawner(spawner: OrchestrationSpawner) {
    let _ = SPAWNER.set(spawner);
}

/// 未注册 spawner 时仅打 warn，不 panic（便于单测只测落库）。
pub fn request_orchestration_spawn(req: OrchestrationSpawnRequest) {
    if let Some(f) = SPAWNER.get() {
        f(req);
    } else {
        tracing::warn!(
            id = %req.orchestration_id,
            "orchestration spawner not registered; job stays queued"
        );
    }
}
