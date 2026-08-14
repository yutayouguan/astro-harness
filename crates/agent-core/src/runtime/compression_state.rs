//! 压缩与上下文维护状态。
//!
//! 从 `AgentLoop` 提取的数据分组，管理 tool 结果压缩的防抖保护和 mid-run 摘要。
//! 所有压缩状态的纯读写方法均在 `CompressionState` 上实现。

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

impl CompressionState {
    // ── 记忆召回 ──────────────────────────────────────────

    pub fn recalled_context(&self) -> &str {
        &self.last_recalled_context
    }

    // ── mid-run 摘要 ──────────────────────────────────────

    pub fn mid_run_summary_done(&self) -> bool {
        self.mid_run_summary_done
    }

    pub fn mid_run_handoff(&self) -> Option<&str> {
        self.mid_run_handoff.as_deref()
    }

    pub fn set_mid_run_handoff(&mut self, text: String) {
        self.mid_run_handoff = Some(text);
        self.mid_run_summary_done = true;
    }

    pub fn mark_mid_run_summary_skipped(&mut self) {
        self.mid_run_summary_done = true;
    }

    // ── compact 建议 ──────────────────────────────────────

    pub fn should_recommend_compact(&self) -> bool {
        self.pending_recommend_compact
    }

    pub fn take_recommend_compact(&mut self) -> bool {
        let v = self.pending_recommend_compact;
        self.pending_recommend_compact = false;
        v
    }

    // ── 轮次重置 ──────────────────────────────────────────

    /// 重置本轮压缩状态（guard、mid-run、compact 建议）。
    pub fn reset_for_new_turn(&mut self, compression_cfg: &memory::CompressionConfig) {
        self.guard = CompressionThrashingGuard::from_config(compression_cfg);
        self.mid_run_handoff = None;
        self.mid_run_summary_done = false;
        self.pending_recommend_compact = false;
    }
}
