//! 协作洞察：编排列表 + handoff 聚合图。
//!
//! 列表权威来自 `orchestration.db`；图边来自 `usage_events`（`kind=orchestration`，`phase=end`）。
//! 不计入用量 KPI `calls`。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::db::OrchestrationDb;
use usage::{period_window, UsageDb, UsagePeriod};

/// 列表默认条数
pub const COLLAB_LIST_LIMIT: usize = 50;
/// API 层步骤 output 截断（字节）
pub const COLLAB_OUTPUT_MAX_BYTES: usize = 2 * 1024;

/// 协作洞察完整结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationInsights {
    pub orchestrations: Vec<CollaborationOrchestration>,
    pub graph: CollaborationGraph,
}

/// 单条编排（含步骤）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationOrchestration {
    pub id: String,
    pub goal: String,
    pub status: String,
    pub parent_agent_id: String,
    pub session_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
    pub result_summary: Option<String>,
    pub steps: Vec<CollaborationStep>,
}

/// 编排步骤摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationStep {
    pub seq: i64,
    pub role: String,
    pub agent_id: Option<String>,
    pub status: String,
    pub output: Option<String>,
    pub error: Option<String>,
}

/// 聚合图
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollaborationGraph {
    pub nodes: Vec<CollaborationNode>,
    pub edges: Vec<CollaborationEdge>,
}

/// 图节点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationNode {
    pub id: String,
    pub label: String,
    pub kind: String,
}

/// 图边（weight = phase=end 次数）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationEdge {
    pub from: String,
    pub to: String,
    pub weight: i64,
}

/// 协作洞察查询参数
#[derive(Debug, Clone)]
pub struct CollaborationInsightsQuery {
    pub period: UsagePeriod,
    pub as_of: Option<String>,
    pub agent_id: Option<String>,
}

/// 查询协作洞察（列表 + 图）
pub fn query_collaboration_insights(
    q: CollaborationInsightsQuery,
) -> anyhow::Result<CollaborationInsights> {
    let (start, end) = period_window(q.period, q.as_of.as_deref())?;
    let agent = q.agent_id.filter(|s| !s.is_empty());
    let orch_db = OrchestrationDb::open_default()?;
    let rows = orch_db.list_in_period(&start, &end, agent.as_deref(), COLLAB_LIST_LIMIT)?;
    let mut orchestrations = Vec::with_capacity(rows.len());
    for row in rows {
        let steps = orch_db.list_steps(&row.id)?;
        orchestrations.push(CollaborationOrchestration {
            id: row.id,
            goal: row.goal,
            status: row.status,
            parent_agent_id: row.parent_agent_id,
            session_id: row.session_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            finished_at: row.finished_at,
            error: row.error,
            result_summary: row.result_summary,
            steps: steps
                .into_iter()
                .map(|s| CollaborationStep {
                    seq: s.seq,
                    role: s.role,
                    agent_id: s.agent_id,
                    status: s.status,
                    output: s.output.map(|o| truncate_utf8(&o, COLLAB_OUTPUT_MAX_BYTES)),
                    error: s.error,
                })
                .collect(),
        });
    }
    let graph = build_graph_from_usage(&start, &end, agent.as_deref())?;
    Ok(CollaborationInsights {
        orchestrations,
        graph,
    })
}

fn build_graph_from_usage(
    start: &str,
    end: &str,
    agent_id: Option<&str>,
) -> anyhow::Result<CollaborationGraph> {
    let db = UsageDb::open_default()?;
    let metas = db.list_orchestration_meta(start, end)?;
    let mut weights: HashMap<(String, String), i64> = HashMap::new();

    for meta in metas {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&meta) else {
            continue;
        };
        let phase = v.get("phase").and_then(|p| p.as_str()).unwrap_or("");
        if phase != "end" {
            continue;
        }
        let from = match v.get("from").and_then(|x| x.as_str()).map(str::trim) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let to = match v.get("to").and_then(|x| x.as_str()).map(str::trim) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        if let Some(aid) = agent_id {
            if from != aid && to != aid {
                continue;
            }
        }
        *weights.entry((from, to)).or_insert(0) += 1;
    }

    let mut node_ids = std::collections::BTreeSet::new();
    let mut edges: Vec<CollaborationEdge> = weights
        .into_iter()
        .map(|((from, to), weight)| {
            node_ids.insert(from.clone());
            node_ids.insert(to.clone());
            CollaborationEdge { from, to, weight }
        })
        .collect();
    edges.sort_by(|a, b| b.weight.cmp(&a.weight).then(a.from.cmp(&b.from)));

    let nodes: Vec<CollaborationNode> = node_ids
        .into_iter()
        .map(|id| {
            if let Some(rest) = id.strip_prefix("role:") {
                CollaborationNode {
                    id: id.clone(),
                    label: rest.to_string(),
                    kind: "role".into(),
                }
            } else {
                CollaborationNode {
                    label: id.clone(),
                    id,
                    kind: "agent".into(),
                }
            }
        })
        .collect();

    Ok(CollaborationGraph { nodes, edges })
}

fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}
