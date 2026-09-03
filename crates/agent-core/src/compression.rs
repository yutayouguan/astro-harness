//! 运行中工具结果压缩，灵感来自 Agno 的 `CompressionManager`。
//!
//! 关键不变量：原生 `ResponseItem` 始终保留完整工具输出，
//! 而 metadata 中的压缩视图仅供后续模型调用。
//!
//! 主路径（异步，在 `AgentLoop::maintain_tool_context` 中）：prune → 通过
//! `tool_llm_compress` 进行逐条 LLM 摘要 → head/tail 兜底。本模块拥有分阶段
//! 阈值、抖动防护和 head/tail 截断启发式逻辑。

use agent_protocol::ResponseItem;
use memory::CompressionConfig;

use crate::prompt::context_usage::{estimate_tokens, DEFAULT_CONTEXT_WINDOW};

pub const DEFAULT_TOOL_RESULTS_LIMIT: usize = 12;

/// Soft / Medium / Hard：按上下文占用比例选择截断强度。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompressionStage {
    /// 占用 ≥ 该比例时进入本阶段（取最高匹配阶段）。
    pub min_ratio: f32,
    pub max_compressed_chars: usize,
    pub head_chars: usize,
    pub tail_chars: usize,
}

/// 默认三阶段（相对模型上下文窗口）：
/// - Soft 40%：轻压，多留事实
/// - Medium 60%：标准头尾
/// - Hard 80%：强压，保住窗口
pub const DEFAULT_COMPRESSION_STAGES: [CompressionStage; 3] = [
    CompressionStage {
        min_ratio: 0.40,
        max_compressed_chars: 2_400,
        head_chars: 1_600,
        tail_chars: 600,
    },
    CompressionStage {
        min_ratio: 0.60,
        max_compressed_chars: 1_800,
        head_chars: 1_100,
        tail_chars: 500,
    },
    CompressionStage {
        min_ratio: 0.80,
        max_compressed_chars: 900,
        head_chars: 600,
        tail_chars: 200,
    },
];

/// Hard 阶段仍高于该占用时，建议用户 `/compact`（会话级整段摘要）。
///
/// 运行时请优先读 [`CompressionConfig::recommend_compact_ratio`]。
pub const HARD_STAGE_RECOMMEND_COMPACT_RATIO: f32 = 0.85;

/// 单次维护占用下降低于该比例视为「低收益」。
pub const THRASHING_MIN_GAIN_RATIO: f32 = 0.05;

pub const MAX_CONSECUTIVE_LOW_GAIN: u32 = 3;

/// 从 [`CompressionConfig`] 构造 Soft/Medium/Hard 阶段表。
pub fn stages_from_config(cfg: &CompressionConfig) -> Vec<CompressionStage> {
    vec![
        CompressionStage {
            min_ratio: cfg.soft_ratio,
            max_compressed_chars: cfg.soft_max_chars,
            head_chars: cfg.soft_head_chars,
            tail_chars: cfg.soft_tail_chars,
        },
        CompressionStage {
            min_ratio: cfg.medium_ratio,
            max_compressed_chars: cfg.medium_max_chars,
            head_chars: cfg.medium_head_chars,
            tail_chars: cfg.medium_tail_chars,
        },
        CompressionStage {
            min_ratio: cfg.hard_ratio,
            max_compressed_chars: cfg.hard_max_chars,
            head_chars: cfg.hard_head_chars,
            tail_chars: cfg.hard_tail_chars,
        },
    ]
}

#[derive(Debug, Clone)]
pub struct ToolCompressionManager {
    pub enabled: bool,
    /// 未压缩 tool 结果条数阈值；`0` 表示关闭条数触发。
    ///
    /// 默认 12：桌面长工具链下的兜底；主路径是按窗口占用分阶段压缩。
    pub tool_results_limit: usize,
    /// 模型上下文窗口（token）；`0` 时回退 [`DEFAULT_CONTEXT_WINDOW`]。
    pub context_window: u32,
    /// 按占用比例排序的阶段表（升序 `min_ratio`）。
    pub stages: Vec<CompressionStage>,
    /// Soft 阶段比例（prune / 兜底对照）。
    pub soft_ratio: f32,
    /// Hard 阶段比例（prune 对照）。
    pub hard_ratio: f32,
}

impl Default for ToolCompressionManager {
    fn default() -> Self {
        Self::from_config(&CompressionConfig::default())
    }
}

impl ToolCompressionManager {
    pub fn from_config(cfg: &CompressionConfig) -> Self {
        Self {
            enabled: cfg.enabled,
            tool_results_limit: cfg.tool_results_limit,
            context_window: DEFAULT_CONTEXT_WINDOW,
            stages: stages_from_config(cfg),
            soft_ratio: cfg.soft_ratio,
            hard_ratio: cfg.hard_ratio,
        }
    }

    pub fn with_context_window(mut self, window: u32) -> Self {
        self.context_window = if window == 0 {
            DEFAULT_CONTEXT_WINDOW
        } else {
            window
        };
        self
    }

    pub fn with_count_disabled(mut self) -> Self {
        self.tool_results_limit = 0;
        self
    }

    pub fn effective_context_window(&self) -> u32 {
        if self.context_window == 0 {
            DEFAULT_CONTEXT_WINDOW
        } else {
            self.context_window
        }
    }

    /// 条数兜底触发时用 Soft 级预算（`min_ratio` 标 0 仅作标记）。
    fn count_fallback_stage(&self) -> CompressionStage {
        let soft = self
            .stages
            .first()
            .copied()
            .unwrap_or(DEFAULT_COMPRESSION_STAGES[0]);
        CompressionStage {
            min_ratio: 0.0,
            max_compressed_chars: soft.max_compressed_chars,
            head_chars: soft.head_chars,
            tail_chars: soft.tail_chars,
        }
    }

    /// 当前消息估算占用比例（0.0–∞，通常 < 1.0）。
    pub fn occupancy_ratio(&self, items: &[ResponseItem]) -> f32 {
        let window = self.effective_context_window().max(1) as f32;
        estimate_response_items_tokens(items) as f32 / window
    }

    /// 按占用选择最高匹配阶段；未达 Soft 则 `None`。
    pub fn active_stage(&self, items: &[ResponseItem]) -> Option<CompressionStage> {
        let ratio = self.occupancy_ratio(items);
        self.stages
            .iter()
            .rev()
            .find(|s| ratio >= s.min_ratio)
            .copied()
    }

    /// 本次应使用的截断参数：优先窗口阶段，否则条数兜底用 Soft 级。
    pub fn stage_for_compress(&self, items: &[ResponseItem]) -> Option<CompressionStage> {
        if !self.enabled {
            return None;
        }
        if let Some(stage) = self.active_stage(items) {
            return Some(stage);
        }
        let uncompressed = uncompressed_tool_result_count(items);
        if self.tool_results_limit > 0 && uncompressed >= self.tool_results_limit {
            return Some(self.count_fallback_stage());
        }
        None
    }

    /// 是否应压缩：有未压缩 tool，且（条数超限 **或** 已进入任一窗口阶段）。
    pub fn should_compress(&self, items: &[ResponseItem]) -> bool {
        self.enabled && self.stage_for_compress(items).is_some()
    }

    pub fn compress_content(
        &self,
        tool_name: Option<&str>,
        content: &str,
        stage: CompressionStage,
    ) -> Option<String> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return None;
        }

        let char_count = trimmed.chars().count();
        if char_count <= stage.max_compressed_chars {
            // 阈值触发时仍标记为已压缩，避免同一条工具结果在后续轮次被反复重新评估。
            return Some(trimmed.to_string());
        }

        let head_n = stage.head_chars.min(stage.max_compressed_chars);
        let tail_n = stage
            .tail_chars
            .min(stage.max_compressed_chars.saturating_sub(head_n));
        let head: String = trimmed.chars().take(head_n).collect();
        let tail_vec: Vec<char> = trimmed.chars().rev().take(tail_n).collect();
        let tail: String = tail_vec.into_iter().rev().collect();
        let removed = char_count.saturating_sub(head_n + tail_n);
        let name = tool_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown");
        let pct = (stage.min_ratio * 100.0).round() as i32;

        Some(format!(
            "[astro:compressed-tool-result stage≥{pct}%]\nTool: {name}\nOriginal chars: {char_count}; removed middle chars: {removed}.\nPreserved head/tail because the full result is stored in session history.\n\n{head}\n\n...[compressed middle omitted]...\n\n{tail}"
        ))
    }
}

pub fn uncompressed_tool_result_count(items: &[ResponseItem]) -> usize {
    items
        .iter()
        .filter(|item| item.is_tool_output() && item.compressed_text().is_none())
        .count()
}

/// 估算原生 Responses 历史发给模型时的 token 量（ceil(chars/4)）。
pub fn estimate_response_items_tokens(items: &[ResponseItem]) -> u32 {
    let total_chars = items.iter().fold(0usize, |total, item| {
        total.saturating_add(item.provider_view_text().chars().count())
    });
    estimate_tokens(total_chars)
}

#[derive(Debug, Clone)]
pub struct CompressionThrashingGuard {
    consecutive_low_gain: u32,
    pub disabled: bool,
    min_gain_ratio: f32,
    max_consecutive: u32,
    recommend_compact_ratio: f32,
}

impl Default for CompressionThrashingGuard {
    fn default() -> Self {
        Self::from_config(&CompressionConfig::default())
    }
}

impl CompressionThrashingGuard {
    pub fn from_config(cfg: &CompressionConfig) -> Self {
        Self {
            consecutive_low_gain: 0,
            disabled: false,
            min_gain_ratio: cfg.thrashing_min_gain_ratio,
            max_consecutive: cfg.thrashing_max_consecutive,
            recommend_compact_ratio: cfg.recommend_compact_ratio,
        }
    }

    pub fn allow_run(&self) -> bool {
        !self.disabled
    }

    pub fn record_outcome(&mut self, before: f32, after: f32) {
        if before <= 0.0 {
            return;
        }
        let gain = (before - after).max(0.0);
        let low_gain = gain < self.min_gain_ratio || after >= self.recommend_compact_ratio;
        if low_gain {
            self.consecutive_low_gain = self.consecutive_low_gain.saturating_add(1);
            if self.consecutive_low_gain >= self.max_consecutive {
                self.disabled = true;
                tracing::warn!(
                    consecutive = self.consecutive_low_gain,
                    before,
                    after,
                    "tool context maintenance thrashing: disabling auto maintenance for this user turn"
                );
            }
        } else {
            self.consecutive_low_gain = 0;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContextMaintenanceResult {
    pub pruned: usize,
    pub compressed: usize,
    /// 其中通过辅模型 LLM 摘要的条数（含在 `compressed` 内）。
    pub llm_summarized: usize,
    pub stage_ratio: Option<f32>,
    pub occupancy_before: f32,
    pub occupancy_after: f32,
    pub thrashing_disabled: bool,
    pub recommend_session_compact: bool,
    /// PreCompact/PostCompact hook 请求当前活跃回合停止。
    pub hook_stopped: bool,
}

pub fn protect_tail_start_index(message_len: usize, protect_tail_messages: usize) -> usize {
    if message_len == 0 {
        return 0;
    }
    message_len.saturating_sub(protect_tail_messages.max(1))
}

pub fn should_prune_tool_at_stage(
    stage: CompressionStage,
    content_chars: usize,
    soft_ratio: f32,
    hard_ratio: f32,
) -> bool {
    if stage.min_ratio >= hard_ratio {
        return true;
    }
    if stage.min_ratio >= soft_ratio {
        return content_chars >= types::PRUNE_MIN_CHARS;
    }
    false
}

pub fn prune_tool_view(tool_name: Option<&str>, spill_rel: Option<&str>) -> String {
    types::make_prune_view(tool_name, spill_rel)
}

// ── CompressionPolicy trait（压缩策略） ──────────────────────────────────

/// 单条需要压缩的工具消息描述。
#[derive(Debug, Clone)]
pub struct CompressTarget {
    pub message_id: i64,
    pub tool_name: Option<String>,
    pub content: String,
    pub max_chars: usize,
    pub head_chars: usize,
    pub tail_chars: usize,
}

/// 单条需要裁剪为 stub 的工具消息。
#[derive(Debug, Clone)]
pub struct PruneTarget {
    pub message_id: i64,
    pub tool_name: Option<String>,
    pub spill_rel: Option<String>,
}

/// 压缩计划：由 [`CompressionPolicy::plan`] 返回。
#[derive(Debug, Default)]
pub struct CompressionPlan {
    /// 需要裁剪为 stub 的消息列表。
    pub prune: Vec<PruneTarget>,
    /// 需要压缩的消息及目标参数。
    pub compress: Vec<CompressTarget>,
    /// 本次计划对应的阶段占用比例。
    pub stage_ratio: Option<f32>,
    /// 计划前的窗口占用比例。
    pub occupancy_before: f32,
}

/// 工具结果压缩策略接口。
///
/// [`StagedCompressionPolicy`]（三阶段渐进）是默认实现。
/// 实现此 trait 可自定义压缩策略（如语义感知、优先级排序等）。
pub trait CompressionPolicy: Send {
    /// 分析当前消息状态，生成压缩计划。
    ///
    /// `stored_messages` 为 DB 中的完整消息列表；
    /// `history` 为原生 Responses 内存镜像（metadata 可含压缩视图）；
    /// `protect_last_n` 为尾部保护消息数。
    fn plan(
        &self,
        stored_messages: &[::session::StoredResponseItem],
        history: &[ResponseItem],
        memory_dir: &std::path::Path,
        session_id: &str,
        protect_last_n: usize,
    ) -> CompressionPlan;

    /// 非 LLM 降级压缩（head/tail 截断）。
    fn compress_fallback(
        &self,
        tool_name: Option<&str>,
        content: &str,
        target: &CompressTarget,
    ) -> Option<String>;

    /// 压缩后占用仍高，是否建议用户 `/compact`。
    fn should_recommend_compact(&self, occupancy_after: f32) -> bool;
}

/// 默认三阶段渐进压缩策略（Soft / Medium / Hard）。
pub struct StagedCompressionPolicy {
    manager: ToolCompressionManager,
    recommend_compact_ratio: f32,
}

impl StagedCompressionPolicy {
    pub fn from_config(cfg: &CompressionConfig) -> Self {
        Self {
            manager: ToolCompressionManager::from_config(cfg),
            recommend_compact_ratio: cfg.recommend_compact_ratio,
        }
    }

    pub fn with_context_window(mut self, window: u32) -> Self {
        self.manager = self.manager.with_context_window(window);
        self
    }
}

impl CompressionPolicy for StagedCompressionPolicy {
    fn plan(
        &self,
        stored_messages: &[::session::StoredResponseItem],
        history: &[ResponseItem],
        memory_dir: &std::path::Path,
        session_id: &str,
        protect_last_n: usize,
    ) -> CompressionPlan {
        let mut plan = CompressionPlan::default();
        if !self.manager.enabled {
            return plan;
        }
        let Some(stage) = self.manager.stage_for_compress(history) else {
            return plan;
        };

        plan.stage_ratio = Some(stage.min_ratio);
        plan.occupancy_before = self.manager.occupancy_ratio(history);

        let protect_start = protect_tail_start_index(stored_messages.len(), protect_last_n.max(1));

        for (idx, stored_msg) in stored_messages.iter().enumerate() {
            if !stored_msg.is_tool_output() {
                continue;
            }
            let content = stored_msg.text();
            if content.trim().is_empty() {
                continue;
            }

            let spill_path =
                types::tool_spill::spill_file_path(memory_dir, session_id, stored_msg.id);
            let spill_rel = spill_path
                .exists()
                .then(|| types::spill_path_for_prompt(memory_dir, &spill_path));

            if idx < protect_start
                && should_prune_tool_at_stage(
                    stage,
                    content.chars().count(),
                    self.manager.soft_ratio,
                    self.manager.hard_ratio,
                )
            {
                let current = stored_msg.compressed_text().unwrap_or(&content);
                if types::is_externalized_view(current)
                    && current.chars().count() <= stage.max_compressed_chars
                {
                    continue;
                }
                plan.prune.push(PruneTarget {
                    message_id: stored_msg.id,
                    tool_name: stored_msg.qualified_tool_name(),
                    spill_rel,
                });
                continue;
            }

            let needs_compress = match stored_msg.compressed_text() {
                None => true,
                Some(c) => {
                    !types::is_externalized_view(c)
                        && c.chars().count() > stage.max_compressed_chars
                }
            };
            if !needs_compress {
                continue;
            }

            if content.chars().count() <= stage.max_compressed_chars {
                plan.compress.push(CompressTarget {
                    message_id: stored_msg.id,
                    tool_name: stored_msg.qualified_tool_name(),
                    content: content.clone(),
                    max_chars: stage.max_compressed_chars,
                    head_chars: stage.head_chars,
                    tail_chars: stage.tail_chars,
                });
                continue;
            }

            plan.compress.push(CompressTarget {
                message_id: stored_msg.id,
                tool_name: stored_msg.qualified_tool_name(),
                content,
                max_chars: stage.max_compressed_chars,
                head_chars: stage.head_chars,
                tail_chars: stage.tail_chars,
            });
        }

        plan
    }

    fn compress_fallback(
        &self,
        tool_name: Option<&str>,
        content: &str,
        target: &CompressTarget,
    ) -> Option<String> {
        let stage = CompressionStage {
            min_ratio: 0.0,
            max_compressed_chars: target.max_chars,
            head_chars: target.head_chars,
            tail_chars: target.tail_chars,
        };
        self.manager.compress_content(tool_name, content, stage)
    }

    fn should_recommend_compact(&self, occupancy_after: f32) -> bool {
        occupancy_after >= self.recommend_compact_ratio
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(text: &str) -> ResponseItem {
        ResponseItem::FunctionCallOutput {
            id: None,
            call_id: None,
            name: Some("test".into()),
            namespace: None,
            output: agent_protocol::FunctionCallOutputPayload::from_text(text.into()),
            internal_chat_message_metadata_passthrough: None,
        }
    }

    fn mark_compressed(item: &mut ResponseItem, text: &str) {
        *item.metadata_mut().expect("tool output metadata") =
            Some(serde_json::json!({"astro_compressed_output": text}));
    }

    #[test]
    fn threshold_counts_uncompressed_tool_results() {
        let messages: Vec<_> = (0..DEFAULT_TOOL_RESULTS_LIMIT)
            .map(|i| tool(&format!("tool-{i}")))
            .collect();
        let mgr = ToolCompressionManager::default();
        assert!(mgr.should_compress(&messages));

        let mut almost = messages;
        almost.pop();
        assert!(!mgr.should_compress(&almost));

        let mut marked = vec![tool("a"), tool("b"), tool("c")];
        for item in &mut marked {
            mark_compressed(item, "done");
        }
        assert!(!mgr.should_compress(&marked));
    }

    #[test]
    fn window_stage_triggers_relative_to_context_window() {
        // 小窗口：少量内容即可摸到 Soft 40%
        let mgr = ToolCompressionManager::default()
            .with_context_window(1_000)
            .with_count_disabled();
        // ~800 chars → ~200 tokens → 20% of 1000 — below soft
        let mid = "x".repeat(800);
        let below = vec![tool(&mid)];
        assert!(mgr.occupancy_ratio(&below) < 0.40);
        assert!(!mgr.should_compress(&below));

        // ~2000 chars → ~500 tokens → 50% of 1000 — soft
        let big = "x".repeat(2_000);
        let above = vec![tool(&big)];
        assert!(mgr.occupancy_ratio(&above) >= 0.40);
        assert_eq!(mgr.active_stage(&above).map(|s| s.min_ratio), Some(0.40));
        assert!(mgr.should_compress(&above));
    }

    #[test]
    fn hard_stage_when_occupancy_high() {
        let mgr = ToolCompressionManager::default()
            .with_context_window(1_000)
            .with_count_disabled();
        // ~3600 chars → ~900 tokens → 90%
        let huge = "x".repeat(3_600);
        let messages = vec![tool(&huge)];
        assert_eq!(mgr.active_stage(&messages).map(|s| s.min_ratio), Some(0.80));
        let stage = mgr.stage_for_compress(&messages).unwrap();
        let out = mgr.compress_content(Some("search"), &huge, stage).unwrap();
        assert!(out.contains("stage≥80%"));
        assert!(out.len() < huge.len());
    }

    #[test]
    fn from_config_overrides_ratios_and_budgets() {
        let cfg = CompressionConfig {
            soft_ratio: 0.30,
            medium_ratio: 0.50,
            hard_ratio: 0.70,
            soft_max_chars: 1_000,
            soft_head_chars: 700,
            soft_tail_chars: 200,
            tool_results_limit: 0,
            ..Default::default()
        };
        let mgr = ToolCompressionManager::from_config(&cfg).with_context_window(1_000);
        // ~1600 chars → ~400 tokens → 40% → Soft (30%)
        let mid = "x".repeat(1_600);
        let messages = vec![tool(&mid)];
        let stage = mgr.active_stage(&messages).unwrap();
        assert!((stage.min_ratio - 0.30).abs() < 1e-6);
        assert_eq!(stage.max_compressed_chars, 1_000);
    }

    #[test]
    fn disabled_config_never_compresses() {
        let cfg = CompressionConfig {
            enabled: false,
            ..Default::default()
        };
        let mgr = ToolCompressionManager::from_config(&cfg).with_context_window(100);
        let huge = "x".repeat(10_000);
        assert!(!mgr.should_compress(&[tool(&huge)]));
    }

    #[test]
    fn no_uncompressed_tools_never_triggers() {
        let mut messages = vec![tool("a"), tool("b"), tool("c")];
        for item in &mut messages {
            mark_compressed(item, "done");
        }
        let mgr = ToolCompressionManager::default().with_context_window(100);
        assert!(!mgr.should_compress(&messages));
    }

    #[test]
    fn estimate_prefers_compressed_view() {
        let big = "x".repeat(400);
        let mut m = tool(&big);
        let before = estimate_response_items_tokens(std::slice::from_ref(&m));
        mark_compressed(&mut m, "short");
        let after = estimate_response_items_tokens(std::slice::from_ref(&m));
        assert!(after < before);
    }

    #[test]
    fn estimate_ignores_user_delivery_marker() {
        let content = "follow the complete durable instruction ".repeat(20);
        let mut user = ResponseItem::user_text(&content);
        let expected = estimate_response_items_tokens(std::slice::from_ref(&user));
        *user.metadata_mut().unwrap() = Some(serde_json::json!({
            "astro_memory_marker": "agent-mailbox-through:42"
        }));

        assert_eq!(
            estimate_response_items_tokens(std::slice::from_ref(&user)),
            expected
        );
    }

    #[test]
    fn heuristic_keeps_tool_facts_and_shortens_long_output() {
        let mgr = ToolCompressionManager::default();
        let content = format!(
            "id=abc123 price=42\n{}\nfinal_url=https://example.com/end",
            "middle filler ".repeat(400)
        );
        let compressed = mgr
            .compress_content(Some("search"), &content, DEFAULT_COMPRESSION_STAGES[1])
            .expect("compressed");
        assert!(compressed.len() < content.len());
        assert!(compressed.contains("id=abc123"));
        assert!(compressed.contains("final_url=https://example.com/end"));
    }

    #[test]
    fn thrashing_disables_after_low_gain_streak() {
        let mut guard = CompressionThrashingGuard::default();
        guard.record_outcome(0.90, 0.89);
        guard.record_outcome(0.89, 0.88);
        assert!(!guard.disabled);
        guard.record_outcome(0.88, 0.87);
        assert!(guard.disabled);
    }

    #[test]
    fn hard_stage_prunes_all_outside_tail() {
        let stage = DEFAULT_COMPRESSION_STAGES[2];
        assert!(should_prune_tool_at_stage(stage, 10, 0.40, 0.80));
    }
}
