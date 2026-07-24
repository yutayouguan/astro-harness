//! 技能库策展（Curator）：结构化健康报告，只建议不自动删改。
//!
//! 在 `skills::usage` 闲置检测之上叠加进化历史 / 评测集启发式，写入
//! `learning/evolution/curator-last.json` 供 UI 展示。

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::evalset::{list_examples, Verdict};
use crate::history::{list_all, HistoryEvent};

/// 策展建议（人审前仅展示，不自动执行）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurateSuggestion {
    Disable {
        skill_id: String,
        reason: String,
    },
    Merge {
        keep: String,
        absorb: Vec<String>,
        reason: String,
    },
    Rewrite {
        skill_id: String,
        reason: String,
    },
}

/// 单技能策展行。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurateSkillRow {
    pub skill_id: String,
    pub description: String,
    pub last_loaded: Option<String>,
    pub stale: bool,
    /// 0–1；信号不足时为 None（不伪造中性分）。
    pub health_score: Option<f32>,
    pub health_reasons: Vec<String>,
    pub bytes: Option<usize>,
}

/// 结构化策展报告。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurateReport {
    pub generated_at: String,
    pub unused_skill_days: u32,
    pub enabled_count: usize,
    pub stale: Vec<String>,
    pub rows: Vec<CurateSkillRow>,
    /// 重叠簇：同一簇内 skill_id 列表（描述/名称相似度高）。
    #[serde(default)]
    pub overlap_clusters: Vec<Vec<String>>,
    pub suggestions: Vec<CurateSuggestion>,
}

/// `{base}/learning/evolution/curator-last.json`
pub fn curator_last_path(base: &Path) -> PathBuf {
    base.join("learning")
        .join("evolution")
        .join("curator-last.json")
}

fn skill_bytes(skill_id: &str) -> Option<usize> {
    skills::load_skill_by_name(skill_id)
        .ok()
        .map(|s| s.content.len())
}

fn outcome_counts(base: &Path, skill_id: &str) -> (usize, usize) {
    let mut approved = 0usize;
    let mut rejected = 0usize;
    for ev in list_all(base) {
        if let HistoryEvent::Outcome {
            skill_id: sid,
            outcome,
            ..
        } = ev
        {
            if sid != skill_id {
                continue;
            }
            match outcome.as_str() {
                "approved" | "branch" => approved += 1,
                "rejected" => rejected += 1,
                _ => {}
            }
        }
    }
    (approved, rejected)
}

fn eval_fail_ratio(base: &Path, skill_id: &str) -> Option<(usize, usize)> {
    let matched: Vec<_> = list_examples(base)
        .into_iter()
        .filter(|e| e.skill_id.as_deref() == Some(skill_id))
        .collect();
    if matched.is_empty() {
        return None;
    }
    let fails = matched
        .iter()
        .filter(|e| e.verdict == Verdict::Fail)
        .count();
    Some((fails, matched.len()))
}

// ---------------------------------------------------------------------------
// 重叠检测（token Jaccard）
// ---------------------------------------------------------------------------

fn tokenize(text: &str) -> std::collections::HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .filter(|t| t.len() >= 2)
        .map(String::from)
        .collect()
}

fn jaccard(a: &std::collections::HashSet<String>, b: &std::collections::HashSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count();
    let union = a.union(b).count();
    inter as f32 / union as f32
}

/// 根据名称+描述的 token Jaccard 相似度发现重叠簇。
///
/// `threshold`：相似度阈值（建议 0.5），达到则视为重叠对。
/// 使用单链接聚合（任意两成员相似即合并），返回 ≥2 个成员的簇。
pub fn find_overlap_clusters(skills: &[(String, String)], threshold: f32) -> Vec<Vec<String>> {
    let tokenized: Vec<(String, std::collections::HashSet<String>)> = skills
        .iter()
        .map(|(id, desc)| {
            let text = format!("{id} {desc}");
            (id.clone(), tokenize(&text))
        })
        .collect();

    // Union-Find for single-linkage clustering
    let n = tokenized.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[rb] = ra;
        }
    }

    for (i, (_, ti)) in tokenized.iter().enumerate() {
        for (j, (_, tj)) in tokenized.iter().enumerate().skip(i + 1) {
            if jaccard(ti, tj) >= threshold {
                union(&mut parent, i, j);
            }
        }
    }

    let mut clusters: std::collections::HashMap<usize, Vec<String>> =
        std::collections::HashMap::new();
    for (i, (id, _)) in tokenized.iter().enumerate() {
        let root = find(&mut parent, i);
        clusters.entry(root).or_default().push(id.clone());
    }
    clusters.into_values().filter(|c| c.len() >= 2).collect()
}

/// 为重叠簇生成 `Merge` 建议。
///
/// `keep`：簇内最近加载或 health 最高的技能，其余为 `absorb`。
fn merge_suggestions_for_clusters(
    clusters: &[Vec<String>],
    rows: &[CurateSkillRow],
) -> Vec<CurateSuggestion> {
    clusters
        .iter()
        .filter_map(|cluster| {
            if cluster.len() < 2 {
                return None;
            }
            // 选择 keep：非 stale > stale，然后 health_score 更高，然后字母序
            let keep = cluster
                .iter()
                .max_by(|a, b| {
                    let ra = rows.iter().find(|r| &r.skill_id == *a);
                    let rb = rows.iter().find(|r| &r.skill_id == *b);
                    let stale_a = ra.map(|r| r.stale).unwrap_or(true);
                    let stale_b = rb.map(|r| r.stale).unwrap_or(true);
                    let health_a = ra.and_then(|r| r.health_score).unwrap_or(0.0);
                    let health_b = rb.and_then(|r| r.health_score).unwrap_or(0.0);
                    // non-stale first
                    stale_b
                        .cmp(&stale_a)
                        .then(
                            health_a
                                .partial_cmp(&health_b)
                                .unwrap_or(std::cmp::Ordering::Equal),
                        )
                        .then(b.cmp(a))
                })?
                .clone();
            let absorb: Vec<String> = cluster.iter().filter(|id| **id != keep).cloned().collect();
            Some(CurateSuggestion::Merge {
                keep: keep.clone(),
                absorb,
                reason: format!("描述重叠，建议合并到 {keep}"),
            })
        })
        .collect()
}

/// 生成结构化策展报告（不修改技能文件）。
pub fn run_curator(base: &Path, unused_skill_days: u32) -> CurateReport {
    let enabled = skills::list_enabled_for_prompt();
    run_curator_with_skills(base, unused_skill_days, &enabled)
}

/// 指定启用技能列表的策展（测试 / 注入用）。
pub fn run_curator_with_skills(
    base: &Path,
    unused_skill_days: u32,
    enabled: &[(String, String)],
) -> CurateReport {
    let cutoff = Utc::now() - chrono::Duration::days(unused_skill_days as i64);
    let mut rows = Vec::new();
    let mut stale = Vec::new();
    let mut suggestions = Vec::new();

    for (name, desc) in enabled {
        let last = skills::last_loaded_at(base, name);
        let last_str = last.map(|t| t.to_rfc3339());
        let is_stale = match last {
            None => true,
            Some(t) => t < cutoff,
        };
        if is_stale {
            stale.push(name.clone());
            suggestions.push(CurateSuggestion::Disable {
                skill_id: name.clone(),
                reason: if last.is_none() {
                    format!("从未加载；闲置阈值 {unused_skill_days} 天")
                } else {
                    format!("超过 {unused_skill_days} 天未加载")
                },
            });
        }

        let mut reasons = Vec::new();
        let mut score: Option<f32> = if is_stale {
            reasons.push("闲置".into());
            Some(0.35)
        } else {
            reasons.push("近期有加载".into());
            Some(0.75)
        };

        let bytes = skill_bytes(name);
        if let Some(b) = bytes {
            if b > 15_360 {
                let s = score.get_or_insert(0.7);
                *s = (*s - 0.1).clamp(0.0, 1.0);
                reasons.push(format!("体积偏大 ({b} 字节)"));
            }
        }

        let (approved, rejected) = outcome_counts(base, name);
        if approved + rejected > 0 {
            let total = approved + rejected;
            let s = score.get_or_insert(0.6);
            if approved > rejected {
                *s = (*s + 0.1).clamp(0.0, 1.0);
                reasons.push(format!("进化采纳 {approved}/{total}"));
            } else if rejected > approved {
                *s = (*s - 0.15).clamp(0.0, 1.0);
                reasons.push(format!("进化拒绝偏多 {rejected}/{total}"));
                if !is_stale {
                    suggestions.push(CurateSuggestion::Rewrite {
                        skill_id: name.clone(),
                        reason: "近期进化提案多次被拒，建议人工改写或定向遗传搜索".into(),
                    });
                }
            }
        }

        if let Some((fails, total)) = eval_fail_ratio(base, name) {
            let ratio = fails as f32 / total as f32;
            let s = score.get_or_insert(0.6);
            if ratio >= 0.5 {
                *s = (*s - 0.2).clamp(0.0, 1.0);
                reasons.push(format!("评测 Fail 占比偏高 ({fails}/{total})"));
                if !is_stale {
                    suggestions.push(CurateSuggestion::Rewrite {
                        skill_id: name.clone(),
                        reason: format!("评测集 Fail {fails}/{total}，建议定向进化"),
                    });
                }
            } else {
                *s = (*s + 0.05).clamp(0.0, 1.0);
                reasons.push(format!("评测 Fail {fails}/{total}"));
            }
        }

        // 无任何增强信号且未 stale 时，若仅靠「近期加载」也保留分数；
        // 完全无信号（无启用列表外场景）不伪造 —— 这里 enabled 行总有闲置/加载信号。
        rows.push(CurateSkillRow {
            skill_id: name.clone(),
            description: desc.clone(),
            last_loaded: last_str,
            stale: is_stale,
            health_score: score,
            health_reasons: reasons,
            bytes,
        });
    }

    // 重叠检测（描述+名称 token Jaccard ≥ 0.5）
    let overlap_clusters = find_overlap_clusters(enabled, 0.5);
    let merge_sugs = merge_suggestions_for_clusters(&overlap_clusters, &rows);
    suggestions.extend(merge_sugs);

    // 去重（同一 skill 可能被多信号触发）
    let mut seen = std::collections::HashSet::new();
    suggestions.retain(|s| {
        let key = match s {
            CurateSuggestion::Disable { skill_id, .. } => format!("disable:{skill_id}"),
            CurateSuggestion::Merge { keep, .. } => format!("merge:{keep}"),
            CurateSuggestion::Rewrite { skill_id, .. } => format!("rewrite:{skill_id}"),
        };
        seen.insert(key)
    });

    CurateReport {
        generated_at: Utc::now().to_rfc3339(),
        unused_skill_days,
        enabled_count: enabled.len(),
        stale,
        rows,
        overlap_clusters,
        suggestions,
    }
}

// ---------------------------------------------------------------------------
// LLM 辅助诊断（提示词 + 解析，调用方注入模型）
// ---------------------------------------------------------------------------

/// LLM 策展诊断的 system 指令。
pub const CURATOR_DIAGNOSE_SYSTEM_PROMPT: &str = r#"你是技能库健康顾问。给定一个技能的当前状态（描述、健康信号、建议类型），用**一句话**给出可操作的诊断。只输出 JSON（不要 markdown 围栏）：
{"diagnosis":"一句可操作的改进建议"}
规则：不超过 80 字；不要复述已知信号；直接说该做什么。"#;

/// 构造单条建议的 LLM 诊断 user prompt。
pub fn build_diagnose_prompt(suggestion: &CurateSuggestion, rows: &[CurateSkillRow]) -> String {
    let mut s = String::new();
    match suggestion {
        CurateSuggestion::Disable { skill_id, reason } => {
            s.push_str(&format!("## 建议：禁用 `{skill_id}`\n理由: {reason}\n"));
            if let Some(row) = rows.iter().find(|r| r.skill_id == *skill_id) {
                s.push_str(&format!("描述: {}\n", row.description));
                for r in &row.health_reasons {
                    s.push_str(&format!("- {r}\n"));
                }
            }
        }
        CurateSuggestion::Merge {
            keep,
            absorb,
            reason,
        } => {
            s.push_str(&format!(
                "## 建议：合并到 `{keep}`，吸收 {}\n理由: {reason}\n",
                absorb.join(", ")
            ));
            for id in std::iter::once(keep.as_str()).chain(absorb.iter().map(|s| s.as_str())) {
                if let Some(row) = rows.iter().find(|r| r.skill_id == id) {
                    s.push_str(&format!("- {id}: {}\n", row.description));
                }
            }
        }
        CurateSuggestion::Rewrite { skill_id, reason } => {
            s.push_str(&format!("## 建议：改写 `{skill_id}`\n理由: {reason}\n"));
            if let Some(row) = rows.iter().find(|r| r.skill_id == *skill_id) {
                s.push_str(&format!("描述: {}\n", row.description));
                for r in &row.health_reasons {
                    s.push_str(&format!("- {r}\n"));
                }
            }
        }
    }
    s.push_str("\n请用一句话给出可操作的诊断（JSON）。");
    s
}

/// 解析 LLM 诊断输出。
pub fn parse_diagnose_output(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(default)]
        diagnosis: String,
    }
    let parsed: Raw = serde_json::from_str(&trimmed[start..=end]).ok()?;
    let d = parsed.diagnosis.trim().to_string();
    if d.is_empty() {
        None
    } else {
        Some(d)
    }
}

/// 将 LLM 诊断回填到 suggestions（替换 reason，保留原因作前缀）。
pub fn apply_diagnoses(suggestions: &mut [CurateSuggestion], diagnoses: &[(usize, String)]) {
    for (idx, diagnosis) in diagnoses {
        if let Some(sug) = suggestions.get_mut(*idx) {
            let reason = match sug {
                CurateSuggestion::Disable { reason, .. } => reason,
                CurateSuggestion::Merge { reason, .. } => reason,
                CurateSuggestion::Rewrite { reason, .. } => reason,
            };
            *reason = format!("{diagnosis}（{reason}）");
        }
    }
}

// ---------------------------------------------------------------------------
// 周期到期（仅报告，不自动入队）
// ---------------------------------------------------------------------------

/// 策展周期门禁结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CuratorDue {
    /// `evolution.curator.enabled` 关闭。
    Disabled,
    /// 距上次报告未满 `interval_days`。
    NotDue { days_since: u32, interval_days: u32 },
    /// 到期：从未跑过，或已超过间隔。
    Due {
        /// 距上次报告天数；从未跑过为 `None`。
        days_since: Option<u32>,
        reason: CuratorDueReason,
    },
}

/// 到期原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CuratorDueReason {
    NeverRan,
    IntervalElapsed,
}

impl CuratorDueReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NeverRan => "never_ran",
            Self::IntervalElapsed => "interval_elapsed",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::NeverRan => "尚未运行过策展",
            Self::IntervalElapsed => "已超过策展间隔",
        }
    }
}

impl CuratorDue {
    pub fn is_due(&self) -> bool {
        matches!(self, Self::Due { .. })
    }
}

/// 策展调度状态（供 UI / Tauri）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratorStatus {
    pub enabled: bool,
    pub interval_days: u32,
    pub due: bool,
    pub days_since_last: Option<u32>,
    pub last_generated_at: Option<String>,
    pub suggestion_count: usize,
    pub skip_reason: Option<String>,
    pub skip_message: Option<String>,
}

/// 解析报告 `generated_at`（RFC3339）；失败视为无有效时间。
pub fn parse_generated_at(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw.trim())
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// 评估是否应自动跑一次策展报告（**不**决定入队）。
pub fn evaluate_curator_due(
    enabled: bool,
    interval_days: u32,
    last_generated_at: Option<&str>,
    now: DateTime<Utc>,
) -> CuratorDue {
    if !enabled {
        return CuratorDue::Disabled;
    }
    let interval = interval_days.max(1);
    let Some(raw) = last_generated_at.filter(|s| !s.trim().is_empty()) else {
        return CuratorDue::Due {
            days_since: None,
            reason: CuratorDueReason::NeverRan,
        };
    };
    let Some(last) = parse_generated_at(raw) else {
        // 时间戳损坏：视为需重跑，避免卡死。
        return CuratorDue::Due {
            days_since: None,
            reason: CuratorDueReason::NeverRan,
        };
    };
    let elapsed = now.signed_duration_since(last);
    let days_since = elapsed.num_days().max(0) as u32;
    if days_since >= interval {
        CuratorDue::Due {
            days_since: Some(days_since),
            reason: CuratorDueReason::IntervalElapsed,
        }
    } else {
        CuratorDue::NotDue {
            days_since,
            interval_days: interval,
        }
    }
}

/// 组装策展调度状态。
pub fn build_curator_status(
    enabled: bool,
    interval_days: u32,
    last: Option<&CurateReport>,
    now: DateTime<Utc>,
) -> CuratorStatus {
    let last_generated_at = last.map(|r| r.generated_at.clone());
    let suggestion_count = last.map(|r| r.suggestions.len()).unwrap_or(0);
    let due = evaluate_curator_due(enabled, interval_days, last_generated_at.as_deref(), now);
    match due {
        CuratorDue::Disabled => CuratorStatus {
            enabled: false,
            interval_days,
            due: false,
            days_since_last: None,
            last_generated_at,
            suggestion_count,
            skip_reason: Some("disabled".into()),
            skip_message: Some("策展提醒未开启".into()),
        },
        CuratorDue::NotDue {
            days_since,
            interval_days: interval,
        } => CuratorStatus {
            enabled: true,
            interval_days: interval,
            due: false,
            days_since_last: Some(days_since),
            last_generated_at,
            suggestion_count,
            skip_reason: Some("not_due".into()),
            skip_message: Some(format!("距上次 {days_since}/{interval} 天，尚未到期")),
        },
        CuratorDue::Due { days_since, reason } => CuratorStatus {
            enabled: true,
            interval_days,
            due: true,
            days_since_last: days_since,
            last_generated_at,
            suggestion_count,
            skip_reason: Some(reason.as_str().into()),
            skip_message: Some(reason.message().into()),
        },
    }
}

/// 运行策展并落盘上次报告。
pub fn run_curator_and_save(base: &Path, unused_skill_days: u32) -> anyhow::Result<CurateReport> {
    let report = run_curator(base, unused_skill_days);
    let path = curator_last_path(base);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&report)?)?;
    fs::rename(&tmp, &path)?;
    Ok(report)
}

/// 读取上次策展报告（若有）。
pub fn load_curator_last(base: &Path) -> Option<CurateReport> {
    let path = curator_last_path(base);
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 将 Disable / Merge 建议转为待审候选（跳过 Rewrite；上限 `max`）。
pub fn suggestions_to_candidates(
    suggestions: &[CurateSuggestion],
    max: usize,
) -> Vec<crate::candidate::SkillCandidate> {
    use crate::candidate::{CandidateKind, SkillCandidate};
    use uuid::Uuid;

    let mut out = Vec::new();
    for s in suggestions {
        if max > 0 && out.len() >= max {
            break;
        }
        match s {
            CurateSuggestion::Disable { skill_id, reason } => {
                out.push(SkillCandidate {
                    id: Uuid::new_v4().to_string(),
                    kind: CandidateKind::Disable,
                    skill_id: skill_id.clone(),
                    description: None,
                    content: Some(format!("# curator disable\n\n{reason}\n")),
                    old_string: None,
                    new_string: None,
                    rationale: reason.clone(),
                    sources: vec!["curator:disable".into()],
                    judge_score: None,
                    judge_reason: None,
                    created_at: Utc::now().to_rfc3339(),
                });
            }
            CurateSuggestion::Merge {
                keep,
                absorb,
                reason,
            } => {
                let mut sources = vec!["curator:merge".into()];
                for a in absorb {
                    sources.push(format!("absorb:{a}"));
                }
                let note = format!(
                    "## Curator merge\n\n{}  \nAbsorb: {}\n",
                    reason,
                    absorb.join(", ")
                );
                out.push(SkillCandidate {
                    id: Uuid::new_v4().to_string(),
                    kind: CandidateKind::Merge,
                    skill_id: keep.clone(),
                    description: None,
                    content: Some(note),
                    old_string: None,
                    new_string: None,
                    rationale: reason.clone(),
                    sources,
                    judge_score: None,
                    judge_reason: None,
                    created_at: Utc::now().to_rfc3339(),
                });
            }
            CurateSuggestion::Rewrite { .. } => {
                // Rewrite 不自动入队；UI 提供定向进化入口
            }
        }
    }
    out
}

/// 策展后可选入队：返回写入的提案数。
pub fn enqueue_curator_suggestions(
    base: &Path,
    report: &CurateReport,
    max: usize,
) -> anyhow::Result<usize> {
    let cands = suggestions_to_candidates(&report.suggestions, max);
    if cands.is_empty() {
        return Ok(0);
    }
    crate::proposal::save_proposals(base, &cands)?;
    Ok(cands.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn empty_enabled_ok() {
        let dir = TempDir::new().unwrap();
        let report = run_curator_with_skills(dir.path(), 30, &[]);
        assert_eq!(report.enabled_count, 0);
        assert!(report.rows.is_empty());
        assert!(report.suggestions.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let report = run_curator_and_save(dir.path(), 14).unwrap();
        let loaded = load_curator_last(dir.path()).unwrap();
        assert_eq!(loaded.unused_skill_days, 14);
        assert_eq!(loaded.generated_at, report.generated_at);
    }

    #[test]
    fn jaccard_identical_is_one() {
        let a = tokenize("pdf merge tool");
        let b = tokenize("pdf merge tool");
        assert!((jaccard(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn jaccard_disjoint_is_zero() {
        let a = tokenize("pdf merge");
        let b = tokenize("video export");
        assert!((jaccard(&a, &b)).abs() < 1e-6);
    }

    #[test]
    fn overlap_clusters_synonymous() {
        let skills = vec![
            (
                "pdf-merge".into(),
                "merge multiple PDF files into one document".into(),
            ),
            (
                "merge-pdf".into(),
                "merge PDF files together into one".into(),
            ),
            ("video-export".into(), "export video as MP4 file".into()),
        ];
        let clusters = find_overlap_clusters(&skills, 0.4);
        assert_eq!(clusters.len(), 1, "pdf-merge and merge-pdf should cluster");
        let c = &clusters[0];
        assert!(c.contains(&"pdf-merge".to_string()));
        assert!(c.contains(&"merge-pdf".to_string()));
        assert!(!c.contains(&"video-export".to_string()));
    }

    #[test]
    fn overlap_clusters_no_match() {
        let skills = vec![
            ("pdf-merge".into(), "合并 PDF".into()),
            ("video-export".into(), "导出视频".into()),
        ];
        let clusters = find_overlap_clusters(&skills, 0.5);
        assert!(clusters.is_empty());
    }

    #[test]
    fn merge_suggestion_keeps_healthier() {
        let rows = vec![
            CurateSkillRow {
                skill_id: "a".into(),
                description: "desc".into(),
                last_loaded: None,
                stale: true,
                health_score: Some(0.3),
                health_reasons: vec![],
                bytes: None,
            },
            CurateSkillRow {
                skill_id: "b".into(),
                description: "desc".into(),
                last_loaded: Some("2026-07-20".into()),
                stale: false,
                health_score: Some(0.8),
                health_reasons: vec![],
                bytes: None,
            },
        ];
        let clusters = vec![vec!["a".into(), "b".into()]];
        let sugs = merge_suggestions_for_clusters(&clusters, &rows);
        assert_eq!(sugs.len(), 1);
        match &sugs[0] {
            CurateSuggestion::Merge { keep, absorb, .. } => {
                assert_eq!(keep, "b", "should keep non-stale, higher health");
                assert_eq!(absorb, &["a"]);
            }
            other => panic!("expected Merge, got {other:?}"),
        }
    }

    #[test]
    fn suggestions_to_candidates_skips_rewrite() {
        let sugs = vec![
            CurateSuggestion::Disable {
                skill_id: "old".into(),
                reason: "闲置".into(),
            },
            CurateSuggestion::Rewrite {
                skill_id: "x".into(),
                reason: "fail".into(),
            },
            CurateSuggestion::Merge {
                keep: "a".into(),
                absorb: vec!["b".into()],
                reason: "重叠".into(),
            },
        ];
        let cands = suggestions_to_candidates(&sugs, 5);
        assert_eq!(cands.len(), 2);
        assert!(cands
            .iter()
            .any(|c| c.kind == crate::candidate::CandidateKind::Disable));
        assert!(cands
            .iter()
            .any(|c| c.kind == crate::candidate::CandidateKind::Merge));
        let merge = cands
            .iter()
            .find(|c| c.kind == crate::candidate::CandidateKind::Merge)
            .unwrap();
        assert!(merge.sources.iter().any(|s| s == "absorb:b"));
    }

    #[test]
    fn enqueue_respects_max() {
        let dir = TempDir::new().unwrap();
        let report = CurateReport {
            generated_at: "now".into(),
            unused_skill_days: 30,
            enabled_count: 2,
            stale: vec!["a".into()],
            rows: vec![],
            overlap_clusters: vec![],
            suggestions: vec![
                CurateSuggestion::Disable {
                    skill_id: "a".into(),
                    reason: "x".into(),
                },
                CurateSuggestion::Disable {
                    skill_id: "b".into(),
                    reason: "y".into(),
                },
            ],
        };
        let n = enqueue_curator_suggestions(dir.path(), &report, 1).unwrap();
        assert_eq!(n, 1);
        assert_eq!(crate::proposal::list_proposals(dir.path()).len(), 1);
    }

    #[test]
    fn parse_diagnose_output_valid() {
        let raw = r#"{"diagnosis":"建议拆分为两个独立技能，各管一个领域"}"#;
        let d = parse_diagnose_output(raw).unwrap();
        assert!(d.contains("拆分"));
    }

    #[test]
    fn parse_diagnose_output_with_fence() {
        let raw = "```json\n{\"diagnosis\":\"直接删除\"}\n```";
        let d = parse_diagnose_output(raw).unwrap();
        assert_eq!(d, "直接删除");
    }

    #[test]
    fn parse_diagnose_output_empty_returns_none() {
        assert!(parse_diagnose_output(r#"{"diagnosis":""}"#).is_none());
        assert!(parse_diagnose_output("not json").is_none());
    }

    #[test]
    fn apply_diagnoses_replaces_reason() {
        let mut sugs = vec![
            CurateSuggestion::Disable {
                skill_id: "a".into(),
                reason: "闲置 30 天".into(),
            },
            CurateSuggestion::Rewrite {
                skill_id: "b".into(),
                reason: "Fail 多".into(),
            },
        ];
        apply_diagnoses(&mut sugs, &[(0, "直接禁用即可".into())]);
        match &sugs[0] {
            CurateSuggestion::Disable { reason, .. } => {
                assert!(reason.contains("直接禁用即可"));
                assert!(reason.contains("闲置 30 天"));
            }
            _ => panic!("wrong variant"),
        }
        // index 1 untouched
        match &sugs[1] {
            CurateSuggestion::Rewrite { reason, .. } => assert_eq!(reason, "Fail 多"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn build_diagnose_prompt_includes_context() {
        let rows = vec![CurateSkillRow {
            skill_id: "pdf-merge".into(),
            description: "合并 PDF 文件".into(),
            last_loaded: None,
            stale: true,
            health_score: Some(0.3),
            health_reasons: vec!["闲置".into()],
            bytes: Some(5000),
        }];
        let sug = CurateSuggestion::Disable {
            skill_id: "pdf-merge".into(),
            reason: "从未加载".into(),
        };
        let p = build_diagnose_prompt(&sug, &rows);
        assert!(p.contains("pdf-merge"));
        assert!(p.contains("禁用"));
        assert!(p.contains("合并 PDF"));
    }

    #[test]
    fn curator_due_disabled() {
        let now = Utc::now();
        assert_eq!(
            evaluate_curator_due(false, 7, None, now),
            CuratorDue::Disabled
        );
    }

    #[test]
    fn curator_due_never_ran() {
        let now = Utc::now();
        match evaluate_curator_due(true, 7, None, now) {
            CuratorDue::Due {
                days_since: None,
                reason: CuratorDueReason::NeverRan,
            } => {}
            other => panic!("expected never_ran, got {other:?}"),
        }
    }

    #[test]
    fn curator_due_within_interval() {
        let now = DateTime::parse_from_rfc3339("2026-07-20T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let last = "2026-07-18T12:00:00Z";
        match evaluate_curator_due(true, 7, Some(last), now) {
            CuratorDue::NotDue {
                days_since: 2,
                interval_days: 7,
            } => {}
            other => panic!("expected not_due, got {other:?}"),
        }
    }

    #[test]
    fn curator_due_interval_elapsed() {
        let now = DateTime::parse_from_rfc3339("2026-07-20T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let last = "2026-07-10T12:00:00Z";
        match evaluate_curator_due(true, 7, Some(last), now) {
            CuratorDue::Due {
                days_since: Some(10),
                reason: CuratorDueReason::IntervalElapsed,
            } => {}
            other => panic!("expected interval_elapsed, got {other:?}"),
        }
    }

    #[test]
    fn build_curator_status_due_flag() {
        let now = Utc::now();
        let st = build_curator_status(true, 7, None, now);
        assert!(st.due);
        assert_eq!(st.skip_reason.as_deref(), Some("never_ran"));
    }
}
