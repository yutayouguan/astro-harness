//! `ExecutionDispatch` 默认实现：桥接 `exec::delegate` 与 `exec::orchestration`。

use delegate::{AsyncDelegateRegistry, DelegateRunRequest};
use orchestration::OrchestrationSpawnRequest;
use tools::ExecutionDispatch;

/// 生产环境默认实现：同步阻塞执行 / 异步 tokio::spawn。
pub struct DefaultExecutionDispatch;

impl ExecutionDispatch for DefaultExecutionDispatch {
    fn run_sync(&self, request: DelegateRunRequest) -> anyhow::Result<String> {
        crate::exec::delegate::run_delegate_blocking(request)
    }

    fn spawn_async(&self, task_id: String, request: DelegateRunRequest) {
        tokio::spawn(async move {
            let reg = AsyncDelegateRegistry::global();
            if reg.is_cancel_requested(&task_id) {
                return;
            }
            match crate::exec::delegate::run_delegate(request).await {
                Ok(json) => {
                    if !reg.is_cancel_requested(&task_id) {
                        reg.finish_ok(&task_id, json);
                    }
                }
                Err(e) => {
                    if !reg.is_cancel_requested(&task_id) {
                        reg.finish_err(&task_id, e.to_string());
                    }
                }
            }
        });
    }

    fn spawn_orchestration(&self, request: OrchestrationSpawnRequest) {
        tokio::spawn(async move {
            if let Err(e) = crate::exec::orchestration::run_orchestration(request).await {
                tracing::warn!(error = %e, "orchestration failed");
            }
        });
    }
}
