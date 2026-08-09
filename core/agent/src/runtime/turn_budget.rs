//! 轮次与工具深度追踪。
//!
//! 从 `AgentLoop` 提取的数据分组，管理会话轮次和工具迭代预算。
//! 所有轮次/深度相关的纯逻辑均在 `TurnState` 上实现。

/// 轮次与工具深度的运行时计数。
#[derive(Default)]
pub struct TurnState {
    pub(crate) current_turn: usize,
    pub(crate) tool_rounds: usize,
    pub(crate) turn_wrote_disk: bool,
    pub(crate) current_turn_id: Option<String>,
}

impl TurnState {
    // ── turn_id 管理 ───────────────────────────────────────

    pub fn set_current_turn_id(&mut self, id: impl Into<String>) {
        self.current_turn_id = Some(id.into());
    }

    pub fn clear_current_turn_id(&mut self) {
        self.current_turn_id = None;
    }

    pub fn current_turn_id(&self) -> Option<&str> {
        self.current_turn_id.as_deref()
    }

    // ── 会话轮次 ───────────────────────────────────────────

    pub fn current_turn(&self) -> usize {
        self.current_turn
    }

    pub fn is_budget_exhausted(&self, max_turns: usize) -> bool {
        self.current_turn >= max_turns
    }

    pub fn increment_turn(&mut self) {
        self.current_turn += 1;
    }

    // ── 工具深度 ───────────────────────────────────────────

    pub fn is_tool_depth_exhausted(&self, multi_turn: usize) -> bool {
        self.tool_rounds >= multi_turn
    }

    /// 递增工具轮次计数；超出 `multi_turn` 时返回 [`MaxDepthError`]。
    pub fn increment_tool_round(&mut self, multi_turn: usize) -> Result<(), MaxDepthError> {
        if self.is_tool_depth_exhausted(multi_turn) {
            return Err(MaxDepthError {
                limit: multi_turn,
                used: self.tool_rounds,
            });
        }
        self.tool_rounds += 1;
        Ok(())
    }

    // ── 磁盘写入标记 ──────────────────────────────────────

    pub fn turn_wrote_disk(&self) -> bool {
        self.turn_wrote_disk
    }

    pub fn mark_wrote_disk(&mut self) {
        self.turn_wrote_disk = true;
    }

    // ── 轮次重置 ──────────────────────────────────────────

    /// 开始新用户轮次：重置 `tool_rounds` 和 `turn_wrote_disk`，返回上一轮的工具次数。
    pub fn begin_new_turn(&mut self) -> usize {
        let prev = self.tool_rounds;
        self.tool_rounds = 0;
        self.turn_wrote_disk = false;
        prev
    }
}

/// 工具循环超过 `multi_turn` 限制时抛出的错误（对齐 Rig `MaxDepthError`）。
#[derive(Debug, Clone)]
pub struct MaxDepthError {
    /// 配置的上限轮次。
    pub limit: usize,
    /// 已消耗的轮次（触发错误时尚未递增）。
    pub used: usize,
}

impl std::fmt::Display for MaxDepthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "tool multi_turn exhausted: used {} / limit {}",
            self.used, self.limit
        )
    }
}

impl std::error::Error for MaxDepthError {}
