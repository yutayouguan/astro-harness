//! [P3] 探测器驱动的 Mutation Hints。
//!
//! 4 个本地探测器从运行时数据（DecisionLog、文件大小、使用记录、curator 健康分）
//! 提炼出短文本 hints，注入 mutation prompt，给 LLM 明确的改进方向。
//! 全部本地计算，不调 LLM，不阻塞。

use std::path::Path;

use memory::DecisionEntry;

use crate::curator::CurateSkillRow;
use crate::signal::skill_failure_signals;

/// 一条机会提示（注入 mutation prompt）。
#[derive(Debug, Clone)]
pub struct OpportunityHint {
    /// 短标签（用于 UI pill 展示）。
    pub tag: String,
    /// 注入到 prompt 的自然语言描述。
    pub focus: String,
}

/// 体积阈值：超过 8KB 触发 large-skill 探测。
const LARGE_SKILL_BYTES: u64 = 8_192;

/// 闲置天数阈值：超过 30 天未加载触发 stale 探测。
const STALE_DAYS: i64 = 30;

/// 健康分阈值：低于 0.5 触发 low-health 探测。
const LOW_HEALTH_SCORE: f32 = 0.5;

/// 高失败信号阈值（复用 signal 模块定义）。
const HIGH_CORRECTION_MIN: usize = 3;

/// 运行所有探测器，返回本次 mutation 的 hints（可为空）。
///
/// - `skill_id`: 当前进化目标的 skill id
/// - `base`: `~/.astro/` 根目录（用于读 skill-usage.json / skill 文件）
/// - `decisions`: 最近 DecisionLog 条目（P2 信号来源）
/// - `curator_rows`: 上次 curator 报告中的技能健康行（可选）
/// - `known_skills`: 所有已启用 skill id 列表（用于 P2 计数）
pub fn detect_opportunities(
    skill_id: &str,
    base: &Path,
    decisions: &[DecisionEntry],
    curator_rows: Option<&[CurateSkillRow]>,
    known_skills: &[String],
) -> Vec<OpportunityHint> {
    let mut hints = Vec::new();

    // ── P3-1: HighCorrectionDetector ────────────────────────────────────────
    let counts = skill_failure_signals(decisions, known_skills);
    if let Some(&n) = counts.get(skill_id) {
        if n >= HIGH_CORRECTION_MIN {
            hints.push(OpportunityHint {
                tag: "high-correction-rate".to_string(),
                focus: format!(
                    "近期 DecisionLog 有 {n} 条与该技能相关的失败/订正信号，\
                     重点改善准确性与输出可靠性。"
                ),
            });
        }
    }

    // ── P3-2: LargeSkillDetector ─────────────────────────────────────────────
    if let Some(bytes) = skill_file_bytes(base, skill_id) {
        if bytes > LARGE_SKILL_BYTES {
            hints.push(OpportunityHint {
                tag: "large-skill".to_string(),
                focus: format!(
                    "SKILL.md 体积 {:.1}KB，超过推荐上限 8KB，\
                     考虑拆分为多个专注技能或精简重复内容。",
                    bytes as f64 / 1024.0
                ),
            });
        }
    }

    // ── P3-3: StaleSkillDetector ─────────────────────────────────────────────
    if let Some(last_loaded_dt) = skills::last_loaded_at(base, skill_id) {
        let days = chrono::Utc::now()
            .signed_duration_since(last_loaded_dt)
            .num_days();
        if days >= STALE_DAYS {
            hints.push(OpportunityHint {
                tag: "stale".to_string(),
                focus: format!(
                    "该技能已 {days} 天未被加载使用，\
                     考虑确认是否仍与当前工作流匹配，\
                     或简化/删除不再需要的部分。"
                ),
            });
        }
    }

    // ── P3-4: CuratorHealthDetector ──────────────────────────────────────────
    if let Some(rows) = curator_rows {
        if let Some(row) = rows.iter().find(|r| r.skill_id == skill_id) {
            if row.health_score.is_some_and(|s| s < LOW_HEALTH_SCORE) {
                let reasons = row.health_reasons.join("；");
                hints.push(OpportunityHint {
                    tag: "low-health".to_string(),
                    focus: format!(
                        "Curator 健康诊断评分 {:.2}（低于 0.5），问题：{reasons}，\
                         请针对性改善。",
                        row.health_score.unwrap_or(0.0)
                    ),
                });
            }
        }
    }

    hints
}

/// 获取 skill SKILL.md 的字节数；找不到返回 None。
fn skill_file_bytes(base: &Path, skill_id: &str) -> Option<u64> {
    // 尝试全局技能目录
    let candidates = [
        base.join("skills").join(skill_id).join("SKILL.md"),
        base.join("workspace").join("skills").join(skill_id).join("SKILL.md"),
    ];
    for p in &candidates {
        if let Ok(meta) = std::fs::metadata(p) {
            return Some(meta.len());
        }
    }
    // 通过 skills crate 加载（最准确但稍慢）
    skills::load_skill_by_name(skill_id)
        .ok()
        .map(|s| s.content.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_when_no_signals() {
        let dir = tempfile::tempdir().unwrap();
        let hints = detect_opportunities("x-skill", dir.path(), &[], None, &[]);
        assert!(hints.is_empty());
    }

    #[test]
    fn high_correction_detected() {
        use memory::{DecisionEntry, DecisionKind};
        let skills = vec!["target-skill".to_string()];
        let decisions: Vec<DecisionEntry> = (0..4)
            .map(|_| {
                DecisionEntry::new(
                    DecisionKind::ToolFailure,
                    "`target-skill` failed",
                )
                .with_tool("skills")
            })
            .collect();
        let dir = tempfile::tempdir().unwrap();
        let hints = detect_opportunities("target-skill", dir.path(), &decisions, None, &skills);
        assert!(hints.iter().any(|h| h.tag == "high-correction-rate"));
    }
}
