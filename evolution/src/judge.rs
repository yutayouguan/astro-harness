//! Judge：对通过静态门禁的候选打分（0–1）并给保留建议。
//!
//! LLM 调用由调用方（Tauri）注入：用 [`JUDGE_SYSTEM_PROMPT`] +
//! [`build_judge_user_prompt`] 得文本，模型输出交给 [`parse_judge_output`]。

use crate::candidate::{CandidateKind, SkillCandidate};

/// judge 裁决。
#[derive(Debug, Clone, PartialEq)]
pub struct JudgeVerdict {
    /// 0–1 分，越高越值得保留。
    pub score: f32,
    /// 是否建议保留。
    pub keep: bool,
    /// 评语。
    pub reason: String,
}

/// judge 的 system 指令：只输出 JSON。
pub const JUDGE_SYSTEM_PROMPT: &str = r#"你是 Agent 技能评审。评估一个技能候选是否值得保留：可复用、正确、不冗余、不含危险指令、与既有技能不重复。只输出 JSON（不要 markdown 围栏）：
{"score":0.0,"keep":true,"reason":"简述"}
score 为 0~1 的小数；keep 为布尔。宁严勿滥：低质量或投机性候选给低分。"#;

/// 构造单个候选的 judge user 提示词。
pub fn build_judge_user_prompt(cand: &SkillCandidate, enabled_skills: &[(String, String)]) -> String {
    let mut s = String::new();
    s.push_str("## 现有技能\n");
    if enabled_skills.is_empty() {
        s.push_str("（无）\n");
    } else {
        for (name, desc) in enabled_skills {
            s.push_str(&format!("- {name}: {}\n", desc.replace('\n', " ")));
        }
    }
    s.push_str("\n## 候选\n");
    match cand.kind {
        CandidateKind::NewSkill => {
            s.push_str(&format!("类型: 新建技能 `{}`\n", cand.skill_id));
            if let Some(d) = &cand.description {
                s.push_str(&format!("描述: {d}\n"));
            }
            s.push_str("正文:\n");
            s.push_str(cand.content.as_deref().unwrap_or(""));
        }
        CandidateKind::Patch => {
            s.push_str(&format!("类型: patch 技能 `{}`\n", cand.skill_id));
            s.push_str(&format!(
                "- 原文: {}\n+ 新文: {}\n",
                cand.old_string.as_deref().unwrap_or(""),
                cand.new_string.as_deref().unwrap_or("")
            ));
        }
    }
    if !cand.rationale.trim().is_empty() {
        s.push_str(&format!("\n提出理由: {}\n", cand.rationale));
    }
    s.push_str("\n请评审并只输出 JSON。");
    s
}

/// 解析 judge 输出（容忍 ```json 围栏）。
pub fn parse_judge_output(raw: &str) -> anyhow::Result<JudgeVerdict> {
    let trimmed = raw.trim();
    let start = trimmed
        .find('{')
        .ok_or_else(|| anyhow::anyhow!("judge 输出不含 JSON 对象"))?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| anyhow::anyhow!("judge 输出缺少 JSON 结尾"))?;

    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(default)]
        score: f32,
        #[serde(default)]
        keep: Option<bool>,
        #[serde(default)]
        reason: String,
    }
    let parsed: Raw = serde_json::from_str(&trimmed[start..=end])?;
    let score = parsed.score.clamp(0.0, 1.0);
    // keep 缺省时按 score >= 0.5 推断
    let keep = parsed.keep.unwrap_or(score >= 0.5);
    Ok(JudgeVerdict {
        score,
        keep,
        reason: parsed.reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand() -> SkillCandidate {
        SkillCandidate {
            id: "1".into(),
            kind: CandidateKind::NewSkill,
            skill_id: "demo".into(),
            description: Some("d".into()),
            content: Some("# demo\n步骤".into()),
            old_string: None,
            new_string: None,
            rationale: "复用".into(),
            sources: vec![],
            judge_score: None,
            judge_reason: None,
            created_at: "now".into(),
        }
    }

    #[test]
    fn parses_score_and_keep() {
        let v = parse_judge_output(r#"{"score":0.82,"keep":true,"reason":"good"}"#).unwrap();
        assert!((v.score - 0.82).abs() < 1e-6);
        assert!(v.keep);
        assert_eq!(v.reason, "good");
    }

    #[test]
    fn keep_inferred_from_score_when_absent() {
        let low = parse_judge_output(r#"{"score":0.3}"#).unwrap();
        assert!(!low.keep);
        let high = parse_judge_output(r#"{"score":0.7}"#).unwrap();
        assert!(high.keep);
    }

    #[test]
    fn clamps_out_of_range() {
        let v = parse_judge_output(r#"{"score":9.0,"keep":true}"#).unwrap();
        assert!((v.score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn prompt_includes_candidate_body() {
        let p = build_judge_user_prompt(&cand(), &[("aihot".into(), "AI".into())]);
        assert!(p.contains("新建技能"));
        assert!(p.contains("步骤"));
        assert!(p.contains("aihot"));
    }
}
