//! Astro 统一错误类型。

use thiserror::Error;

/// 跨模块可识别的错误分类。
#[derive(Debug, Error)]
pub enum AstroError {
    /// 供应商 / HTTP / 模型相关失败。
    #[error("Provider error: {0}")]
    Provider(String),
    /// 记忆 / 工作区 / 持久化失败。
    #[error("Memory error: {0}")]
    Memory(String),
    /// 权限检查拒绝。
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    /// Agent 回合预算耗尽（值为已用回合数）。
    #[error("Budget exhausted after {0} turns")]
    BudgetExhausted(usize),
    /// 其它任意错误。
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
