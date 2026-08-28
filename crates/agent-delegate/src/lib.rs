//! Git worktree helpers used by explicit desktop multi-task workflows.
//!
//! Subagent threads intentionally do not create implicit worktrees;
//! they inherit the parent workspace and permission policy.

pub mod git_worktree;
pub use git_worktree::{
    cleanup_task_worktree, create_task_worktree, find_git_root, resolve_project_root,
    WorktreeHandle,
};
