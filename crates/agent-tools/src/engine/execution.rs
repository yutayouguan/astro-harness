//! 子 Agent 执行调度统一接口。
//!
//! 替代原来分散的 `DelegateRunner` / `DelegateAsyncSpawner` / `OrchestrationSpawner`
//! 三个 `Arc<dyn Fn>` 类型别名，使编排策略可测试、可替换。

use delegate::DelegateRunRequest;
use orchestration::OrchestrationSpawnRequest;

/// 子 Agent 执行调度统一接口。
///
/// 由 `AgentLoop` 在构造时注入实现（通常为 `DefaultExecutionDispatch`），
/// 经 `ToolContext` 传递给 `subagent` / `pipeline` / `team` 工具。
pub trait ExecutionDispatch: Send + Sync {
    /// 同步执行委派（阻塞等待结果）。
    fn run_sync(&self, request: DelegateRunRequest) -> anyhow::Result<String>;

    /// 异步启动委派（立即返回，结果写入 `AsyncDelegateRegistry`）。
    fn spawn_async(&self, task_id: String, request: DelegateRunRequest);

    /// 启动编排流水线（立即返回，结果写入 `OrchestrationDb`）。
    fn spawn_orchestration(&self, request: OrchestrationSpawnRequest);
}
