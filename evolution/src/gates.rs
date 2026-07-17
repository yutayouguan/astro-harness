//! 候选门禁：体积、patch 结构完整性（唯一匹配在 apply 时校验）。
//!
//! `run_tests`：技能多为 Markdown，无 pytest 基建；v1 仅在候选目标技能目录下
//! 存在 `scripts/test.*` 时由调用方另行执行，本函数不涉及测试运行。

use memory::EvolutionGates;

use crate::candidate::{CandidateKind, SkillCandidate};

/// 门禁结果。
#[derive(Debug, Clone)]
pub struct GateOutcome {
    pub passed: bool,
    pub reasons: Vec<String>,
}

impl GateOutcome {
    fn pass() -> Self {
        Self {
            passed: true,
            reasons: Vec::new(),
        }
    }
    fn fail(reason: impl Into<String>) -> Self {
        Self {
            passed: false,
            reasons: vec![reason.into()],
        }
    }
}

/// 对单个候选做静态门禁（不含唯一匹配与测试运行）。
pub fn check_candidate(c: &SkillCandidate, gates: &EvolutionGates) -> GateOutcome {
    // 体积门禁
    let len = c.payload_len();
    if len == 0 {
        return GateOutcome::fail("候选内容为空");
    }
    if gates.max_skill_bytes > 0 && len > gates.max_skill_bytes {
        return GateOutcome::fail(format!(
            "超出体积上限：{len} > {} 字节",
            gates.max_skill_bytes
        ));
    }
    // patch 结构完整性
    if c.kind == CandidateKind::Patch {
        let old_ok = c.old_string.as_deref().map(str::is_empty) == Some(false);
        if !old_ok {
            return GateOutcome::fail("patch 缺少 old_string");
        }
        if c.new_string.is_none() {
            return GateOutcome::fail("patch 缺少 new_string");
        }
        if c.old_string == c.new_string {
            return GateOutcome::fail("patch old_string 与 new_string 相同");
        }
    }
    GateOutcome::pass()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::CandidateKind;

    fn cand(kind: CandidateKind) -> SkillCandidate {
        SkillCandidate {
            id: "1".into(),
            kind,
            skill_id: "demo".into(),
            description: None,
            content: Some("# demo".into()),
            old_string: Some("a".into()),
            new_string: Some("b".into()),
            rationale: String::new(),
            sources: vec![],
            created_at: "now".into(),
        }
    }

    #[test]
    fn size_gate_blocks_oversize() {
        let mut c = cand(CandidateKind::NewSkill);
        c.content = Some("x".repeat(100));
        let gates = EvolutionGates {
            run_tests: false,
            max_skill_bytes: 10,
            require_pr: true,
        };
        let out = check_candidate(&c, &gates);
        assert!(!out.passed);
        assert!(out.reasons[0].contains("体积"));
    }

    #[test]
    fn patch_needs_distinct_old_new() {
        let mut c = cand(CandidateKind::Patch);
        c.new_string = c.old_string.clone();
        let gates = EvolutionGates::default();
        assert!(!check_candidate(&c, &gates).passed);
    }

    #[test]
    fn valid_new_skill_passes() {
        let c = cand(CandidateKind::NewSkill);
        assert!(check_candidate(&c, &EvolutionGates::default()).passed);
    }
}
