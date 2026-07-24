//! [P2] 从 DecisionLog 中提取 per-skill 失败信号，驱动定向自动进化。
//!
//! 不查 LLM，全部本地计算：遍历 ToolFailure / UserCorrection 条目，
//! 从 `summary` + `tool_name` 中推断涉及哪个 skill，按 skill 计数。

use std::collections::HashMap;

use memory::{DecisionEntry, DecisionKind};

/// 某个 skill 的失败信号摘要。
#[derive(Debug, Clone)]
pub struct SkillSignalSummary {
    pub skill_id: String,
    pub failure_signals: usize,
}

/// 从 DecisionLog 条目中按 skill_id 统计失败信号数。
///
/// 只计入 `ToolFailure` 和 `UserCorrection` 类型；
/// 通过 `extract_skill_id_from_summary` 从 `summary` 文本推断 skill。
pub fn skill_failure_signals(
    decisions: &[DecisionEntry],
    known_skills: &[String],
) -> HashMap<String, usize> {
    let mut map: HashMap<String, usize> = HashMap::new();
    for d in decisions {
        if !matches!(d.kind, DecisionKind::ToolFailure | DecisionKind::UserCorrection) {
            continue;
        }
        // 工具类型过滤：只关注 skills 工具（或 tool_name 未知时全量匹配）
        if let Some(ref tn) = d.tool_name {
            if tn != "skills" && tn != "skill" {
                continue;
            }
        }
        if let Some(skill_id) = extract_skill_id_from_summary(&d.summary, known_skills) {
            *map.entry(skill_id).or_default() += 1;
        }
    }
    map
}

/// 从 summary 文本中提取 skill_id（若存在于 `known_skills` 中）。
///
/// 策略：遍历已知 skill_id，找第一个出现在文本中的。
/// 优先完整词边界匹配，降级到子串匹配。
pub fn extract_skill_id_from_summary(summary: &str, known_skills: &[String]) -> Option<String> {
    let lower = summary.to_lowercase();
    // 优先：被反引号或引号包裹的精确匹配
    for id in known_skills {
        let patterns = [
            format!("`{id}`"),
            format!("\"{id}\""),
            format!("'{id}'"),
            format!("skill_id={id}"),
            format!("skill: {id}"),
        ];
        if patterns.iter().any(|p| lower.contains(p.as_str())) {
            return Some(id.clone());
        }
    }
    // 降级：子串匹配（只有唯一匹配时才返回，避免误报）
    let matches: Vec<&String> = known_skills
        .iter()
        .filter(|id| !id.is_empty() && lower.contains(id.as_str()))
        .collect();
    if matches.len() == 1 {
        return Some(matches[0].clone());
    }
    None
}

/// 返回失败信号最多且 ≥ `min_signals` 的 skill；平局取字典序第一。
pub fn top_failing_skill(
    decisions: &[DecisionEntry],
    known_skills: &[String],
    min_signals: usize,
) -> Option<SkillSignalSummary> {
    if min_signals == 0 {
        return None;
    }
    let counts = skill_failure_signals(decisions, known_skills);
    counts
        .into_iter()
        .filter(|(_, count)| *count >= min_signals)
        .max_by(|(id_a, cnt_a), (id_b, cnt_b)| {
            cnt_a.cmp(cnt_b).then_with(|| id_b.cmp(id_a)) // 计数大优先；平局按字典序小优先
        })
        .map(|(skill_id, failure_signals)| SkillSignalSummary {
            skill_id,
            failure_signals,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory::DecisionKind;

    fn entry(kind: DecisionKind, summary: &str, tool: Option<&str>) -> DecisionEntry {
        let mut e = DecisionEntry::new(kind, summary);
        if let Some(t) = tool {
            e = e.with_tool(t);
        }
        e
    }

    #[test]
    fn counts_by_skill_id_in_backticks() {
        let skills = vec!["demo-skill".to_string(), "other-skill".to_string()];
        let decisions = vec![
            entry(DecisionKind::ToolFailure, "技能 `demo-skill` 执行失败", Some("skills")),
            entry(DecisionKind::ToolFailure, "`demo-skill` 又出错了", Some("skills")),
            entry(DecisionKind::UserCorrection, "skill: demo-skill 结果不对", Some("skills")),
            entry(DecisionKind::ToolFailure, "无关日志", Some("terminal")),
        ];
        let counts = skill_failure_signals(&decisions, &skills);
        assert_eq!(counts.get("demo-skill"), Some(&3));
        assert_eq!(counts.get("other-skill"), None);
    }

    #[test]
    fn top_failing_skill_returns_highest() {
        let skills = vec!["a-skill".to_string(), "b-skill".to_string()];
        let decisions = vec![
            entry(DecisionKind::ToolFailure, "`a-skill` fail", Some("skills")),
            entry(DecisionKind::ToolFailure, "`a-skill` fail again", Some("skills")),
            entry(DecisionKind::ToolFailure, "`b-skill` fail", Some("skills")),
        ];
        let top = top_failing_skill(&decisions, &skills, 2).unwrap();
        assert_eq!(top.skill_id, "a-skill");
        assert_eq!(top.failure_signals, 2);
    }

    #[test]
    fn min_signals_zero_returns_none() {
        let skills = vec!["x".to_string()];
        let decisions = vec![entry(DecisionKind::ToolFailure, "`x` fail", Some("skills"))];
        assert!(top_failing_skill(&decisions, &skills, 0).is_none());
    }
}
