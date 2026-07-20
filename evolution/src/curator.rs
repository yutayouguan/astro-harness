//! 技能库策展（Curator）：结构化健康报告，只建议不自动删改。
//!
//! 在 `skills::usage` 闲置检测之上叠加进化历史 / 评测集启发式，写入
//! `learning/evolution/curator-last.json` 供 UI 展示。

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
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
pub fn find_overlap_clusters(
    skills: &[(String, String)],
    threshold: f32,
) -> Vec<Vec<String>> {
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

    for i in 0..n {
        for j in (i + 1)..n {
            if jaccard(&tokenized[i].1, &tokenized[j].1) >= threshold {
                union(&mut parent, i, j);
            }
        }
    }

    let mut clusters: std::collections::HashMap<usize, Vec<String>> =
        std::collections::HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        clusters
            .entry(root)
            .or_default()
            .push(tokenized[i].0.clone());
    }
    clusters
        .into_values()
        .filter(|c| c.len() >= 2)
        .collect()
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
                        .then(health_a.partial_cmp(&health_b).unwrap_or(std::cmp::Ordering::Equal))
                        .then(b.cmp(a))
                })?
                .clone();
            let absorb: Vec<String> = cluster
                .iter()
                .filter(|id| **id != keep)
                .cloned()
                .collect();
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
}
