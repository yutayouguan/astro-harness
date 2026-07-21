use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{bail, Result};

use crate::model::{NodeType, WorkflowEdge, WorkflowNode};

/// DAG 执行计划：按拓扑序分层，同一层内节点可并行
#[derive(Debug, Clone)]
pub struct DagPlan {
    /// 执行层级：每层包含可并行执行的节点 id
    pub layers: Vec<Vec<String>>,
    /// 触发器节点 id（入口）
    pub trigger_node_id: Option<String>,
}

/// 对工作流 DAG 做拓扑排序，返回分层执行计划。
///
/// 禁用的节点及其下游自动跳过。含环则报错。
pub fn resolve_dag(nodes: &[WorkflowNode], edges: &[WorkflowEdge]) -> Result<DagPlan> {
    let enabled: HashSet<&str> = nodes
        .iter()
        .filter(|n| !n.disabled)
        .map(|n| n.id.as_str())
        .collect();

    // 只保留两端都启用的边
    let live_edges: Vec<&WorkflowEdge> = edges
        .iter()
        .filter(|e| enabled.contains(e.source.as_str()) && enabled.contains(e.target.as_str()))
        .collect();

    // 邻接表 + 入度
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_deg: HashMap<&str, usize> = HashMap::new();
    for id in &enabled {
        adj.entry(id).or_default();
        in_deg.entry(id).or_insert(0);
    }
    for e in &live_edges {
        adj.entry(e.source.as_str()).or_default().push(e.target.as_str());
        *in_deg.entry(e.target.as_str()).or_insert(0) += 1;
    }

    // Kahn 拓扑排序
    let mut queue: VecDeque<&str> = in_deg
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&id, _)| id)
        .collect();

    let mut layers: Vec<Vec<String>> = Vec::new();
    let mut visited = 0usize;

    while !queue.is_empty() {
        let layer: Vec<&str> = queue.drain(..).collect();
        visited += layer.len();
        let mut next_queue = Vec::new();
        for &node_id in &layer {
            if let Some(neighbors) = adj.get(node_id) {
                for &nb in neighbors {
                    let deg = in_deg.get_mut(nb).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        next_queue.push(nb);
                    }
                }
            }
        }
        layers.push(layer.into_iter().map(String::from).collect());
        queue.extend(next_queue);
    }

    if visited != enabled.len() {
        bail!("工作流中存在环路，无法执行");
    }

    // 找触发器节点
    let trigger_node_id = nodes
        .iter()
        .find(|n| {
            !n.disabled
                && matches!(
                    n.node_type,
                    NodeType::ManualTrigger | NodeType::ScheduledTrigger | NodeType::WebhookTrigger
                )
        })
        .map(|n| n.id.clone());

    Ok(DagPlan {
        layers,
        trigger_node_id,
    })
}

/// 检测环路，返回 Some(环中节点 id 列表) 或 None
pub fn detect_cycle(nodes: &[WorkflowNode], edges: &[WorkflowEdge]) -> Option<Vec<String>> {
    match resolve_dag(nodes, edges) {
        Ok(_) => None,
        Err(_) => {
            // 简易返回：参与环的节点（入度 > 0 且未被拓扑排序消耗）
            let enabled: HashSet<&str> = nodes
                .iter()
                .filter(|n| !n.disabled)
                .map(|n| n.id.as_str())
                .collect();
            let mut in_deg: HashMap<&str, usize> = enabled.iter().map(|&id| (id, 0)).collect();
            for e in edges {
                if enabled.contains(e.source.as_str()) && enabled.contains(e.target.as_str()) {
                    *in_deg.entry(e.target.as_str()).or_insert(0) += 1;
                }
            }
            // BFS 剥离入度=0 的节点
            let mut queue: VecDeque<&str> = in_deg
                .iter()
                .filter(|(_, &d)| d == 0)
                .map(|(&id, _)| id)
                .collect();
            let mut removed = HashSet::new();
            while let Some(id) = queue.pop_front() {
                removed.insert(id);
                for e in edges {
                    if e.source == id && enabled.contains(e.target.as_str()) {
                        let deg = in_deg.get_mut(e.target.as_str()).unwrap();
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(e.target.as_str());
                        }
                    }
                }
            }
            let cycle_nodes: Vec<String> = enabled
                .iter()
                .filter(|id| !removed.contains(*id))
                .map(|id| id.to_string())
                .collect();
            if cycle_nodes.is_empty() {
                None
            } else {
                Some(cycle_nodes)
            }
        }
    }
}

/// 获取指定节点的所有直接上游节点 id
pub fn upstream_nodes(node_id: &str, edges: &[WorkflowEdge]) -> Vec<String> {
    edges
        .iter()
        .filter(|e| e.target == node_id)
        .map(|e| e.source.clone())
        .collect()
}

/// 获取从指定条件/分支节点出发、特定 handle 可达的下游节点 id
pub fn downstream_from_handle(
    node_id: &str,
    handle: Option<&str>,
    edges: &[WorkflowEdge],
) -> Vec<String> {
    edges
        .iter()
        .filter(|e| e.source == node_id && e.source_handle.as_deref() == handle)
        .map(|e| e.target.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Position;

    fn node(id: &str, nt: NodeType) -> WorkflowNode {
        WorkflowNode {
            id: id.into(),
            node_type: nt,
            label: id.into(),
            position: Position { x: 0.0, y: 0.0 },
            config: serde_json::json!({}),
            disabled: false,
        }
    }

    fn edge(src: &str, tgt: &str) -> WorkflowEdge {
        WorkflowEdge {
            id: format!("{src}->{tgt}"),
            source: src.into(),
            source_handle: None,
            target: tgt.into(),
            target_handle: None,
        }
    }

    #[test]
    fn linear_dag() {
        let nodes = vec![
            node("t", NodeType::ManualTrigger),
            node("a", NodeType::SetFields),
            node("b", NodeType::Output),
        ];
        let edges = vec![edge("t", "a"), edge("a", "b")];
        let plan = resolve_dag(&nodes, &edges).unwrap();
        assert_eq!(plan.layers.len(), 3);
        assert_eq!(plan.trigger_node_id.as_deref(), Some("t"));
    }

    #[test]
    fn parallel_branches() {
        let nodes = vec![
            node("t", NodeType::ManualTrigger),
            node("a", NodeType::SetFields),
            node("b", NodeType::FormatText),
            node("m", NodeType::Merge),
        ];
        let edges = vec![edge("t", "a"), edge("t", "b"), edge("a", "m"), edge("b", "m")];
        let plan = resolve_dag(&nodes, &edges).unwrap();
        // t → [a, b] → m
        assert_eq!(plan.layers.len(), 3);
        assert_eq!(plan.layers[1].len(), 2);
    }

    #[test]
    fn cycle_detected() {
        let nodes = vec![
            node("a", NodeType::SetFields),
            node("b", NodeType::SetFields),
        ];
        let edges = vec![edge("a", "b"), edge("b", "a")];
        assert!(resolve_dag(&nodes, &edges).is_err());
        let cycle = detect_cycle(&nodes, &edges).unwrap();
        assert_eq!(cycle.len(), 2);
    }

    #[test]
    fn disabled_nodes_skipped() {
        let mut nodes = vec![
            node("t", NodeType::ManualTrigger),
            node("a", NodeType::SetFields),
            node("b", NodeType::Output),
        ];
        nodes[1].disabled = true;
        let edges = vec![edge("t", "a"), edge("a", "b")];
        let plan = resolve_dag(&nodes, &edges).unwrap();
        // a is disabled, so b has no incoming live edge → t and b are independent
        assert_eq!(plan.layers.len(), 1);
        assert_eq!(plan.layers[0].len(), 2); // t and b both have in_deg=0
    }
}
