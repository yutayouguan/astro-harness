//! 每 Agent 独立的迭代预算（对齐 Hermes `IterationBudget`）。
//!
//! - 父 Agent 默认上限为 [`DEFAULT_MAX_ITERATIONS`]（90）
//! - 每个 Agent Thread 使用自己的独立预算
//! - `code_exec` 等廉价轮次可通过 [`IterationBudget::refund`] 退还

use std::cell::Cell;

/// 父 Agent / 主会话默认工具迭代上限（对齐 Hermes `max_iterations`）。
pub const DEFAULT_MAX_ITERATIONS: usize = 90;

/// 单任务迭代计数器：每轮 API/工具迭代 `consume` 一次，必要时 `refund`。
///
/// 仅在单个 async 任务内顺序访问，不需要跨线程同步，故用 `Cell` 而非 `Mutex`。
#[derive(Debug)]
pub struct IterationBudget {
    max_total: usize,
    used: Cell<usize>,
}

impl IterationBudget {
    /// 创建上限为 `max_total` 的预算。
    pub fn new(max_total: usize) -> Self {
        Self {
            max_total,
            used: Cell::new(0),
        }
    }

    /// 预算上限。
    pub fn max_total(&self) -> usize {
        self.max_total
    }

    /// 已消耗次数。
    pub fn used(&self) -> usize {
        self.used.get()
    }

    /// 剩余次数。
    pub fn remaining(&self) -> usize {
        self.max_total.saturating_sub(self.used.get())
    }

    /// 尝试消耗 1 次；已满则返回 `false`。
    pub fn consume(&self) -> bool {
        let cur = self.used.get();
        if cur >= self.max_total {
            return false;
        }
        self.used.set(cur + 1);
        true
    }

    /// 退还 1 次（例如仅 `code_exec` 的轮次，或压缩后重试）。
    pub fn refund(&self) {
        let cur = self.used.get();
        if cur > 0 {
            self.used.set(cur - 1);
        }
    }
}

/// 本轮工具调用是否应退还预算（对齐 Hermes：仅 `execute_code`）。
///
/// Astro 对应工具名为 `code_exec`；必须且只能是该类工具才退还。
pub fn should_refund_tool_round(tool_names: &[&str]) -> bool {
    !tool_names.is_empty() && tool_names.iter().all(|n| *n == "code_exec")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consume_until_exhausted() {
        let b = IterationBudget::new(2);
        assert!(b.consume());
        assert!(b.consume());
        assert!(!b.consume());
        assert_eq!(b.used(), 2);
        assert_eq!(b.remaining(), 0);
    }

    #[test]
    fn refund_restores_one() {
        let b = IterationBudget::new(2);
        assert!(b.consume());
        assert!(b.consume());
        b.refund();
        assert_eq!(b.used(), 1);
        assert!(b.consume());
        assert!(!b.consume());
    }

    #[test]
    fn refund_at_zero_is_noop() {
        let b = IterationBudget::new(1);
        b.refund();
        assert_eq!(b.used(), 0);
        assert!(b.consume());
    }

    #[test]
    fn defaults_match_hermes() {
        assert_eq!(DEFAULT_MAX_ITERATIONS, 90);
    }

    #[test]
    fn code_exec_only_refunds() {
        assert!(should_refund_tool_round(&["code_exec"]));
        assert!(should_refund_tool_round(&["code_exec", "code_exec"]));
        assert!(!should_refund_tool_round(&[]));
        assert!(!should_refund_tool_round(&["exec_command"]));
        assert!(!should_refund_tool_round(&["code_exec", "exec_command"]));
    }
}
