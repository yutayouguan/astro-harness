pub mod dag;
pub mod executor;
pub mod variables;

use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use chrono::Local;
use futures::future::join_all;

macro_rules! log_db_err {
    ($expr:expr) => {
        if let Err(e) = $expr {
            tracing::warn!("run_db 写入失败: {e}");
        }
    };
}

use crate::model::{NodeType, Workflow, WorkflowEdge, WorkflowNode};
use crate::nodes;
use crate::run_db::WorkflowRunDb;
use crate::store::WorkflowStore;
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

const WORKFLOW_TIMEOUT_SECS: u64 = 30 * 60; // 30 分钟

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

    let timeout_secs = workflow
        .variables
        .get("timeout_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(WORKFLOW_TIMEOUT_SECS);

    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        execute_inner(workflow, trigger_input, &run_id, run_db),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => Err(anyhow::anyhow!(
            "工作流执行超时（{}秒），可在工作流变量中设置 timeout_seconds 调整",
            timeout_secs,
        )),
    };

    let finished_at = Local::now().to_rfc3339();
    match &result {
        Ok(res) => {
            let output_str = res.output.as_ref().map(|v| {
                let s = serde_json::to_string(v).unwrap_or_default();
                truncate_utf8_safe(&s, 512_000)
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
            run_db.finish_run(
                &run_id,
                "failure",
                &finished_at,
                Some(&e.to_string()),
                None,
                0,
            )?;
        }
    }

    // 自动清理旧记录（保留最近 500 条）
    if let Err(e) = run_db.prune_old_runs(500) {
        tracing::warn!("清理旧运行记录失败: {e}");
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

    let executors = nodes::executor_registry();

    let mut ctx = VariableContext::new(workflow.variables.clone());

    // 将 trigger_input 注入全局变量
    if let serde_json::Value::Object(map) = trigger_input {
        let mut merged = serde_json::Map::new();
        for (k, v) in map {
            merged.insert(k, v);
        }
        ctx.set_node_output("trigger_input", serde_json::Value::Object(merged));
    }

    let node_map: HashMap<&str, &WorkflowNode> =
        workflow.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let mut skipped: HashSet<String> = HashSet::new();
    let mut steps_executed = 0usize;
    let mut final_output: Option<serde_json::Value> = None;

    for layer in &plan.layers {
        // ── 2.2 层内并行执行 ──
        // 收集本层可执行的节点
        let mut layer_tasks: Vec<(&str, &WorkflowNode, String)> = Vec::new();
        // 需要串行处理的特殊节点（子工作流、循环）
        let mut serial_nodes: Vec<(&str, &WorkflowNode, String)> = Vec::new();

        for node_id in layer {
            if skipped.contains(node_id) {
                continue;
            }
            let node = match node_map.get(node_id.as_str()) {
                Some(n) => *n,
                None => continue,
            };
            if executors.get(&node.node_type).is_none() {
                tracing::warn!(node_type = ?node.node_type, "无可用执行器，跳过");
                continue;
            }
            let step_id = uuid::Uuid::new_v4().to_string();
            let step_started = Local::now().to_rfc3339();
            log_db_err!(run_db.insert_step_log(
                step_id.as_str(),
                run_id,
                node_id,
                &format!("{:?}", node.node_type),
                &node.label,
                &step_started,
            ));
            if matches!(
                node.node_type,
                NodeType::RunLoop | NodeType::CustomLoop | NodeType::Loop
            ) {
                serial_nodes.push((node_id.as_str(), node, step_id));
            } else {
                layer_tasks.push((node_id.as_str(), node, step_id));
            }
        }

        // 并行执行普通节点
        if !layer_tasks.is_empty() {
            let ctx_ref = &ctx;
            let futs = layer_tasks.iter().map(|(_, node, _)| {
                let executor = executors.get(&node.node_type).unwrap();
                let retry_count = node
                    .config
                    .get("retry_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let retry_interval = node
                    .config
                    .get("retry_interval")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1);
                async move {
                    let mut last_err = None;
                    for attempt in 0..=retry_count {
                        if attempt > 0 {
                            tokio::time::sleep(std::time::Duration::from_secs(retry_interval))
                                .await;
                        }
                        match executor.execute(node, ctx_ref).await {
                            Ok(r) => return Ok(r),
                            Err(e) => {
                                last_err = Some(e);
                            }
                        }
                    }
                    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("执行失败")))
                }
            });

            let results: Vec<Result<NodeResult>> = join_all(futs).await;

            for ((node_id, node, step_id), result) in layer_tasks.iter().zip(results) {
                let step_finished = Local::now().to_rfc3339();
                steps_executed += 1;
                let on_error = node
                    .config
                    .get("on_error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("abort");
                let fallback_value = node.config.get("fallback_value").cloned();

                match result {
                    Ok(NodeResult::Success(output)) => {
                        ctx.set_node_output(node_id, output.clone());
                        if node.node_type == NodeType::Output {
                            final_output = Some(output.clone());
                        }
                        let out_str = serde_json::to_string(&output).ok();
                        log_db_err!(run_db.finish_step_log(
                            step_id,
                            "success",
                            &step_finished,
                            out_str.as_deref(),
                            None
                        ));
                    }
                    Ok(NodeResult::Branch(active_handles)) => {
                        ctx.set_node_output(
                            node_id,
                            serde_json::json!({ "active_branches": &active_handles }),
                        );
                        mark_inactive_downstream(
                            node_id,
                            &active_handles,
                            &workflow.edges,
                            &mut skipped,
                        );
                        log_db_err!(run_db.finish_step_log(
                            step_id,
                            "success",
                            &step_finished,
                            Some(&format!("branches: {:?}", active_handles)),
                            None
                        ));
                    }
                    Ok(NodeResult::Filtered) => {
                        mark_all_downstream(node_id, &workflow.edges, &node_map, &mut skipped);
                        log_db_err!(run_db.finish_step_log(
                            step_id,
                            "skipped",
                            &step_finished,
                            Some("filtered"),
                            None
                        ));
                    }
                    Ok(NodeResult::Approved) => {
                        ctx.set_node_output(node_id, serde_json::json!({ "approved": true }));
                        log_db_err!(run_db.finish_step_log(
                            step_id,
                            "success",
                            &step_finished,
                            Some("approved"),
                            None
                        ));
                    }
                    Ok(NodeResult::PendingApproval { prompt }) => {
                        log_db_err!(run_db.finish_step_log(
                            step_id,
                            "pending_approval",
                            &step_finished,
                            Some(
                                &serde_json::json!({"prompt": prompt, "node_id": node_id})
                                    .to_string()
                            ),
                            None
                        ));
                        return Ok(WorkflowRunResult {
                            run_id: run_id.to_string(),
                            status: "pending_approval".to_string(),
                            output: Some(
                                serde_json::json!({"pending_node": node_id, "prompt": prompt}),
                            ),
                            error: None,
                            steps_executed,
                        });
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        match on_error {
                            "skip" => {
                                mark_all_downstream(
                                    node_id,
                                    &workflow.edges,
                                    &node_map,
                                    &mut skipped,
                                );
                                log_db_err!(run_db.finish_step_log(
                                    step_id,
                                    "skipped",
                                    &step_finished,
                                    None,
                                    Some(&err_msg)
                                ));
                            }
                            "fallback" => {
                                let fb = fallback_value.unwrap_or(serde_json::json!(null));
                                ctx.set_node_output(node_id, fb.clone());
                                let fb_str = serde_json::to_string(&fb).ok();
                                log_db_err!(run_db.finish_step_log(
                                    step_id,
                                    "fallback",
                                    &step_finished,
                                    fb_str.as_deref(),
                                    Some(&err_msg)
                                ));
                            }
                            _ => {
                                log_db_err!(run_db.finish_step_log(
                                    step_id,
                                    "failure",
                                    &step_finished,
                                    None,
                                    Some(&err_msg)
                                ));
                                bail!("节点 {} ({}) 执行失败: {}", node.label, node_id, e);
                            }
                        }
                    }
                }
            }
        }

        // 串行执行特殊节点（子工作流、循环）
        for (node_id, node, step_id) in &serial_nodes {
            let result = if matches!(node.node_type, NodeType::RunLoop | NodeType::CustomLoop) {
                execute_sub_workflow(node, &ctx, run_db, depth).await
            } else {
                // Loop 节点
                executors
                    .get(&node.node_type)
                    .unwrap()
                    .execute(node, &ctx)
                    .await
            };

            let step_finished = Local::now().to_rfc3339();
            steps_executed += 1;

            match result {
                Ok(NodeResult::Success(output)) => {
                    if node.node_type == NodeType::Loop {
                        let iter_steps = execute_loop_body(
                            node,
                            &mut LoopContext {
                                edges: &workflow.edges,
                                node_map: &node_map,
                                executors,
                                ctx: &mut ctx,
                                skipped: &mut skipped,
                                run_id,
                                run_db,
                            },
                        )
                        .await?;
                        steps_executed += iter_steps;
                    }
                    ctx.set_node_output(node_id, output.clone());
                    if node.node_type == NodeType::Output {
                        final_output = Some(output.clone());
                    }
                    let out_str = serde_json::to_string(&output).ok();
                    log_db_err!(run_db.finish_step_log(
                        step_id,
                        "success",
                        &step_finished,
                        out_str.as_deref(),
                        None
                    ));
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    let on_error = node
                        .config
                        .get("on_error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("abort");
                    match on_error {
                        "skip" => {
                            mark_all_downstream(node_id, &workflow.edges, &node_map, &mut skipped);
                            log_db_err!(run_db.finish_step_log(
                                step_id,
                                "skipped",
                                &step_finished,
                                None,
                                Some(&err_msg)
                            ));
                        }
                        "fallback" => {
                            let fb = node
                                .config
                                .get("fallback_value")
                                .cloned()
                                .unwrap_or(serde_json::json!(null));
                            ctx.set_node_output(node_id, fb.clone());
                            let fb_str = serde_json::to_string(&fb).ok();
                            log_db_err!(run_db.finish_step_log(
                                step_id,
                                "fallback",
                                &step_finished,
                                fb_str.as_deref(),
                                Some(&err_msg)
                            ));
                        }
                        _ => {
                            log_db_err!(run_db.finish_step_log(
                                step_id,
                                "failure",
                                &step_finished,
                                None,
                                Some(&err_msg)
                            ));
                            bail!("节点 {} ({}) 执行失败: {}", node.label, node_id, e);
                        }
                    }
                }
                other => {
                    log_db_err!(run_db.finish_step_log(
                        step_id,
                        "success",
                        &step_finished,
                        Some(&format!("{:?}", other)),
                        None
                    ));
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
        let workflow_id = node
            .config
            .get("workflow_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if workflow_id.is_empty() {
            bail!("RunLoop 节点未配置 workflow_id");
        }
        let store = WorkflowStore::open_default()?;
        let sub_wf = store
            .get(workflow_id)?
            .ok_or_else(|| anyhow::anyhow!("子工作流 {} 不存在", workflow_id))?;

        let input = ctx.snapshot_outputs();
        let sub_run_id = uuid::Uuid::new_v4().to_string();
        let started_at = Local::now().to_rfc3339();
        log_db_err!(run_db.insert_run(
            &sub_run_id,
            &sub_wf.id,
            &sub_wf.name,
            "sub_workflow",
            &started_at
        ));

        let result = execute_inner_with_depth(&sub_wf, input, &sub_run_id, run_db, depth + 1).await;

        let finished_at = Local::now().to_rfc3339();
        match &result {
            Ok(res) => {
                let out_str = res
                    .output
                    .as_ref()
                    .map(|v| serde_json::to_string(v).unwrap_or_default());
                log_db_err!(run_db.finish_run(
                    &sub_run_id,
                    "success",
                    &finished_at,
                    None,
                    out_str.as_deref(),
                    res.steps_executed as i64
                ));
                Ok(NodeResult::Success(res.output.clone().unwrap_or(
                    serde_json::json!({"sub_workflow": workflow_id}),
                )))
            }
            Err(e) => {
                log_db_err!(run_db.finish_run(
                    &sub_run_id,
                    "failure",
                    &finished_at,
                    Some(&e.to_string()),
                    None,
                    0
                ));
                bail!("子工作流 {} 执行失败: {}", workflow_id, e)
            }
        }
    })
}

// ── 2.3 Loop 节点迭代 ──────────────────────────────────────────────

struct LoopContext<'a> {
    edges: &'a [WorkflowEdge],
    node_map: &'a HashMap<&'a str, &'a WorkflowNode>,
    executors: &'a HashMap<NodeType, Box<dyn executor::NodeExecutor>>,
    ctx: &'a mut VariableContext,
    skipped: &'a mut HashSet<String>,
    run_id: &'a str,
    run_db: &'a WorkflowRunDb,
}

async fn execute_loop_body(loop_node: &WorkflowNode, lc: &mut LoopContext<'_>) -> Result<usize> {
    let max_iter = loop_node
        .config
        .get("max_iterations")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;
    let break_cond = loop_node
        .config
        .get("break_condition")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let loop_id = &loop_node.id;

    // 收集 loop 节点的完整子 DAG（拓扑序），而非仅直接下游
    let body_chain = collect_sub_dag(loop_id, lc.edges, lc.node_map);
    if body_chain.is_empty() {
        return Ok(0);
    }

    let mut total_steps = 0usize;
    for iteration in 0..max_iter {
        lc.ctx.set_node_output(
            &format!("{}_iteration", loop_id),
            serde_json::json!({ "index": iteration }),
        );

        if !break_cond.is_empty() {
            if let Ok(true) = lc.ctx.evaluate_condition(break_cond) {
                tracing::info!(loop_node = %loop_node.label, iteration, "break 条件满足，退出循环");
                break;
            }
        }

        for body_id in &body_chain {
            if lc.skipped.contains(body_id) {
                continue;
            }
            let body_node = match lc.node_map.get(body_id.as_str()) {
                Some(n) => *n,
                None => continue,
            };
            let executor = match lc.executors.get(&body_node.node_type) {
                Some(e) => e,
                None => continue,
            };

            let step_id = uuid::Uuid::new_v4().to_string();
            let step_started = Local::now().to_rfc3339();
            log_db_err!(lc.run_db.insert_step_log(
                step_id.as_str(),
                lc.run_id,
                body_id,
                &format!("{:?}", body_node.node_type),
                &body_node.label,
                &step_started
            ));

            let result = executor.execute(body_node, lc.ctx).await;
            let step_finished = Local::now().to_rfc3339();
            total_steps += 1;

            match result {
                Ok(NodeResult::Success(output)) => {
                    lc.ctx.set_node_output(body_id, output.clone());
                    let out_str = serde_json::to_string(&output).ok();
                    log_db_err!(lc.run_db.finish_step_log(
                        step_id.as_str(),
                        "success",
                        &step_finished,
                        out_str.as_deref(),
                        None
                    ));
                }
                Ok(other) => {
                    log_db_err!(lc.run_db.finish_step_log(
                        step_id.as_str(),
                        "success",
                        &step_finished,
                        Some(&format!("{:?}", other)),
                        None
                    ));
                }
                Err(e) => {
                    log_db_err!(lc.run_db.finish_step_log(
                        step_id.as_str(),
                        "failure",
                        &step_finished,
                        None,
                        Some(&e.to_string())
                    ));
                    bail!("循环体节点 {} 执行失败: {}", body_node.label, e);
                }
            }
        }
    }

    for body_id in &body_chain {
        lc.skipped.insert(body_id.clone());
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
                None => !active_handles.is_empty(),
            };
            if !is_active && !skipped.contains(&edge.target) {
                skipped.insert(edge.target.clone());
                // 递归 skip 所有下游
                let mut queue = vec![edge.target.clone()];
                while let Some(cur) = queue.pop() {
                    for e2 in edges {
                        if e2.source == cur && !skipped.contains(&e2.target) {
                            skipped.insert(e2.target.clone());
                            queue.push(e2.target.clone());
                        }
                    }
                }
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

/// 从 start_node 出发，收集所有可达下游节点并按拓扑序排列
fn collect_sub_dag(
    start_node: &str,
    edges: &[WorkflowEdge],
    node_map: &HashMap<&str, &WorkflowNode>,
) -> Vec<String> {
    // 1. BFS 收集可达节点
    let mut reachable = HashSet::new();
    let mut bfs_queue = std::collections::VecDeque::new();
    for e in edges {
        if e.source == start_node && !reachable.contains(&e.target) {
            reachable.insert(e.target.clone());
            bfs_queue.push_back(e.target.clone());
        }
    }
    while let Some(id) = bfs_queue.pop_front() {
        for e in edges {
            if e.source == id && !reachable.contains(&e.target) {
                reachable.insert(e.target.clone());
                bfs_queue.push_back(e.target.clone());
            }
        }
    }

    // 2. Kahn 拓扑排序（仅在子图内）
    let mut in_deg: HashMap<&str, usize> =
        reachable.iter().map(|id| (id.as_str(), 0usize)).collect();
    for e in edges {
        if reachable.contains(&e.source) && reachable.contains(&e.target) {
            *in_deg.entry(e.target.as_str()).or_insert(0) += 1;
        }
    }
    // start_node 的直接下游入度减去来自 start_node 的边
    for e in edges {
        if e.source == start_node && reachable.contains(&e.target) {
            if let Some(d) = in_deg.get_mut(e.target.as_str()) {
                *d = d.saturating_sub(1);
            }
        }
    }

    let mut topo_queue: std::collections::VecDeque<String> = in_deg
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&id, _)| id.to_string())
        .collect();
    let mut result = Vec::new();
    while let Some(id) = topo_queue.pop_front() {
        if node_map.contains_key(id.as_str()) {
            result.push(id.clone());
        }
        for e in edges {
            if e.source == id && reachable.contains(&e.target) {
                if let Some(d) = in_deg.get_mut(e.target.as_str()) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        topo_queue.push_back(e.target.clone());
                    }
                }
            }
        }
    }
    // 环中节点兜底（避免丢失）
    for id in &reachable {
        if !result.contains(id) && node_map.contains_key(id.as_str()) {
            result.push(id.clone());
        }
    }

    result
}

fn truncate_utf8_safe(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}
