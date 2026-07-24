//! 压缩与上下文维护状态。
//!
//! 从 `AgentLoop` 提取的数据分组，管理 tool 结果压缩的防抖保护和 mid-run 摘要。

use crate::compression::CompressionThrashingGuard;

/// 压缩与上下文维护相关的运行时状态。
#[derive(Default)]
pub struct CompressionState {
    pub(crate) guard: CompressionThrashingGuard,
    pub(crate) mid_run_handoff: Option<String>,
    pub(crate) mid_run_summary_done: bool,
    pub(crate) pending_recommend_compact: bool,
    pub(crate) last_recalled_context: String,
}
