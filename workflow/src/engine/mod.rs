pub mod dag;
pub mod executor;
pub mod variables;

use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use chrono::Local;

use crate::model::{NodeType, Workflow, WorkflowEdge};
use crate::nodes;
use crate::run_db::WorkflowRunDb;
use dag::resolve_dag;
use executor::NodeResult;
use variables::VariableContext;

/// 工作流执行结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkflowRunResult {
    pub run_id: String,
    pub status: String,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub steps_executed: usize,
}

/// 执行一条工作流
pub async fn execute_workflow(
    workflow: &Workflow,
    trigger_input: serde_json::Value,
    trigger_type: &str,
    run_db: &WorkflowRunDb,
) -> Result<WorkflowRunResult> {
    let run_id = uuid::Uuid::new_v4().to_string();
    let started_at = Local::now().to_rfc3339();

    run_db.insert_run(
        &run_id,
        &workflow.id,
        &workflow.name,
        trigger_type,
        &started_at,
    )?;

    let result = execute_inner(workflow, trigger_input, &run_id, run_db).await;

    let finished_at = Local::now().to_rfc3339();
    match &result {
        Ok(res) => {
            let output_str = res.output.as_ref().map(|v| {
                let s = serde_json::to_string(v).unwrap_or_default();
                if s.len() > 512_000 { s[..512_000].to_string() } else { s }
            });
            run_db.finish_run(
                &run_id,
                "success",
                &finished_at,
                None,
                output_str.as_deref(),
                res.steps_executed as i64,
            )?;
        }
        Err(e) => {
            run_db.finish_run(&run_id, "failure", &finished_at, Some(&e.to_string()), None, 0)?;
        }
    }

    result
}

async fn execute_inner(
    workflow: &Workflow,
    trigger_input: serde_json::Value,
    run_id: &str,
    run_db: &WorkflowRunDb,
) -> Result<WorkflowRunResult> {
    let plan = resolve_dag(&workflow.nodes, &workflow.edges)?;

    let executors = nodes::build_executor_registry();

    let mut ctx = VariableContext::new(workflow.variables.clone());

    // 将 trigger_input 注入全局变量
    if let serde_json::Value::Object(map) = trigger_input {
        for (k, v) in map {
            ctx.set_node_output("trigger_input", serde_json::json!({}));
            // 存为顶层全局可访问
            let mut merged = ctx.get_node_output("trigger_input")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            if let Some(obj) = merged.as_object_mut() {
                obj.insert(k, v);
            }
            ctx.set_node_output("trigger_input", merged);
        }
    }

    let node_map: HashMap<&str, &crate::model::WorkflowNode> = workflow
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();

    // 跟踪被分支/过滤跳过的节点
    let mut skipped: HashSet<String> = HashSet::new();
    let mut steps_executed = 0usize;
    let mut final_output: Option<serde_json::Value> = None;

    for layer in &plan.layers {
        for node_id in layer {
            if skipped.contains(node_id) {
                continue;
            }

            let node = match node_map.get(node_id.as_str()) {
                Some(n) => *n,
                None => continue,
            };

            let executor = match executors.get(&node.node_type) {
                Some(e) => e,
                None => {
                    tracing::warn!(node_type = ?node.node_type, "无可用执行器，跳过");
                    continue;
                }
            };

            let step_id = uuid::Uuid::new_v4().to_string();
            let step_started = Local::now().to_rfc3339();

            // 记录步骤开始
            let _ = run_db.insert_step_log(
                &step_id,
                run_id,
                node_id,
                &format!("{:?}", node.node_type),
                &node.label,
                &step_started,
            );

            let result = executor.execute(node, &ctx).await;

            let step_finished = Local::now().to_rfc3339();
            steps_executed += 1;

            match result {
                Ok(NodeResult::Success(output)) => {
                    ctx.set_node_output(node_id, output.clone());
                    if node.node_type == NodeType::Output {
                        final_output = Some(output.clone());
                    }
                    let out_str = serde_json::to_string(&output).ok();
                    let _ = run_db.finish_step_log(
                        &step_id,
                        "success",
                        &step_finished,
                        out_str.as_deref(),
                        None,
                    );
                }
                Ok(NodeResult::Branch(active_handles)) => {
                    ctx.set_node_output(
                        node_id,
                        serde_json::json!({ "active_branches": &active_handles }),
                    );
                    // 标记未激活分支的下游节点为 skipped
                    mark_inactive_downstream(
                        node_id,
                        &active_handles,
                        &workflow.edges,
                        &mut skipped,
                    );
                    let _ = run_db.finish_step_log(
                        &step_id,
                        "success",
                        &step_finished,
                        Some(&format!("branches: {:?}", active_handles)),
                        None,
                    );
                }
                Ok(NodeResult::Filtered) => {
                    // 标记所有下游为 skipped
                    mark_all_downstream(node_id, &workflow.edges, &node_map, &mut skipped);
                    let _ = run_db.finish_step_log(
                        &step_id,
                        "skipped",
                        &step_finished,
                        Some("filtered"),
                        None,
                    );
                }
                Ok(NodeResult::Approved) => {
                    ctx.set_node_output(node_id, serde_json::json!({ "approved": true }));
                    let _ = run_db.finish_step_log(
                        &step_id,
                        "success",
                        &step_finished,
                        Some("approved"),
                        None,
                    );
                }
                Err(e) => {
                    let _ = run_db.finish_step_log(
                        &step_id,
                        "failure",
                        &step_finished,
                        None,
                        Some(&e.to_string()),
                    );
                    bail!("节点 {} ({}) 执行失败: {}", node.label, node_id, e);
                }
            }
        }
    }

    Ok(WorkflowRunResult {
        run_id: run_id.to_string(),
        status: "success".to_string(),
        output: final_output,
        error: None,
        steps_executed,
    })
}

/// 标记条件/分支节点中未被激活的输出端口的直接下游节点为 skipped
fn mark_inactive_downstream(
    node_id: &str,
    active_handles: &[String],
    edges: &[WorkflowEdge],
    skipped: &mut HashSet<String>,
) {
    let active_set: HashSet<&str> = active_handles.iter().map(|s| s.as_str()).collect();
    for edge in edges {
        if edge.source == node_id {
            let handle = edge.source_handle.as_deref();
            let is_active = match handle {
                Some(h) => active_set.contains(h),
                None => !active_handles.is_empty(), // 无 handle 的边：如果有任何激活则放行
            };
            if !is_active {
                skipped.insert(edge.target.clone());
            }
        }
    }
}

/// 标记某节点所有下游（递归）为 skipped
fn mark_all_downstream(
    node_id: &str,
    edges: &[WorkflowEdge],
    _node_map: &HashMap<&str, &crate::model::WorkflowNode>,
    skipped: &mut HashSet<String>,
) {
    let mut queue = vec![node_id.to_string()];
    while let Some(current) = queue.pop() {
        for edge in edges {
            if edge.source == current && !skipped.contains(&edge.target) {
                skipped.insert(edge.target.clone());
                queue.push(edge.target.clone());
            }
        }
    }
}
