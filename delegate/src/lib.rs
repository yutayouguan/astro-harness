//! 委派运行时契约：同步请求、异步任务登记、git worktree 隔离。
//!
//! 实际执行在 `agent::exec::delegate`；tools 只持有 [`DelegateRunner`] / spawner 回调。

pub mod async_reg;
pub mod git_worktree;
pub mod spawn;

pub use async_reg::{
    async_delegate_cancel, async_delegate_collect, async_delegate_status,
    resume_incomplete_async_delegates, start_delegate_async, AsyncDelegateRecord,
    AsyncDelegateRegistry, AsyncDelegateStatus, DelegateAsyncSpawner,
};
pub use git_worktree::{
    cleanup_task_worktree, create_task_worktree, find_git_root, resolve_project_root, WorktreeHandle,
};
pub use spawn::{DelegateRole, DelegateRunRequest, DelegateRunner, DelegateTaskSpec};
