//! 显式 Run 状态机：对齐 Agno Run / requirements 语义。
//!
//! multi_turn 仍以事件流驱动；本模块提供可派生 `RunFinished.outcome_type`
//! 的一等枚举，避免「等待确认 / 执行工具」等语义散落在布尔与字符串里。

use serde::{Deserialize, Serialize};

/// 单次用户发送对应的 run 阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunPhase {
    /// 正在流式请求 / 接收 LLM。
    StreamingLlm,
    /// 正在执行本轮工具。
    ExecutingTools,
    /// 阻塞等待 HITL（confirm / clarify / location / 危险命令审批）。
    AwaitingHitl,
    /// 迭代预算耗尽后的强制总结轮。
    Summarizing,
    /// 正常结束。
    Finished,
    /// 用户取消或连接断开。
    Cancelled,
    /// 不可恢复错误。
    Error,
}

/// Run 当前对用户/外部系统的阻塞需求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunRequirements {
    #[default]
    None,
    /// 等待用户确认（`confirm` / 危险 terminal Ask）。
    UserConfirmation { interrupt_ids: Vec<String> },
    /// 等待用户输入（`clarify` / `request_user_location`）。
    UserInput { interrupt_ids: Vec<String> },
}

impl RunRequirements {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn for_hitl_reason(reason: &str, interrupt_id: impl Into<String>) -> Self {
        let id = interrupt_id.into();
        match reason {
            "confirmation" | "tool_call" => Self::UserConfirmation {
                interrupt_ids: vec![id],
            },
            _ => Self::UserInput {
                interrupt_ids: vec![id],
            },
        }
    }
}

/// 可观测的 run 快照（供日志 / 测试 / 未来 UI）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunState {
    pub phase: RunPhase,
    pub requirements: RunRequirements,
}

impl RunState {
    pub fn new() -> Self {
        Self {
            phase: RunPhase::StreamingLlm,
            requirements: RunRequirements::None,
        }
    }

    pub fn set_phase(&mut self, phase: RunPhase) {
        self.phase = phase;
        if matches!(
            phase,
            RunPhase::Finished | RunPhase::Cancelled | RunPhase::Error | RunPhase::StreamingLlm
        ) {
            self.requirements = RunRequirements::None;
        }
    }

    pub fn await_hitl(&mut self, requirements: RunRequirements) {
        self.phase = RunPhase::AwaitingHitl;
        self.requirements = requirements;
    }

    /// 派生 `RunFinished.outcome_type` 字符串（保持与现有前端契约兼容）。
    pub fn outcome_type(&self) -> &'static str {
        match self.phase {
            RunPhase::AwaitingHitl => "hitl_waiting",
            RunPhase::Cancelled => "interrupt",
            RunPhase::Error => "error",
            RunPhase::Finished | RunPhase::StreamingLlm | RunPhase::ExecutingTools
            | RunPhase::Summarizing => "success",
        }
    }
}

impl Default for RunState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hitl_phase_maps_to_hitl_waiting_outcome() {
        let mut s = RunState::new();
        s.await_hitl(RunRequirements::for_hitl_reason("confirmation", "i1"));
        assert_eq!(s.phase, RunPhase::AwaitingHitl);
        assert_eq!(s.outcome_type(), "hitl_waiting");
        s.set_phase(RunPhase::Finished);
        assert!(s.requirements.is_none());
        assert_eq!(s.outcome_type(), "success");
    }
}
