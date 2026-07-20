//! 进化可观测：记录 run 与提案采纳结果，聚合统计。
//!
//! 追加式 JSONL：`{base}/learning/evolution/history.jsonl`。

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 遗传搜索可复现实验摘要（仅 `mode=search` 时有值）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SearchRunMeta {
    pub generations: u32,
    pub variants: u32,
    pub population_size: u32,
    pub crossover: bool,
    pub budget_limit: u32,
    pub budget_used: u32,
    pub optimize_examples: usize,
    pub holdout_examples: usize,
    pub holdout_enabled: bool,
    pub reflection_model: String,
    pub judge_model: String,
    #[serde(default)]
    pub termination: String,
}

/// 一条历史事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HistoryEvent {
    /// 一次进化运行。
    Run {
        id: String,
        ts: String,
        /// `reflect` | `search` | `dspy`
        mode: String,
        generated: usize,
        gated_out: usize,
        judged_out: usize,
        proposals: usize,
        #[serde(default)]
        search_meta: Option<SearchRunMeta>,
    },
    /// 一条提案的最终去向。
    Outcome {
        id: String,
        ts: String,
        proposal_id: String,
        skill_id: String,
        kind: String,
        #[serde(default)]
        score: Option<f32>,
        /// `approved` | `rejected` | `branch`
        outcome: String,
    },
}

/// `{base}/learning/evolution/history.jsonl`
pub fn history_path(base: &Path) -> PathBuf {
    base.join("learning")
        .join("evolution")
        .join("history.jsonl")
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

/// 记录一次 run（best-effort）。
pub fn record_run(
    base: &Path,
    mode: &str,
    generated: usize,
    gated_out: usize,
    judged_out: usize,
    proposals: usize,
) {
    record_run_meta(
        base,
        mode,
        generated,
        gated_out,
        judged_out,
        proposals,
        None,
    );
}

/// 记录一次 run，可选附带 search 摘要。
pub fn record_run_meta(
    base: &Path,
    mode: &str,
    generated: usize,
    gated_out: usize,
    judged_out: usize,
    proposals: usize,
    search_meta: Option<SearchRunMeta>,
) {
    let ev = HistoryEvent::Run {
        id: new_id(),
        ts: now(),
        mode: mode.to_string(),
        generated,
        gated_out,
        judged_out,
        proposals,
        search_meta,
    };
    let _ = append(base, &ev);
}

/// 记录一条提案去向（best-effort）。
pub fn record_outcome(
    base: &Path,
    proposal_id: &str,
    skill_id: &str,
    kind: &str,
    score: Option<f32>,
    outcome: &str,
) {
    let ev = HistoryEvent::Outcome {
        id: new_id(),
        ts: now(),
        proposal_id: proposal_id.to_string(),
        skill_id: skill_id.to_string(),
        kind: kind.to_string(),
        score,
        outcome: outcome.to_string(),
    };
    let _ = append(base, &ev);
}

fn append(base: &Path, ev: &HistoryEvent) -> anyhow::Result<()> {
    let path = history_path(base);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    serde_json::to_writer(&mut f, ev)?;
    f.write_all(b"\n")?;
    Ok(())
}

/// 读取全部事件（跳过坏行）。
pub fn list_all(base: &Path) -> Vec<HistoryEvent> {
    let Ok(f) = fs::File::open(history_path(base)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let Ok(line) = line else { break };
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Ok(ev) = serde_json::from_str::<HistoryEvent>(t) {
            out.push(ev);
        }
    }
    out
}

/// 聚合统计。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HistorySummary {
    pub total_runs: usize,
    pub runs_by_mode: BTreeMap<String, usize>,
    pub total_generated: usize,
    pub total_proposals: usize,
    pub approved: usize,
    pub rejected: usize,
    pub branched: usize,
    /// 采纳率 =（approved + branch）/（approved + branch + rejected）。
    pub adoption_rate: f32,
    /// 已采纳提案的平均分（approved/branch 且有分）。
    pub avg_adopted_score: f32,
    /// 按时间的采纳分数趋势（最多 30 点）。
    pub score_trend: Vec<f32>,
}

/// 计算聚合统计。
pub fn summarize(base: &Path) -> HistorySummary {
    let events = list_all(base);
    let mut s = HistorySummary::default();
    let mut adopted_scores: Vec<f32> = Vec::new();
    for ev in &events {
        match ev {
            HistoryEvent::Run {
                mode,
                generated,
                proposals,
                ..
            } => {
                s.total_runs += 1;
                *s.runs_by_mode.entry(mode.clone()).or_insert(0) += 1;
                s.total_generated += generated;
                s.total_proposals += proposals;
            }
            HistoryEvent::Outcome { outcome, score, .. } => match outcome.as_str() {
                "approved" => {
                    s.approved += 1;
                    if let Some(sc) = score {
                        adopted_scores.push(*sc);
                    }
                }
                "branch" => {
                    s.branched += 1;
                    if let Some(sc) = score {
                        adopted_scores.push(*sc);
                    }
                }
                "rejected" => s.rejected += 1,
                _ => {}
            },
        }
    }
    let decided = s.approved + s.branched + s.rejected;
    s.adoption_rate = if decided > 0 {
        (s.approved + s.branched) as f32 / decided as f32
    } else {
        0.0
    };
    s.avg_adopted_score = if adopted_scores.is_empty() {
        0.0
    } else {
        adopted_scores.iter().sum::<f32>() / adopted_scores.len() as f32
    };
    // 趋势：所有 outcome 分数按序，末 30 点
    let mut trend: Vec<f32> = events
        .iter()
        .filter_map(|e| match e {
            HistoryEvent::Outcome { score, .. } => *score,
            _ => None,
        })
        .collect();
    if trend.len() > 30 {
        trend = trend.split_off(trend.len() - 30);
    }
    s.score_trend = trend;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn old_run_json_without_search_meta_still_parses() {
        let raw = r#"{"type":"run","id":"1","ts":"t","mode":"search","generated":1,"gated_out":0,"judged_out":0,"proposals":1}"#;
        let ev: HistoryEvent = serde_json::from_str(raw).unwrap();
        match ev {
            HistoryEvent::Run { search_meta, .. } => assert!(search_meta.is_none()),
            _ => panic!("expected run"),
        }
    }

    #[test]
    fn records_and_summarizes() {
        let dir = TempDir::new().unwrap();
        let b = dir.path();
        record_run(b, "search", 5, 2, 1, 2);
        record_run(b, "reflect", 3, 1, 0, 2);
        record_outcome(b, "p1", "s", "new_skill", Some(0.8), "approved");
        record_outcome(b, "p2", "s", "patch", Some(0.6), "branch");
        record_outcome(b, "p3", "s", "new_skill", Some(0.2), "rejected");

        let sum = summarize(b);
        assert_eq!(sum.total_runs, 2);
        assert_eq!(sum.runs_by_mode.get("search"), Some(&1));
        assert_eq!(sum.total_generated, 8);
        assert_eq!(sum.total_proposals, 4);
        assert_eq!(sum.approved, 1);
        assert_eq!(sum.branched, 1);
        assert_eq!(sum.rejected, 1);
        // 采纳率 = 2/3
        assert!((sum.adoption_rate - 2.0 / 3.0).abs() < 1e-4);
        // 平均采纳分 = (0.8+0.6)/2 = 0.7
        assert!((sum.avg_adopted_score - 0.7).abs() < 1e-4);
        assert_eq!(sum.score_trend.len(), 3);
    }

    #[test]
    fn empty_summary_is_zero() {
        let dir = TempDir::new().unwrap();
        let sum = summarize(dir.path());
        assert_eq!(sum.total_runs, 0);
        assert_eq!(sum.adoption_rate, 0.0);
    }
}
