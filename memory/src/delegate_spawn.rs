//! 由 agent 在启动时注入；tools 的 `delegate` 同步调用并等待结果。

use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

/// 单个委派子任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateTaskSpec {
    pub goal: String,
    pub context: String,
}

/// 同步委派请求（父 Agent 凭据 + 任务列表）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateRunRequest {
    pub parent_agent_id: String,
    pub parent_session_id: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    /// 含 primary 的聊天 fallback 链；空则子 Agent 由四字段合成单目标。
    #[serde(default)]
    pub chat_targets: Vec<common::ChatTarget>,
    pub tasks: Vec<DelegateTaskSpec>,
    /// 并行上限（至少 1）。
    pub max_concurrent: usize,
    /// 发起方当前嵌套深度（顶层 0）。
    pub caller_depth: u32,
    /// 允许发起嵌套的最大 caller depth（默认 1）。
    pub max_spawn_depth: u32,
}

pub type DelegateRunner =
    Arc<dyn Fn(DelegateRunRequest) -> anyhow::Result<String> + Send + Sync + 'static>;

static RUNNER: OnceLock<DelegateRunner> = OnceLock::new();

pub fn set_delegate_runner(runner: DelegateRunner) {
    let _ = RUNNER.set(runner);
}

/// 未注册时返回错误（便于测试发现漏注册）。
pub fn run_delegate_sync(req: DelegateRunRequest) -> anyhow::Result<String> {
    match RUNNER.get() {
        Some(f) => f(req),
        None => anyhow::bail!(
            "delegate runner not registered; cannot execute sub-agent (PLAN-ONLY fallback removed)"
        ),
    }
}
