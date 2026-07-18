//! GEPA-lite 遗传搜索：多变体 + Pareto 选择（judge 分↑ / 体积↓）。
//!
//! 无标注评测集，适应度来自 judge 分与写入体积；变异/评分模型调用由 Tauri 注入。
//! 仅做「变异 + 反思回喂」，不做交叉（crossover）。

use crate::candidate::{CandidateKind, SkillCandidate};

/// 一个被评分的变体。
#[derive(Debug, Clone)]
pub struct ScoredVariant {
    pub candidate: SkillCandidate,
    /// judge 分（0–1，越高越好）。
    pub score: f32,
    /// 写入体积字节（越小越好）。
    pub size: usize,
}

impl ScoredVariant {
    pub fn new(candidate: SkillCandidate, score: f32) -> Self {
        let size = candidate.payload_len();
        Self {
            candidate,
            score,
            size,
        }
    }

    /// 自身是否支配 `other`：score≥ 且 size≤，且至少一维严格更优。
    fn dominates(&self, other: &ScoredVariant) -> bool {
        let ge = self.score >= other.score && self.size <= other.size;
        let strictly = self.score > other.score || self.size < other.size;
        ge && strictly
    }
}

/// 计算 Pareto 非支配前沿（保持输入顺序）。
pub fn pareto_front(variants: &[ScoredVariant]) -> Vec<ScoredVariant> {
    variants
        .iter()
        .filter(|v| !variants.iter().any(|o| o.dominates(v)))
        .cloned()
        .collect()
}

/// 从前沿按 score 降序（同分取更小 size）取前 `n`。
pub fn select_front_capped(mut front: Vec<ScoredVariant>, n: usize) -> Vec<ScoredVariant> {
    front.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.size.cmp(&b.size))
    });
    front.truncate(n);
    front
}

/// 变异模型 system 指令：产出多个改进变体，JSON only。
pub const MUTATION_SYSTEM_PROMPT: &str = r#"你是技能进化的变异器。给定一个技能候选（种子）与上一代的评审意见，产出若干**改进后的变体**。只输出 JSON（不要 markdown 围栏）：
{"variants":[{"kind":"new_skill|patch","skill_id":"同种子","description":"...","content":"...","old_string":"...","new_string":"...","rationale":"改了什么、为什么更好"}]}
规则：
- 每个变体针对同一 skill_id 与同一 kind；在种子基础上做有意义的差异化改进（更清晰、更健壮、更简洁、修正评审指出的问题）。
- new_skill 用 content；patch 用 old_string/new_string。
- 不要臆造事实；宁可少而精。"#;

/// 构造变异 user 提示：种子 + 期望变体数 + 可选上一代评语。
pub fn build_mutation_prompt(
    seed: &SkillCandidate,
    variants: u32,
    critiques: &[String],
) -> String {
    let mut s = String::new();
    s.push_str(&format!("目标：产出 {variants} 个改进变体。\n\n## 种子候选\n"));
    s.push_str(&format!("skill_id: {}\n", seed.skill_id));
    match seed.kind {
        CandidateKind::NewSkill => {
            s.push_str("kind: new_skill\n");
            if let Some(d) = &seed.description {
                s.push_str(&format!("description: {d}\n"));
            }
            s.push_str("content:\n");
            s.push_str(seed.content.as_deref().unwrap_or(""));
            s.push('\n');
        }
        CandidateKind::Patch => {
            s.push_str("kind: patch\n");
            s.push_str(&format!(
                "old_string: {}\nnew_string: {}\n",
                seed.old_string.as_deref().unwrap_or(""),
                seed.new_string.as_deref().unwrap_or("")
            ));
        }
    }
    if !critiques.is_empty() {
        s.push_str("\n## 上一代评审意见（请针对性改进）\n");
        for c in critiques {
            if !c.trim().is_empty() {
                s.push_str(&format!("- {}\n", c.replace('\n', " ")));
            }
        }
    }
    s.push_str("\n请输出变体 JSON。");
    s
}

/// 交叉算子 system 指令：融合两个父代为一个子代，JSON only。
pub const CROSSOVER_SYSTEM_PROMPT: &str = r#"你是技能进化的交叉算子。给定同一技能的两个候选变体（父代 A / B），产出**一个**融合两者优点的子代（取各自更好的部分、去除冗余与缺陷）。只输出 JSON（不要 markdown 围栏）：
{"variants":[{"kind":"new_skill|patch","skill_id":"同父代","description":"...","content":"...","old_string":"...","new_string":"...","rationale":"融合了哪些优点"}]}
规则：只产出 1 个子代；kind 与 skill_id 同父代；不要臆造事实。"#;

fn describe_variant(label: &str, c: &SkillCandidate) -> String {
    let mut s = format!("### 父代 {label}\n");
    match c.kind {
        CandidateKind::NewSkill => {
            s.push_str("kind: new_skill\ncontent:\n");
            s.push_str(c.content.as_deref().unwrap_or(""));
            s.push('\n');
        }
        CandidateKind::Patch => {
            s.push_str(&format!(
                "kind: patch\nold_string: {}\nnew_string: {}\n",
                c.old_string.as_deref().unwrap_or(""),
                c.new_string.as_deref().unwrap_or("")
            ));
        }
    }
    if let Some(sc) = c.judge_score {
        s.push_str(&format!("（评分 {sc:.2}）\n"));
    }
    s
}

/// 构造交叉 user 提示（两个父代）。
pub fn build_crossover_prompt(a: &SkillCandidate, b: &SkillCandidate) -> String {
    let mut s = format!("对技能 `{}` 做交叉，融合两个父代为一个更优子代。\n\n", a.skill_id);
    s.push_str(&describe_variant("A", a));
    s.push('\n');
    s.push_str(&describe_variant("B", b));
    s.push_str("\n请输出恰好 1 个子代（JSON）。");
    s
}

/// 解析变异输出为候选（沿用 reflection 的健壮解析，强制 kind/skill_id 与种子一致）。
pub fn parse_variants(raw: &str, seed: &SkillCandidate) -> anyhow::Result<Vec<SkillCandidate>> {
    // 复用 reflect 的解析：把 {"variants":[...]} 归一化成 {"candidates":[...]} 再解析。
    let normalized = raw.replacen("\"variants\"", "\"candidates\"", 1);
    let mut cands = crate::reflect::parse_candidates(&normalized)?;
    // 强制与种子同 skill_id / kind，过滤跑偏的。
    cands.retain(|c| c.skill_id == seed.skill_id && c.kind == seed.kind);
    Ok(cands)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(skill: &str, content: &str) -> SkillCandidate {
        SkillCandidate {
            id: uuid::Uuid::new_v4().to_string(),
            kind: CandidateKind::NewSkill,
            skill_id: skill.into(),
            description: Some("d".into()),
            content: Some(content.into()),
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
    fn pareto_drops_dominated() {
        // A: score 0.9 size 100; B: score 0.8 size 200 (被 A 支配); C: score 0.7 size 50 (不被支配)
        let vs = vec![
            ScoredVariant {
                candidate: cand("s", "x"),
                score: 0.9,
                size: 100,
            },
            ScoredVariant {
                candidate: cand("s", "x"),
                score: 0.8,
                size: 200,
            },
            ScoredVariant {
                candidate: cand("s", "x"),
                score: 0.7,
                size: 50,
            },
        ];
        let front = pareto_front(&vs);
        assert_eq!(front.len(), 2);
        assert!(front.iter().any(|v| (v.score - 0.9).abs() < 1e-6));
        assert!(front.iter().any(|v| v.size == 50));
    }

    #[test]
    fn select_front_capped_orders_by_score() {
        let front = vec![
            ScoredVariant {
                candidate: cand("s", "x"),
                score: 0.7,
                size: 50,
            },
            ScoredVariant {
                candidate: cand("s", "x"),
                score: 0.9,
                size: 100,
            },
        ];
        let top = select_front_capped(front, 1);
        assert_eq!(top.len(), 1);
        assert!((top[0].score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn parse_variants_filters_mismatched() {
        let seed = cand("pdf-merge", "seed");
        let raw = r#"{"variants":[
          {"kind":"new_skill","skill_id":"pdf-merge","content":"v1 body","rationale":"clearer"},
          {"kind":"new_skill","skill_id":"other-skill","content":"nope"},
          {"kind":"patch","skill_id":"pdf-merge","old_string":"a","new_string":"b"}
        ]}"#;
        let out = parse_variants(raw, &seed).unwrap();
        // 只保留同 skill_id 且同 kind(new_skill) 的一条
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].skill_id, "pdf-merge");
        assert_eq!(out[0].kind, CandidateKind::NewSkill);
    }

    #[test]
    fn crossover_prompt_shows_both_parents() {
        let a = cand("pdf", "AAA body");
        let b = cand("pdf", "BBB body");
        let p = build_crossover_prompt(&a, &b);
        assert!(p.contains("父代 A"));
        assert!(p.contains("父代 B"));
        assert!(p.contains("AAA body"));
        assert!(p.contains("BBB body"));
        assert!(p.contains("pdf"));
    }

    #[test]
    fn scored_variant_size_from_payload() {
        let v = ScoredVariant::new(cand("s", "hello"), 0.5);
        assert_eq!(v.size, 5);
    }
}
