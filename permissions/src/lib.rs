//! 权限与 Hook 总线。
//!
//! 提供危险命令检测、命令放行判断，以及工具/技能/记忆生命周期上的钩子。

pub mod dangerous;
pub mod engine;
pub mod hooks;

pub use dangerous::DangerousCommandDetector;
pub use engine::PermissionEngine;
pub use hooks::{HookAction, HookBus, HookEvent};

use serde::{Deserialize, Serialize};

/// 一次权限裁决结果。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionDecision {
    /// 是否允许。
    pub allowed: bool,
    /// 拒绝或补充说明。
    pub reason: Option<String>,
}

/// 构造「允许」裁决。
pub fn allow() -> PermissionDecision {
    PermissionDecision {
        allowed: true,
        reason: None,
    }
}
