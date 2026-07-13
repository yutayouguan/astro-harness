//! 权限裁决：危险命令检测与放行判断。
//!
//! 生命周期钩子已迁至独立 crate [`hooks`]（`PluginHookBus` / Gateway / Shell）。

pub mod dangerous;
pub mod engine;

pub use dangerous::DangerousCommandDetector;
pub use engine::PermissionEngine;

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
