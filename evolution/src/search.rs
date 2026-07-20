//! GEPA-lite 遗传搜索：变异 + 交叉 + Pareto 选择（judge 分↑ / 体积↓）。
//!
//! 适应度来自 judge 分（含 grounded eval + Fail 加权）与写入体积；
//! 变异/评分模型调用由 Tauri 注入，通过 [`SearchBudget`] 控制总调用上限。

use crate::candidate::{CandidateKind, SkillCandidate};

// ---------------------------------------------------------------------------
// SearchBudget：LLM 调用预算的严格上限
// ---------------------------------------------------------------------------

/// 搜索过程的 LLM 调用预算。`limit == 0` 表示不限。
///
/// 所有消费同一预算的调用方（seed reflection、mutation、crossover、judge、
/// grounded eval）必须在发起 LLM 请求**前**调用 `try_reserve_one()`，
/// 失败则不得发起该次请求。
#[derive(Debug, Clone)]
pub struct SearchBudget {
    limit: u32,
    used: u32,
}

impl SearchBudget {
    pub fn new(limit: u32) -> Self {
        Self { limit, used: 0 }
    }

    /// 预留 `calls` 次调用。预算充足则扣减并返回 true；不足则不扣减并返回 false。
    pub fn try_reserve(&mut self, calls: u32) -> bool {
        if self.limit > 0 && self.used.saturating_add(calls) > self.limit {
            return false;
        }
        self.used = self.used.saturating_add(calls);
        true
    }

    /// 预留 1 次调用（循环内逐次消费的快捷方法）。
    pub fn try_reserve_one(&mut self) -> bool {
        self.try_reserve(1)
    }

    pub fn remaining(&self) -> Option<u32> {
        (self.limit > 0).then(|| self.limit.saturating_sub(self.used))
    }

    pub fn used(&self) -> u32 {
        self.used
    }
}

/// 计算候选的有效体积（post-image 字节数）。
///
/// - `NewSkill`：完整 `content` 长度。
/// - `Patch`：将 `old_string → new_string` 应用到 `current_skill` 后的完整文本长度。
///   `current_skill` 为 `None`、替换未命中或多次命中时返回错误。
pub fn effective_candidate_size(
    candidate: &SkillCandidate,
    current_skill: Option<&str>,
) -> anyhow::Result<usize> {
    match candidate.kind {
        CandidateKind::NewSkill | CandidateKind::Merge => {
            Ok(candidate.content.as_deref().map(str::len).unwrap_or(0))
        }
        CandidateKind::Disable => Ok(1),
        CandidateKind::Patch => {
            let text = current_skill
                .ok_or_else(|| anyhow::anyhow!("patch 需要 current_skill 计算有效体积"))?;
            let old = candidate.old_string.as_deref().unwrap_or("");
            let new = candidate.new_string.as_deref().unwrap_or("");
            let post = crate::proposal::apply_patch_unique(text, old, new)?;
            Ok(post.len())
        }
    }
}

/// 一个被评分的变体。
#[derive(Debug, Clone)]
pub struct ScoredVariant {
    pub candidate: SkillCandidate,
    /// judge 分（0–1，越高越好）。
    pub score: f32,
    /// 有效体积字节（越小越好）。
    pub size: usize,
    /// 测试通过率（0–1，越高越好）；`None` = 该技能无测试脚本，不参与 Pareto 比较。
    pub test_pass: Option<f32>,
}

impl ScoredVariant {
    /// 使用调用方已计算的 `effective_size` 构造（测试不适用）。
    pub fn with_size(candidate: SkillCandidate, score: f32, effective_size: usize) -> Self {
        Self {
            candidate,
            score,
            size: effective_size,
            test_pass: None,
        }
    }

    /// 便捷构造：使用 `payload_len()`（兼容不需要 post-image 体积的场景）。
    pub fn new(candidate: SkillCandidate, score: f32) -> Self {
        let size = candidate.payload_len();
        Self {
            candidate,
            score,
            size,
            test_pass: None,
        }
    }

    /// 设置测试通过率。
    pub fn with_test_pass(mut self, test_pass: Option<f32>) -> Self {
        self.test_pass = test_pass;
        self
    }

    /// 自身是否支配 `other`：所有维度 ≥ 且至少一维严格更优。
    ///
    /// 三维：score↑, test_pass↑, size↓。
    /// `test_pass` 均为 `None` 时该维度不参与比较（退化到二维）。
    fn dominates(&self, other: &ScoredVariant) -> bool {
        let score_ge = self.score >= other.score;
        let size_le = self.size <= other.size;

        let (test_ge, test_strict) = match (self.test_pass, other.test_pass) {
            (Some(a), Some(b)) => (a >= b, a > b),
            (None, None) => (true, false),
            // 一方有测试一方无：不可比较，阻止支配
            _ => (false, false),
        };

        let ge = score_ge && size_le && test_ge;
        let strictly = self.score > other.score || self.size < other.size || test_strict;
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

fn cmp_test_pass(a: Option<f32>, b: Option<f32>) -> std::cmp::Ordering {
    match (b, a) {
        (Some(bv), Some(av)) => bv.partial_cmp(&av).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn sort_scored(v: &mut [ScoredVariant]) {
    v.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| cmp_test_pass(a.test_pass, b.test_pass))
            .then(a.size.cmp(&b.size))
    });
}

/// 从前沿按 score 降序、test_pass 降序、size 升序取前 `n`。
pub fn select_front_capped(mut front: Vec<ScoredVariant>, n: usize) -> Vec<ScoredVariant> {
    sort_scored(&mut front);
    front.truncate(n);
    front
}

/// 候选指纹：用于种群去重（相同 kind + skill_id + 内容 → 同一指纹）。
pub fn candidate_fingerprint(c: &SkillCandidate) -> String {
    match c.kind {
        CandidateKind::NewSkill | CandidateKind::Merge => {
            format!("new:{}:{}", c.skill_id, c.content.as_deref().unwrap_or(""))
        }
        CandidateKind::Patch => {
            format!(
                "patch:{}:{}->{}",
                c.skill_id,
                c.old_string.as_deref().unwrap_or(""),
                c.new_string.as_deref().unwrap_or("")
            )
        }
        CandidateKind::Disable => format!("disable:{}", c.skill_id),
    }
}

/// 从评分变体中选出种群：Pareto 前沿优先，不足时从被支配集按 score 降序
/// （同分取更小 size）补齐。去重后最多保留 `population_size` 个。
pub fn select_population(scored: Vec<ScoredVariant>, population_size: usize) -> Vec<ScoredVariant> {
    if population_size == 0 || scored.is_empty() {
        return Vec::new();
    }
    let front = pareto_front(&scored);
    let mut seen = std::collections::HashSet::new();
    let mut result: Vec<ScoredVariant> = Vec::new();

    // 先从前沿取
    let mut front_sorted = front;
    sort_scored(&mut front_sorted);
    for v in front_sorted {
        let fp = candidate_fingerprint(&v.candidate);
        if seen.insert(fp) {
            result.push(v);
        }
        if result.len() >= population_size {
            return result;
        }
    }

    // 不足时从被支配集合按 score 降序补齐
    let mut rest: Vec<ScoredVariant> = scored
        .into_iter()
        .filter(|v| !seen.contains(&candidate_fingerprint(&v.candidate)))
        .collect();
    sort_scored(&mut rest);
    for v in rest {
        let fp = candidate_fingerprint(&v.candidate);
        if seen.insert(fp) {
            result.push(v);
        }
        if result.len() >= population_size {
            break;
        }
    }
    result
}

/// 变异模型 system 指令：产出多个改进变体，JSON only。
pub const MUTATION_SYSTEM_PROMPT: &str = r#"你是技能进化的变异器。给定一个技能候选（种子）与上一代的评审意见，产出若干**改进后的变体**。只输出 JSON（不要 markdown 围栏）：
{"variants":[{"kind":"new_skill|patch","skill_id":"同种子","description":"...","content":"...","old_string":"...","new_string":"...","rationale":"改了什么、为什么更好"}]}
规则：
- 每个变体针对同一 skill_id 与同一 kind；在种子基础上做有意义的差异化改进（更清晰、更健壮、更简洁、修正评审指出的问题）。
- new_skill 用 content；patch 用 old_string/new_string。
- 不要臆造事实；宁可少而精。"#;

/// 构造变异 user 提示：种子 + 期望变体数 + 结构化 critique（缺口 + 保留项）。
pub fn build_mutation_prompt(
    seed: &SkillCandidate,
    variants: u32,
    critiques: &[String],
    strengths: &[String],
) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "目标：产出 {variants} 个改进变体。\n\n## 种子候选\n"
    ));
    s.push_str(&format!("skill_id: {}\n", seed.skill_id));
    match seed.kind {
        CandidateKind::NewSkill | CandidateKind::Merge => {
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
        CandidateKind::Disable => {
            s.push_str("kind: disable\n");
        }
    }
    if !critiques.is_empty() {
        s.push_str("\n## 必须修复的缺口（上一代评审指出的未满足要点）\n");
        for c in critiques {
            if !c.trim().is_empty() {
                s.push_str(&format!("- {}\n", c.replace('\n', " ")));
            }
        }
    }
    if !strengths.is_empty() {
        s.push_str("\n## 必须保留的已有能力\n");
        for item in strengths {
            if !item.trim().is_empty() {
                s.push_str(&format!("- {}\n", item.replace('\n', " ")));
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
        CandidateKind::NewSkill | CandidateKind::Merge => {
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
        CandidateKind::Disable => {
            s.push_str("kind: disable\n");
        }
    }
    if let Some(sc) = c.judge_score {
        s.push_str(&format!("（评分 {sc:.2}）\n"));
    }
    s
}

/// 构造交叉 user 提示（两个父代）。
pub fn build_crossover_prompt(a: &SkillCandidate, b: &SkillCandidate) -> String {
    let mut s = format!(
        "对技能 `{}` 做交叉，融合两个父代为一个更优子代。\n\n",
        a.skill_id
    );
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
                test_pass: None,
            },
            ScoredVariant {
                candidate: cand("s", "x"),
                score: 0.8,
                size: 200,
                test_pass: None,
            },
            ScoredVariant {
                candidate: cand("s", "x"),
                score: 0.7,
                size: 50,
                test_pass: None,
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
                test_pass: None,
            },
            ScoredVariant {
                candidate: cand("s", "x"),
                score: 0.9,
                size: 100,
                test_pass: None,
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

    #[test]
    fn scored_variant_with_explicit_size() {
        let v = ScoredVariant::with_size(cand("s", "hello"), 0.5, 42);
        assert_eq!(v.size, 42);
    }

    #[test]
    fn effective_size_new_skill_equals_content_len() {
        let c = cand("s", "hello world");
        assert_eq!(effective_candidate_size(&c, None).unwrap(), 11);
    }

    #[test]
    fn effective_size_patch_uses_post_image() {
        let mut c = cand("s", "");
        c.kind = CandidateKind::Patch;
        c.content = None;
        c.old_string = Some("foo".into());
        c.new_string = Some("bar baz".into());
        let current = "prefix foo suffix";
        let result = effective_candidate_size(&c, Some(current)).unwrap();
        // "prefix bar baz suffix" = 21 bytes
        assert_eq!(result, 21);
    }

    #[test]
    fn effective_size_patch_without_skill_errors() {
        let mut c = cand("s", "");
        c.kind = CandidateKind::Patch;
        c.old_string = Some("x".into());
        c.new_string = Some("y".into());
        assert!(effective_candidate_size(&c, None).is_err());
    }

    #[test]
    fn effective_size_patch_no_match_errors() {
        let mut c = cand("s", "");
        c.kind = CandidateKind::Patch;
        c.old_string = Some("not found".into());
        c.new_string = Some("y".into());
        assert!(effective_candidate_size(&c, Some("no match here")).is_err());
    }

    #[test]
    fn select_population_deduplicates() {
        let c1 = cand("s", "hello");
        let c2 = cand("s", "hello"); // same fingerprint
        let c3 = cand("s", "world"); // different
        let scored = vec![
            ScoredVariant::with_size(c1, 0.9, 5),
            ScoredVariant::with_size(c2, 0.8, 5),
            ScoredVariant::with_size(c3, 0.7, 5),
        ];
        let pop = select_population(scored, 3);
        assert_eq!(pop.len(), 2); // deduplicated
    }

    #[test]
    fn select_population_size_one_takes_best() {
        let scored = vec![
            ScoredVariant::with_size(cand("s", "aaa"), 0.6, 3),
            ScoredVariant::with_size(cand("s", "bbb"), 0.9, 3),
        ];
        let pop = select_population(scored, 1);
        assert_eq!(pop.len(), 1);
        assert!((pop[0].score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn select_population_fills_from_dominated() {
        // A dominates B (higher score, same size), C is non-dominated (lower score, smaller size)
        let scored = vec![
            ScoredVariant::with_size(cand("s", "aaa"), 0.9, 100),
            ScoredVariant::with_size(cand("s", "bbb"), 0.8, 200), // dominated
            ScoredVariant::with_size(cand("s", "ccc"), 0.7, 50),  // non-dominated
        ];
        // front = {aaa, ccc}, request 3 → fills bbb from dominated set
        let pop = select_population(scored, 3);
        assert_eq!(pop.len(), 3);
    }

    #[test]
    fn search_budget_never_exceeds_limit() {
        let mut budget = SearchBudget::new(3);
        assert!(budget.try_reserve(1));
        assert!(budget.try_reserve(2));
        assert!(!budget.try_reserve(1));
        assert_eq!(budget.used(), 3);
        assert_eq!(budget.remaining(), Some(0));
    }

    #[test]
    fn zero_budget_means_unlimited() {
        let mut budget = SearchBudget::new(0);
        assert!(budget.try_reserve(10_000));
        assert_eq!(budget.remaining(), None);
    }

    #[test]
    fn search_budget_try_reserve_one() {
        let mut budget = SearchBudget::new(2);
        assert!(budget.try_reserve_one());
        assert!(budget.try_reserve_one());
        assert!(!budget.try_reserve_one());
        assert_eq!(budget.used(), 2);
    }

    #[test]
    fn search_budget_no_partial_reserve() {
        let mut budget = SearchBudget::new(3);
        assert!(budget.try_reserve(2));
        // 剩余 1，请求 2 — 不扣减
        assert!(!budget.try_reserve(2));
        assert_eq!(budget.used(), 2);
        assert_eq!(budget.remaining(), Some(1));
    }

    #[test]
    fn pareto_three_dim_test_pass() {
        // A: score=0.9, size=100, test=1.0
        // B: score=0.9, size=100, test=0.0 — A 支配 B（test 严格更优）
        // C: score=0.7, size=50,  test=1.0 — 不被 A 支配（size 更优）
        let vs = vec![
            ScoredVariant {
                candidate: cand("s", "aaa"),
                score: 0.9,
                size: 100,
                test_pass: Some(1.0),
            },
            ScoredVariant {
                candidate: cand("s", "bbb"),
                score: 0.9,
                size: 100,
                test_pass: Some(0.0),
            },
            ScoredVariant {
                candidate: cand("s", "ccc"),
                score: 0.7,
                size: 50,
                test_pass: Some(1.0),
            },
        ];
        let front = pareto_front(&vs);
        assert_eq!(front.len(), 2);
        assert!(front
            .iter()
            .any(|v| v.candidate.content.as_deref() == Some("aaa")));
        assert!(front
            .iter()
            .any(|v| v.candidate.content.as_deref() == Some("ccc")));
    }

    #[test]
    fn pareto_no_test_degrades_to_two_dim() {
        // 双方都 None → test 维度不参与，退化二维
        let vs = vec![
            ScoredVariant {
                candidate: cand("s", "aaa"),
                score: 0.9,
                size: 100,
                test_pass: None,
            },
            ScoredVariant {
                candidate: cand("s", "bbb"),
                score: 0.7,
                size: 50,
                test_pass: None,
            },
        ];
        let front = pareto_front(&vs);
        // 两者互不支配
        assert_eq!(front.len(), 2);
    }

    #[test]
    fn pareto_mixed_test_none_some_non_comparable() {
        // A: score=0.9, size=50, test=None (untested)
        // B: score=0.8, size=100, test=Some(1.0)
        // A should NOT dominate B because test dimensions are non-comparable
        let vs = vec![
            ScoredVariant {
                candidate: cand("s", "aaa"),
                score: 0.9,
                size: 50,
                test_pass: None,
            },
            ScoredVariant {
                candidate: cand("s", "bbb"),
                score: 0.8,
                size: 100,
                test_pass: Some(1.0),
            },
        ];
        let front = pareto_front(&vs);
        assert_eq!(front.len(), 2, "mixed None/Some should be non-comparable");
    }

    #[test]
    fn select_front_capped_prefers_test_passing() {
        // Same score, same size, but one passes test and the other fails
        let vs = vec![
            ScoredVariant::with_size(cand("s", "fail"), 0.9, 100).with_test_pass(Some(0.0)),
            ScoredVariant::with_size(cand("s", "pass"), 0.9, 100).with_test_pass(Some(1.0)),
        ];
        let top = select_front_capped(vs, 1);
        assert_eq!(top[0].candidate.content.as_deref(), Some("pass"));
    }
}
