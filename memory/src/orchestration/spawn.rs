//! 由 agent 在启动时注入；tools 在 orchestration_run 成功落库后调用。

use std::sync::{Arc, OnceLock};

pub struct OrchestrationSpawnRequest {
    pub orchestration_id: String,
    pub parent_agent_id: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
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
