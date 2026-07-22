pub mod dag;
pub mod executor;
pub mod variables;

use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use chrono::Local;

use crate::model::{NodeType, Workflow, WorkflowEdge, WorkflowNode};
use crate::nodes;
use crate::run_db::WorkflowRunDb;
use crate::store::WorkflowStore;
use dag::{resolve_dag, downstream_from_handle};
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

/// 最大子工作流递归深度
const MAX_SUB_WORKFLOW_DEPTH: u32 = 5;

async fn execute_inner(
    workflow: &Workflow,
    trigger_input: serde_json::Value,
    run_id: &str,
    run_db: &WorkflowRunDb,
) -> Result<WorkflowRunResult> {
    execute_inner_with_depth(workflow, trigger_input, run_id, run_db, 0).await
}

async fn execute_inner_with_depth(
    workflow: &Workflow,
    trigger_input: serde_json::Value,
    run_id: &str,
    run_db: &WorkflowRunDb,
    depth: u32,
) -> Result<WorkflowRunResult> {
    let plan = resolve_dag(&workflow.nodes, &workflow.edges)?;

    let executors = nodes::build_executor_registry();

    let mut ctx = VariableContext::new(workflow.variables.clone());

    // 将 trigger_input 注入全局变量
    if let serde_json::Value::Object(map) = trigger_input {
        let mut merged = serde_json::Map::new();
        for (k, v) in map {
            merged.insert(k, v);
        }
        ctx.set_node_output("trigger_input", serde_json::Value::Object(merged));
    }

    let node_map: HashMap<&str, &WorkflowNode> = workflow
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();

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

            // ── 2.1 错误处理：读取节点配置的重试和错误策略 ──
            let retry_count = node.config.get("retry_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let retry_interval = node.config.get("retry_interval").and_then(|v| v.as_u64()).unwrap_or(1);
            let on_error = node.config.get("on_error").and_then(|v| v.as_str()).unwrap_or("abort");
            let fallback_value = node.config.get("fallback_value").cloned();

            let step_id = uuid::Uuid::new_v4().to_string();
            let step_started = Local::now().to_rfc3339();
            let _ = run_db.insert_step_log(
                &step_id, run_id, node_id,
                &format!("{:?}", node.node_type), &node.label, &step_started,
            );

            // ── 2.4 子工作流特殊处理 ──
            let result = if matches!(node.node_type, NodeType::RunLoop | NodeType::CustomLoop) {
                execute_sub_workflow(node, &ctx, run_db, depth).await
            } else {
                // 带重试的执行
                let mut last_err = None;
                let mut res = None;
                for attempt in 0..=retry_count {
                    if attempt > 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(retry_interval)).await;
                        tracing::info!(node = %node.label, attempt, "重试执行");
                    }
                    match executor.execute(node, &ctx).await {
                        Ok(r) => { res = Some(r); break; }
                        Err(e) => { last_err = Some(e); }
                    }
                }
                match res {
                    Some(r) => Ok(r),
                    None => Err(last_err.unwrap_or_else(|| anyhow::anyhow!("执行失败"))),
                }
            };

            let step_finished = Local::now().to_rfc3339();
            steps_executed += 1;

            match result {
                Ok(NodeResult::Success(output)) => {
                    // ── 2.3 Loop 节点迭代 ──
                    if node.node_type == NodeType::Loop {
                        let iter_steps = execute_loop_body(
                            node, &workflow.edges, &node_map, &executors,
                            &mut ctx, &mut skipped, run_id, run_db,
                        ).await?;
                        steps_executed += iter_steps;
                    }
                    ctx.set_node_output(node_id, output.clone());
                    if node.node_type == NodeType::Output {
                        final_output = Some(output.clone());
                    }
                    let out_str = serde_json::to_string(&output).ok();
                    let _ = run_db.finish_step_log(&step_id, "success", &step_finished, out_str.as_deref(), None);
                }
                Ok(NodeResult::Branch(active_handles)) => {
                    ctx.set_node_output(node_id, serde_json::json!({ "active_branches": &active_handles }));
                    mark_inactive_downstream(node_id, &active_handles, &workflow.edges, &mut skipped);
                    let _ = run_db.finish_step_log(&step_id, "success", &step_finished, Some(&format!("branches: {:?}", active_handles)), None);
                }
                Ok(NodeResult::Filtered) => {
                    mark_all_downstream(node_id, &workflow.edges, &node_map, &mut skipped);
                    let _ = run_db.finish_step_log(&step_id, "skipped", &step_finished, Some("filtered"), None);
                }
                Ok(NodeResult::Approved) => {
                    ctx.set_node_output(node_id, serde_json::json!({ "approved": true }));
                    let _ = run_db.finish_step_log(&step_id, "success", &step_finished, Some("approved"), None);
                }
                Ok(NodeResult::PendingApproval { prompt }) => {
                    let _ = run_db.finish_step_log(
                        &step_id, "pending_approval", &step_finished,
                        Some(&serde_json::json!({"prompt": prompt, "node_id": node_id}).to_string()), None,
                    );
                    return Ok(WorkflowRunResult {
                        run_id: run_id.to_string(),
                        status: "pending_approval".to_string(),
                        output: Some(serde_json::json!({"pending_node": node_id, "prompt": prompt})),
                        error: None,
                        steps_executed,
                    });
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    // ── 2.1 错误策略 ──
                    match on_error {
                        "skip" => {
                            tracing::warn!(node = %node.label, error = %err_msg, "节点失败，策略=skip，跳过下游");
                            mark_all_downstream(node_id, &workflow.edges, &node_map, &mut skipped);
                            let _ = run_db.finish_step_log(&step_id, "skipped", &step_finished, None, Some(&err_msg));
                        }
                        "fallback" => {
                            let fb = fallback_value.clone().unwrap_or(serde_json::json!(null));
                            tracing::warn!(node = %node.label, error = %err_msg, "节点失败，策略=fallback");
                            ctx.set_node_output(node_id, fb.clone());
                            let fb_str = serde_json::to_string(&fb).ok();
                            let _ = run_db.finish_step_log(&step_id, "fallback", &step_finished, fb_str.as_deref(), Some(&err_msg));
                        }
                        _ => {
                            let _ = run_db.finish_step_log(&step_id, "failure", &step_finished, None, Some(&err_msg));
                            bail!("节点 {} ({}) 执行失败: {}", node.label, node_id, e);
                        }
                    }
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

// ── 2.4 子工作流执行 ────────────────────────────────────────────────

fn execute_sub_workflow<'a>(
    node: &'a WorkflowNode,
    ctx: &'a VariableContext,
    run_db: &'a WorkflowRunDb,
    depth: u32,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<NodeResult>> + 'a>> {
    Box::pin(async move {
        if depth >= MAX_SUB_WORKFLOW_DEPTH {
            bail!("子工作流递归深度超过限制 ({})", MAX_SUB_WORKFLOW_DEPTH);
        }
        let workflow_id = node.config.get("workflow_id").and_then(|v| v.as_str()).unwrap_or("");
        if workflow_id.is_empty() {
            bail!("RunLoop 节点未配置 workflow_id");
        }
        let store = WorkflowStore::open_default()?;
        let sub_wf = store.get(workflow_id)?
            .ok_or_else(|| anyhow::anyhow!("子工作流 {} 不存在", workflow_id))?;

        let input = ctx.snapshot_outputs();
        let sub_run_id = uuid::Uuid::new_v4().to_string();
        let started_at = Local::now().to_rfc3339();
        let _ = run_db.insert_run(&sub_run_id, &sub_wf.id, &sub_wf.name, "sub_workflow", &started_at);

        let result = execute_inner_with_depth(&sub_wf, input, &sub_run_id, run_db, depth + 1).await;

        let finished_at = Local::now().to_rfc3339();
        match &result {
            Ok(res) => {
                let out_str = res.output.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default());
                let _ = run_db.finish_run(&sub_run_id, "success", &finished_at, None, out_str.as_deref(), res.steps_executed as i64);
                Ok(NodeResult::Success(res.output.clone().unwrap_or(serde_json::json!({"sub_workflow": workflow_id}))))
            }
            Err(e) => {
                let _ = run_db.finish_run(&sub_run_id, "failure", &finished_at, Some(&e.to_string()), None, 0);
                bail!("子工作流 {} 执行失败: {}", workflow_id, e)
            }
        }
    })
}

// ── 2.3 Loop 节点迭代 ──────────────────────────────────────────────

async fn execute_loop_body(
    loop_node: &WorkflowNode,
    edges: &[WorkflowEdge],
    node_map: &HashMap<&str, &WorkflowNode>,
    executors: &HashMap<NodeType, Box<dyn executor::NodeExecutor>>,
    ctx: &mut VariableContext,
    skipped: &mut HashSet<String>,
    run_id: &str,
    run_db: &WorkflowRunDb,
) -> Result<usize> {
    let max_iter = loop_node.config.get("max_iterations").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
    let break_cond = loop_node.config.get("break_condition").and_then(|v| v.as_str()).unwrap_or("");
    let loop_id = &loop_node.id;

    // 找到 loop 节点的直接下游节点（loop body）
    let body_nodes: Vec<String> = downstream_from_handle(loop_id, None, edges);
    if body_nodes.is_empty() {
        return Ok(0);
    }

    let mut total_steps = 0usize;
    for iteration in 0..max_iter {
        ctx.set_node_output(
            &format!("{}_iteration", loop_id),
            serde_json::json!({ "index": iteration }),
        );

        // 检查 break 条件
        if !break_cond.is_empty() {
            if let Ok(true) = ctx.evaluate_condition(break_cond) {
                tracing::info!(loop_node = %loop_node.label, iteration, "break 条件满足，退出循环");
                break;
            }
        }

        // 执行 body 中的每个节点
        for body_id in &body_nodes {
            if skipped.contains(body_id) { continue; }
            let body_node = match node_map.get(body_id.as_str()) {
                Some(n) => *n,
                None => continue,
            };
            let executor = match executors.get(&body_node.node_type) {
                Some(e) => e,
                None => continue,
            };

            let step_id = uuid::Uuid::new_v4().to_string();
            let step_started = Local::now().to_rfc3339();
            let _ = run_db.insert_step_log(&step_id, run_id, body_id, &format!("{:?}", body_node.node_type), &body_node.label, &step_started);

            let result = executor.execute(body_node, ctx).await;
            let step_finished = Local::now().to_rfc3339();
            total_steps += 1;

            match result {
                Ok(NodeResult::Success(output)) => {
                    ctx.set_node_output(body_id, output.clone());
                    let out_str = serde_json::to_string(&output).ok();
                    let _ = run_db.finish_step_log(&step_id, "success", &step_finished, out_str.as_deref(), None);
                }
                Ok(other) => {
                    let _ = run_db.finish_step_log(&step_id, "success", &step_finished, Some(&format!("{:?}", other)), None);
                }
                Err(e) => {
                    let _ = run_db.finish_step_log(&step_id, "failure", &step_finished, None, Some(&e.to_string()));
                    bail!("循环体节点 {} 执行失败: {}", body_node.label, e);
                }
            }
        }

        // 将 body 节点标记为已执行（skipped 集合中移除以允许下次迭代）
    }

    // loop body 节点已由迭代处理，标记为 skipped 防止 DAG 重复执行
    for body_id in &body_nodes {
        skipped.insert(body_id.clone());
    }

    Ok(total_steps)
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
