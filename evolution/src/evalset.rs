//! 标注评测集：用户从历史挑例子，标注 task + 期望要点 + 通过/失败。
//!
//! 存 `{base}/learning/evolution/evalset.jsonl`。进化打分时，若候选技能有匹配
//! 例子，则让 judge 针对具体 task+expectations 做 grounded 评分（0–1），比泛化
//! judge 更客观；无匹配则回退泛化 judge（由调用方处理）。
//!
//! 无标准技能运行时，适应度仍是 LLM 评分，但锚定到用户给的具体期望。

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::candidate::{CandidateKind, SkillCandidate};

/// 观测结果：该例子当初是「做对了」还是「做错了」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
}

/// 一条评测例子。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalExample {
    pub id: String,
    /// 该例子评测哪个技能（None = 通用）。
    #[serde(default)]
    pub skill_id: Option<String>,
    /// 任务 / 用户诉求。
    pub task: String,
    /// 期望满足的要点。
    #[serde(default)]
    pub expectations: Vec<String>,
    /// 观测结果。
    pub verdict: Verdict,
    /// 来源会话。
    #[serde(default)]
    pub source_session: Option<String>,
    pub created_at: String,
}

impl EvalExample {
    pub fn new(
        skill_id: Option<String>,
        task: impl Into<String>,
        expectations: Vec<String>,
        verdict: Verdict,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            skill_id,
            task: task.into(),
            expectations,
            verdict,
            source_session: None,
            created_at: Utc::now().to_rfc3339(),
        }
    }
}

/// `{base}/learning/evolution/evalset.jsonl`
pub fn evalset_path(base: &Path) -> PathBuf {
    base.join("learning")
        .join("evolution")
        .join("evalset.jsonl")
}

/// 追加一条例子（append-only JSONL）。
pub fn append_example(base: &Path, ex: &EvalExample) -> anyhow::Result<()> {
    let path = evalset_path(base);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    serde_json::to_writer(&mut f, ex)?;
    f.write_all(b"\n")?;
    Ok(())
}

/// 列出全部例子（跳过坏行）。
pub fn list_examples(base: &Path) -> Vec<EvalExample> {
    let path = evalset_path(base);
    let Ok(f) = fs::File::open(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let Ok(line) = line else { break };
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Ok(ex) = serde_json::from_str::<EvalExample>(t) {
            out.push(ex);
        }
    }
    out
}

/// 按 id 删除；重写文件。
pub fn remove_example(base: &Path, id: &str) -> anyhow::Result<()> {
    let path = evalset_path(base);
    if !path.is_file() {
        return Ok(());
    }
    let kept: Vec<EvalExample> = list_examples(base)
        .into_iter()
        .filter(|e| e.id != id)
        .collect();
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        for e in &kept {
            serde_json::to_writer(&mut f, e)?;
            f.write_all(b"\n")?;
        }
    }
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// 取与某技能匹配的例子（skill_id 相同，或例子为通用 None）。
pub fn examples_for_skill<'a>(all: &'a [EvalExample], skill_id: &str) -> Vec<&'a EvalExample> {
    all.iter()
        .filter(|e| e.skill_id.as_deref() == Some(skill_id) || e.skill_id.is_none())
        .collect()
}

/// optimize / holdout 划分结果（稳定哈希，按 example id 分区）。
#[derive(Debug, Clone)]
pub struct EvalSplit<'a> {
    pub optimize: Vec<&'a EvalExample>,
    pub holdout: Vec<&'a EvalExample>,
    pub holdout_enabled: bool,
}

const MIN_HOLDOUT_TOTAL: usize = 5;
const DEFAULT_HOLDOUT_PERCENT: u8 = 20;

fn stable_bucket(id: &str) -> u8 {
    (id.bytes()
        .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64))
        % 100) as u8
}

/// 将匹配例子划分为 optimize（搜索适应度）与 holdout（最终验证）。
///
/// - 少于 5 条：全部 optimize，holdout 关闭；
/// - 按 example id 稳定哈希分区（增删其它例子不改变已有 id 的分区）；
/// - holdout 尽量同时含 Pass/Fail，避免单一 verdict。
pub fn split_eval_examples<'a>(
    examples: &[&'a EvalExample],
    holdout_percent: u8,
) -> EvalSplit<'a> {
    if examples.len() < MIN_HOLDOUT_TOTAL {
        return EvalSplit {
            optimize: examples.to_vec(),
            holdout: Vec::new(),
            holdout_enabled: false,
        };
    }
    let pct = holdout_percent.clamp(5, 40);
    let mut optimize: Vec<&'a EvalExample> = Vec::new();
    let mut holdout: Vec<&'a EvalExample> = Vec::new();
    for ex in examples {
        if stable_bucket(&ex.id) < pct {
            holdout.push(*ex);
        } else {
            optimize.push(*ex);
        }
    }
    if optimize.is_empty() || holdout.is_empty() {
        return EvalSplit {
            optimize: examples.to_vec(),
            holdout: Vec::new(),
            holdout_enabled: false,
        };
    }
    if !holdout.iter().any(|e| e.verdict == Verdict::Fail) {
        if let Some(i) = optimize.iter().position(|e| e.verdict == Verdict::Fail) {
            holdout.push(optimize.remove(i));
        }
    }
    if !holdout.iter().any(|e| e.verdict == Verdict::Pass) {
        if let Some(i) = optimize.iter().position(|e| e.verdict == Verdict::Pass) {
            holdout.push(optimize.remove(i));
        }
    }
    if optimize.is_empty() || holdout.is_empty() {
        return EvalSplit {
            optimize: examples.to_vec(),
            holdout: Vec::new(),
            holdout_enabled: false,
        };
    }
    EvalSplit {
        optimize,
        holdout,
        holdout_enabled: true,
    }
}

pub fn default_holdout_percent() -> u8 {
    DEFAULT_HOLDOUT_PERCENT
}

/// eval judge 的 system 指令：针对具体 task+expectations 评分，输出结构化 JSON。
pub const EVAL_JUDGE_SYSTEM_PROMPT: &str = r#"你是技能评测器。给定一个技能内容、一个任务与该任务的期望要点，判断「若用该技能执行此任务，能在多大程度上满足期望」。只输出 JSON（不要 markdown 围栏）：
{"score":0.0,"satisfied":["已满足的要点"],"unmet":["未满足的要点"],"reason":"简述"}
score 为 0~1 小数：全部满足≈1，完全不满足≈0。宁严勿滥。satisfied/unmet 列出具体要点。"#;

/// 构造针对单个例子的 eval judge user 提示。
pub fn build_eval_judge_prompt(cand: &SkillCandidate, ex: &EvalExample) -> String {
    let mut s = String::new();
    s.push_str("## 技能内容\n");
    match cand.kind {
        CandidateKind::NewSkill => {
            s.push_str(cand.content.as_deref().unwrap_or(""));
        }
        CandidateKind::Patch => {
            s.push_str(&format!(
                "（patch）将 `{}` 替换为 `{}`",
                cand.old_string.as_deref().unwrap_or(""),
                cand.new_string.as_deref().unwrap_or("")
            ));
        }
    }
    s.push_str(&format!("\n\n## 任务\n{}\n\n## 期望要点\n", ex.task));
    if ex.expectations.is_empty() {
        s.push_str("（未列具体要点，按任务合理判断）\n");
    } else {
        for e in &ex.expectations {
            s.push_str(&format!("- {e}\n"));
        }
    }
    s.push_str("\n请评分并只输出 JSON。");
    s
}

/// 加权均值适应度：`Verdict::Fail` 例权重 2.0，`Verdict::Pass` 例权重 1.0。
///
/// 背景：修复已知失败比维持已通过任务的通过率更有价值；Fail 例加权让
/// 适应度函数优先选出「能修复缺陷」的候选，而非只追求通过率平均分。
///
/// 返回 `None` 当 `verdicts` 为空或权重和为 0。
pub fn weighted_eval_score(verdicts: &[(Verdict, f32)]) -> Option<f32> {
    let (w_sum, w_total) = verdicts.iter().fold((0.0f32, 0.0f32), |(s, w), (v, score)| {
        let weight = match v {
            Verdict::Fail => 2.0,
            Verdict::Pass => 1.0,
        };
        (s + score * weight, w + weight)
    });
    if w_total == 0.0 {
        None
    } else {
        Some((w_sum / w_total).clamp(0.0, 1.0))
    }
}

/// 解析 eval judge 输出为 0–1 分（兼容旧格式）。
pub fn parse_eval_score(raw: &str) -> anyhow::Result<f32> {
    let j = parse_eval_judgement(raw)?;
    Ok(j.score)
}

/// 结构化 eval judge 结果（grounded eval 路径专用）。
#[derive(Debug, Clone)]
pub struct EvalJudgement {
    pub score: f32,
    pub satisfied: Vec<String>,
    pub unmet: Vec<String>,
    pub reason: String,
}

/// 解析结构化 eval judge 输出。`satisfied`/`unmet` 缺失时使用空数组。
pub fn parse_eval_judgement(raw: &str) -> anyhow::Result<EvalJudgement> {
    let trimmed = raw.trim();
    let start = trimmed
        .find('{')
        .ok_or_else(|| anyhow::anyhow!("eval 输出不含 JSON"))?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| anyhow::anyhow!("eval 输出缺少 JSON 结尾"))?;

    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(default)]
        score: f32,
        #[serde(default)]
        satisfied: Vec<String>,
        #[serde(default)]
        unmet: Vec<String>,
        #[serde(default)]
        reason: String,
    }
    let parsed: Raw = serde_json::from_str(&trimmed[start..=end])?;
    Ok(EvalJudgement {
        score: parsed.score.clamp(0.0, 1.0),
        satisfied: parsed.satisfied,
        unmet: parsed.unmet,
        reason: parsed.reason,
    })
}

/// 聚合多条 judgement 的 critique：优先 unmet，去重，按频次排序，最多 `max_items` 条。
pub fn aggregate_critiques(judgements: &[EvalJudgement], max_items: usize) -> Vec<String> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for j in judgements {
        for item in &j.unmet {
            let key = item.trim().to_string();
            if !key.is_empty() {
                *counts.entry(key).or_default() += 1;
            }
        }
    }
    let mut items: Vec<(String, usize)> = counts.into_iter().collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    items
        .into_iter()
        .take(max_items.max(1).min(8))
        .map(|(s, _)| s)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn cand(skill: &str) -> SkillCandidate {
        SkillCandidate {
            id: "1".into(),
            kind: CandidateKind::NewSkill,
            skill_id: skill.into(),
            description: None,
            content: Some("# demo\n步骤".into()),
            old_string: None,
            new_string: None,
            rationale: String::new(),
            sources: vec![],
            judge_score: None,
            judge_reason: None,
            created_at: "now".into(),
        }
    }

    #[test]
    fn append_list_remove_roundtrip() {
        let dir = TempDir::new().unwrap();
        let a = EvalExample::new(
            Some("pdf".into()),
            "合并两个 PDF",
            vec!["保持顺序".into()],
            Verdict::Fail,
        );
        let b = EvalExample::new(None, "通用任务", vec![], Verdict::Pass);
        append_example(dir.path(), &a).unwrap();
        append_example(dir.path(), &b).unwrap();
        assert_eq!(list_examples(dir.path()).len(), 2);
        remove_example(dir.path(), &a.id).unwrap();
        let left = list_examples(dir.path());
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, b.id);
    }

    #[test]
    fn split_is_stable_for_same_ids() {
        let mk = |id: &str, v: Verdict| EvalExample {
            id: id.into(),
            skill_id: Some("pdf".into()),
            task: id.into(),
            expectations: vec![],
            verdict: v,
            source_session: None,
            created_at: "t".into(),
        };
        let a = mk("a", Verdict::Fail);
        let b = mk("b", Verdict::Pass);
        let c = mk("c", Verdict::Fail);
        let d = mk("d", Verdict::Pass);
        let e = mk("e", Verdict::Fail);
        let refs: Vec<&EvalExample> = vec![&a, &b, &c, &d, &e];
        let s1 = split_eval_examples(&refs, 20);
        let s2 = split_eval_examples(&refs, 20);
        assert_eq!(s1.optimize.len(), s2.optimize.len());
        assert_eq!(s1.holdout.len(), s2.holdout.len());
        assert_eq!(s1.holdout_enabled, s2.holdout_enabled);
    }

    #[test]
    fn split_disabled_when_few_examples() {
        let ex = EvalExample::new(Some("s".into()), "t", vec![], Verdict::Pass);
        let refs = vec![&ex];
        let s = split_eval_examples(&refs, 20);
        assert!(!s.holdout_enabled);
        assert_eq!(s.optimize.len(), 1);
        assert!(s.holdout.is_empty());
    }

    #[test]
    fn split_enables_with_five_mixed() {
        let mut all = Vec::new();
        for i in 0..5 {
            all.push(EvalExample::new(
                Some("s".into()),
                format!("t{i}"),
                vec![],
                if i % 2 == 0 {
                    Verdict::Fail
                } else {
                    Verdict::Pass
                },
            ));
        }
        let refs: Vec<&EvalExample> = all.iter().collect();
        let s = split_eval_examples(&refs, 20);
        assert!(s.holdout_enabled);
        assert!(!s.optimize.is_empty());
        assert!(!s.holdout.is_empty());
    }

    #[test]
    fn examples_for_skill_includes_generic() {
        let all = vec![
            EvalExample::new(Some("pdf".into()), "t1", vec![], Verdict::Pass),
            EvalExample::new(Some("other".into()), "t2", vec![], Verdict::Pass),
            EvalExample::new(None, "t3", vec![], Verdict::Pass),
        ];
        let m = examples_for_skill(&all, "pdf");
        assert_eq!(m.len(), 2); // pdf + generic
    }

    #[test]
    fn parse_eval_score_clamps() {
        assert!((parse_eval_score(r#"{"score":0.7}"#).unwrap() - 0.7).abs() < 1e-6);
        assert!((parse_eval_score(r#"{"score":2.0}"#).unwrap() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn eval_prompt_has_task_and_expectations() {
        let ex = EvalExample::new(
            Some("s".into()),
            "做个 X",
            vec!["要点A".into()],
            Verdict::Fail,
        );
        let p = build_eval_judge_prompt(&cand("s"), &ex);
        assert!(p.contains("做个 X"));
        assert!(p.contains("要点A"));
        assert!(p.contains("技能内容"));
    }

    #[test]
    fn weighted_eval_score_empty_returns_none() {
        assert!(weighted_eval_score(&[]).is_none());
    }

    #[test]
    fn weighted_eval_score_fail_weights_double() {
        // Fail(0.4) × 2 + Pass(1.0) × 1 = 1.8 / 3 = 0.6
        let vs = [(Verdict::Fail, 0.4), (Verdict::Pass, 1.0)];
        let s = weighted_eval_score(&vs).unwrap();
        assert!((s - 0.6).abs() < 1e-5, "got {s}");
    }

    #[test]
    fn weighted_eval_score_only_pass_equals_plain_mean() {
        let vs = [(Verdict::Pass, 0.8), (Verdict::Pass, 0.4)];
        let s = weighted_eval_score(&vs).unwrap();
        assert!((s - 0.6).abs() < 1e-5, "got {s}");
    }

    #[test]
    fn weighted_eval_score_clamps_to_one() {
        let vs = [(Verdict::Fail, 1.5)];
        let s = weighted_eval_score(&vs).unwrap();
        assert!((s - 1.0).abs() < 1e-5);
    }

    #[test]
    fn parse_eval_judgement_full() {
        let raw = r#"{"score":0.7,"satisfied":["A"],"unmet":["B","C"],"reason":"ok"}"#;
        let j = parse_eval_judgement(raw).unwrap();
        assert!((j.score - 0.7).abs() < 1e-6);
        assert_eq!(j.satisfied, vec!["A"]);
        assert_eq!(j.unmet, vec!["B", "C"]);
        assert_eq!(j.reason, "ok");
    }

    #[test]
    fn parse_eval_judgement_missing_arrays() {
        let raw = r#"{"score":0.5,"reason":"no arrays"}"#;
        let j = parse_eval_judgement(raw).unwrap();
        assert!(j.satisfied.is_empty());
        assert!(j.unmet.is_empty());
    }

    #[test]
    fn parse_eval_judgement_with_fence() {
        let raw = "```json\n{\"score\":0.3}\n```";
        let j = parse_eval_judgement(raw).unwrap();
        assert!((j.score - 0.3).abs() < 1e-6);
    }

    #[test]
    fn parse_eval_judgement_invalid_json_errors() {
        assert!(parse_eval_judgement("not json at all").is_err());
    }

    #[test]
    fn aggregate_critiques_deduplicates_and_sorts() {
        let judgements = vec![
            EvalJudgement {
                score: 0.5,
                satisfied: vec![],
                unmet: vec!["缺少错误处理".into(), "步骤不清晰".into()],
                reason: String::new(),
            },
            EvalJudgement {
                score: 0.6,
                satisfied: vec!["A".into()],
                unmet: vec!["缺少错误处理".into(), "缺少回退".into()],
                reason: String::new(),
            },
        ];
        let c = aggregate_critiques(&judgements, 8);
        assert_eq!(c[0], "缺少错误处理"); // appears 2x, ranked first
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn aggregate_critiques_caps_at_max() {
        let j = EvalJudgement {
            score: 0.3,
            satisfied: vec![],
            unmet: (0..20).map(|i| format!("item {i}")).collect(),
            reason: String::new(),
        };
        let c = aggregate_critiques(&[j], 3);
        assert_eq!(c.len(), 3);
    }
}
