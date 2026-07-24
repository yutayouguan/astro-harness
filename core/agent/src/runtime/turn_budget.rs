//! 轮次与工具深度追踪。
//!
//! 从 `AgentLoop` 提取的数据分组，管理会话轮次和工具迭代预算。

/// 轮次与工具深度的运行时计数。
#[derive(Default)]
pub struct TurnState {
    pub(crate) current_turn: usize,
    pub(crate) tool_rounds: usize,
    pub(crate) turn_wrote_disk: bool,
    pub(crate) current_turn_id: Option<String>,
}
