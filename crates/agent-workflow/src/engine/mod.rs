pub mod dag;
pub mod executor;
pub mod variables;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use chrono::Local;
use futures::future::join_all;

use crate::error::WorkflowError;
use crate::model::{NodeType, Workflow, WorkflowEdge, WorkflowNode};
use crate::nodes;
use crate::run_db::WorkflowRunDb;
use crate::store::WorkflowStore;
use dag::resolve_dag;
use executor::NodeResult;
pub use variables::RuntimeProviderConfig;
use variables::VariableContext;

macro_rules! log_db_err {
    ($expr:expr) => {
        if let Err(e) = $expr {
            tracing::warn!("run_db 写入失败: {e}");
        }
    };
}

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

#[derive(Clone, Default)]
struct WorkflowExecutionOptions {
    human_approval_granted: bool,
    owner_session_id: String,
    workflow_snapshots: Option<Arc<HashMap<String, Workflow>>>,
}

/// 执行一条工作流
pub async fn execute_workflow(
    workflow: &Workflow,
    trigger_input: serde_json::Value,
    trigger_type: &str,
    run_db: &WorkflowRunDb,
) -> Result<WorkflowRunResult> {
    execute_workflow_with_provider_configs(
        workflow,
        trigger_input,
        trigger_type,
        run_db,
        environment_provider_configs()?,
    )
    .await
}

#[derive(serde::Deserialize)]
struct StoredProviders {
    #[serde(default)]
    providers: Vec<StoredProvider>,
}

#[derive(serde::Deserialize)]
struct StoredProvider {
    id: String,
    kind: String,
    endpoint: String,
    model: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    image_model: String,
    #[serde(default)]
    video_model: String,
    #[serde(default)]
    tts_model: String,
    #[serde(default)]
    music_model: String,
}

/// Headless workflow runs cannot access the desktop keyring. They still resolve
/// persisted provider IDs to backend IDs/endpoints and obtain credentials from
/// the provider's documented environment variables.
fn environment_provider_configs() -> Result<HashMap<String, RuntimeProviderConfig>> {
    let Some(stored) = home::settings::read::<StoredProviders>(
        &home::default_memory_dir(),
        &["desktop", "providers"],
    )?
    else {
        return Ok(HashMap::new());
    };
    let mut configs = HashMap::new();
    for provider in stored
        .providers
        .into_iter()
        .filter(|provider| provider.enabled)
    {
        let backend_id = providers::profile::normalize_provider_id(&provider.kind).to_string();
        let requires_key =
            providers::AuthKind::for_provider(&backend_id) != providers::AuthKind::None;
        let api_key = providers::profile::read_env_api_key(&backend_id).unwrap_or_default();
        if requires_key && api_key.is_empty() {
            continue;
        }
        let runtime = RuntimeProviderConfig {
            backend_id: backend_id.clone(),
            config: providers::ProviderConfig {
                api_key,
                base_url: (!provider.endpoint.trim().is_empty()).then_some(provider.endpoint),
                model: provider.model,
                ..providers::ProviderConfig::default()
            },
            image_model: provider.image_model,
            video_model: provider.video_model,
            tts_model: provider.tts_model,
            music_model: provider.music_model,
        };
        configs.insert(backend_id, runtime.clone());
        configs.insert(provider.id, runtime);
    }
    Ok(configs)
}

/// 执行工作流，并传入只存在于本次运行内存中的 Provider 凭据。
///
/// 密钥不写回 Workflow JSON，也不进入节点输出或日志。
pub async fn execute_workflow_with_provider_configs(
    workflow: &Workflow,
    trigger_input: serde_json::Value,
    trigger_type: &str,
    run_db: &WorkflowRunDb,
    provider_configs: HashMap<String, RuntimeProviderConfig>,
) -> Result<WorkflowRunResult> {
    let run_id = uuid::Uuid::new_v4().to_string();
    execute_workflow_with_provider_configs_and_run_id(
        workflow,
        trigger_input,
        trigger_type,
        run_db,
        provider_configs,
        run_id,
    )
    .await
}

/// 使用调用方预先分配的 run id 执行工作流。
///
/// Agent 工具适配器依靠该入口在转入后台运行前就向模型返回可查询的 id。
pub async fn execute_workflow_with_provider_configs_and_run_id(
    workflow: &Workflow,
    trigger_input: serde_json::Value,
    trigger_type: &str,
    run_db: &WorkflowRunDb,
    provider_configs: HashMap<String, RuntimeProviderConfig>,
    run_id: String,
) -> Result<WorkflowRunResult> {
    execute_workflow_with_options(
        workflow,
        trigger_input,
        trigger_type,
        run_db,
        provider_configs,
        run_id,
        WorkflowExecutionOptions::default(),
    )
    .await
}

/// Agent 工具调用入口：调用已经过 Harness 审批链，因此内部
/// HumanApproval 节点可消费这一次性授权，无需二次 park。
pub async fn execute_workflow_as_agent_tool(
    workflow: &Workflow,
    trigger_input: serde_json::Value,
    run_db: &WorkflowRunDb,
    provider_configs: HashMap<String, RuntimeProviderConfig>,
    run_id: String,
    owner_session_id: &str,
    workflow_snapshots: Arc<HashMap<String, Workflow>>,
) -> Result<WorkflowRunResult> {
    execute_workflow_with_options(
        workflow,
        trigger_input,
        "agent_tool",
        run_db,
        provider_configs,
        run_id,
        WorkflowExecutionOptions {
            human_approval_granted: true,
            owner_session_id: owner_session_id.to_string(),
            workflow_snapshots: Some(workflow_snapshots),
        },
    )
    .await
}

async fn execute_workflow_with_options(
    workflow: &Workflow,
    trigger_input: serde_json::Value,
    trigger_type: &str,
    run_db: &WorkflowRunDb,
    provider_configs: HashMap<String, RuntimeProviderConfig>,
    run_id: String,
    options: WorkflowExecutionOptions,
) -> Result<WorkflowRunResult> {
    anyhow::ensure!(!run_id.trim().is_empty(), "workflow run id 不能为空");
    let started_at = Local::now().to_rfc3339();

    run_db
        .insert_run_owned(
            &run_id,
            &workflow.id,
            &workflow.name,
            trigger_type,
            &started_at,
            &options.owner_session_id,
        )
        .await?;

    let timeout_secs = workflow
        .variables
        .get("timeout_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(WORKFLOW_TIMEOUT_SECS);

    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        execute_inner(
            workflow,
            trigger_input,
            &run_id,
            run_db,
            provider_configs,
            options,
        ),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => Err(WorkflowError::Timeout { timeout_secs }.into()),
    };

    let finished_at = Local::now().to_rfc3339();
    match &result {
        Ok(res) => {
            let output_str = res.output.as_ref().map(|v| {
                let s = serde_json::to_string(v).unwrap_or_default();
                truncate_utf8_safe(&s, 512_000)
            });
            run_db
                .finish_run(
                    &run_id,
                    &res.status,
                    &finished_at,
                    None,
                    output_str.as_deref(),
                    res.steps_executed as i64,
                )
                .await?;
        }
        Err(e) => {
            run_db
                .finish_run(
                    &run_id,
                    "failure",
                    &finished_at,
                    Some(&e.to_string()),
                    None,
                    0,
                )
                .await?;
        }
    }

    // 自动清理旧记录（保留最近 500 条）
    if let Err(e) = run_db.prune_old_runs(500).await {
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
    provider_configs: HashMap<String, RuntimeProviderConfig>,
    options: WorkflowExecutionOptions,
) -> Result<WorkflowRunResult> {
    execute_inner_with_depth(
        workflow,
        trigger_input,
        run_id,
        run_db,
        0,
        provider_configs,
        options,
    )
    .await
}

async fn execute_inner_with_depth(
    workflow: &Workflow,
    trigger_input: serde_json::Value,
    run_id: &str,
    run_db: &WorkflowRunDb,
    depth: u32,
    provider_configs: HashMap<String, RuntimeProviderConfig>,
    options: WorkflowExecutionOptions,
) -> Result<WorkflowRunResult> {
    let plan = resolve_dag(&workflow.nodes, &workflow.edges)?;

    let executors = nodes::executor_registry();

    let mut ctx = VariableContext::new(workflow.variables.clone())
        .with_provider_configs(provider_configs)
        .with_human_approval_granted(options.human_approval_granted)
        .with_workflow_snapshots(options.workflow_snapshots);

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
            log_db_err!(
                run_db
                    .insert_step_log(
                        step_id.as_str(),
                        run_id,
                        node_id,
                        &format!("{:?}", node.node_type),
                        &node.label,
                        &step_started,
                    )
                    .await
            );
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
                let executor = executors
                    .get(&node.node_type)
                    .expect("pre-checked in layer loop");
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
                        log_db_err!(
                            run_db
                                .finish_step_log(
                                    step_id,
                                    "success",
                                    &step_finished,
                                    out_str.as_deref(),
                                    None
                                )
                                .await
                        );
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
                        log_db_err!(
                            run_db
                                .finish_step_log(
                                    step_id,
                                    "success",
                                    &step_finished,
                                    Some(&format!("branches: {:?}", active_handles)),
                                    None
                                )
                                .await
                        );
                    }
                    Ok(NodeResult::Filtered) => {
                        mark_all_downstream(node_id, &workflow.edges, &node_map, &mut skipped);
                        log_db_err!(
                            run_db
                                .finish_step_log(
                                    step_id,
                                    "skipped",
                                    &step_finished,
                                    Some("filtered"),
                                    None
                                )
                                .await
                        );
                    }
                    Ok(NodeResult::Approved) => {
                        ctx.set_node_output(node_id, serde_json::json!({ "approved": true }));
                        log_db_err!(
                            run_db
                                .finish_step_log(
                                    step_id,
                                    "success",
                                    &step_finished,
                                    Some("approved"),
                                    None
                                )
                                .await
                        );
                    }
                    Ok(NodeResult::PendingApproval { prompt }) => {
                        log_db_err!(
                            run_db
                                .finish_step_log(
                                    step_id,
                                    "pending_approval",
                                    &step_finished,
                                    Some(
                                        &serde_json::json!({"prompt": prompt, "node_id": node_id})
                                            .to_string()
                                    ),
                                    None
                                )
                                .await
                        );
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
                                log_db_err!(
                                    run_db
                                        .finish_step_log(
                                            step_id,
                                            "skipped",
                                            &step_finished,
                                            None,
                                            Some(&err_msg)
                                        )
                                        .await
                                );
                            }
                            "fallback" => {
                                let fb = fallback_value.unwrap_or(serde_json::json!(null));
                                ctx.set_node_output(node_id, fb.clone());
                                let fb_str = serde_json::to_string(&fb).ok();
                                log_db_err!(
                                    run_db
                                        .finish_step_log(
                                            step_id,
                                            "fallback",
                                            &step_finished,
                                            fb_str.as_deref(),
                                            Some(&err_msg)
                                        )
                                        .await
                                );
                            }
                            _ => {
                                log_db_err!(
                                    run_db
                                        .finish_step_log(
                                            step_id,
                                            "failure",
                                            &step_finished,
                                            None,
                                            Some(&err_msg)
                                        )
                                        .await
                                );
                                return Err(WorkflowError::NodeExecFailed {
                                    node_id: node_id.to_string(),
                                    label: node.label.clone(),
                                    source: e,
                                }
                                .into());
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
                match executors.get(&node.node_type) {
                    Some(exec) => exec.execute(node, &ctx).await,
                    None => Err(WorkflowError::NoExecutor {
                        node_id: node_id.to_string(),
                        label: node.label.clone(),
                        node_type: node.node_type,
                    }
                    .into()),
                }
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
                    log_db_err!(
                        run_db
                            .finish_step_log(
                                step_id,
                                "success",
                                &step_finished,
                                out_str.as_deref(),
                                None
                            )
                            .await
                    );
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
                            log_db_err!(
                                run_db
                                    .finish_step_log(
                                        step_id,
                                        "skipped",
                                        &step_finished,
                                        None,
                                        Some(&err_msg)
                                    )
                                    .await
                            );
                        }
                        "fallback" => {
                            let fb = node
                                .config
                                .get("fallback_value")
                                .cloned()
                                .unwrap_or(serde_json::json!(null));
                            ctx.set_node_output(node_id, fb.clone());
                            let fb_str = serde_json::to_string(&fb).ok();
                            log_db_err!(
                                run_db
                                    .finish_step_log(
                                        step_id,
                                        "fallback",
                                        &step_finished,
                                        fb_str.as_deref(),
                                        Some(&err_msg)
                                    )
                                    .await
                            );
                        }
                        _ => {
                            log_db_err!(
                                run_db
                                    .finish_step_log(
                                        step_id,
                                        "failure",
                                        &step_finished,
                                        None,
                                        Some(&err_msg)
                                    )
                                    .await
                            );
                            return Err(WorkflowError::NodeExecFailed {
                                node_id: node_id.to_string(),
                                label: node.label.clone(),
                                source: e,
                            }
                            .into());
                        }
                    }
                }
                other => {
                    log_db_err!(
                        run_db
                            .finish_step_log(
                                step_id,
                                "success",
                                &step_finished,
                                Some(&format!("{:?}", other)),
                                None
                            )
                            .await
                    );
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
            return Err(WorkflowError::MaxDepthExceeded {
                max_depth: MAX_SUB_WORKFLOW_DEPTH,
            }
            .into());
        }
        let workflow_id = node
            .config
            .get("workflow_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if workflow_id.is_empty() {
            return Err(WorkflowError::MissingConfig {
                field: "workflow_id".into(),
            }
            .into());
        }
        let sub_wf = if let Some(snapshot) = ctx.workflow_snapshot(workflow_id) {
            snapshot.clone()
        } else {
            WorkflowStore::open_default()?
                .get(workflow_id)?
                .ok_or_else(|| WorkflowError::SubWorkflowNotFound {
                    workflow_id: workflow_id.to_string(),
                })?
        };

        let input = ctx.snapshot_outputs();
        let sub_run_id = uuid::Uuid::new_v4().to_string();
        let started_at = Local::now().to_rfc3339();
        log_db_err!(
            run_db
                .insert_run(
                    &sub_run_id,
                    &sub_wf.id,
                    &sub_wf.name,
                    "sub_workflow",
                    &started_at
                )
                .await
        );

        let result = execute_inner_with_depth(
            &sub_wf,
            input,
            &sub_run_id,
            run_db,
            depth + 1,
            ctx.provider_configs(),
            WorkflowExecutionOptions {
                human_approval_granted: ctx.human_approval_granted(),
                owner_session_id: String::new(),
                workflow_snapshots: ctx.workflow_snapshots(),
            },
        )
        .await;

        let finished_at = Local::now().to_rfc3339();
        match &result {
            Ok(res) => {
                let out_str = res
                    .output
                    .as_ref()
                    .map(|v| serde_json::to_string(v).unwrap_or_default());
                log_db_err!(
                    run_db
                        .finish_run(
                            &sub_run_id,
                            "success",
                            &finished_at,
                            None,
                            out_str.as_deref(),
                            res.steps_executed as i64
                        )
                        .await
                );
                Ok(NodeResult::Success(res.output.clone().unwrap_or(
                    serde_json::json!({"sub_workflow": workflow_id}),
                )))
            }
            Err(e) => {
                let err_msg = e.to_string();
                log_db_err!(
                    run_db
                        .finish_run(
                            &sub_run_id,
                            "failure",
                            &finished_at,
                            Some(&err_msg),
                            None,
                            0
                        )
                        .await
                );
                Err(WorkflowError::NodeExecFailed {
                    node_id: workflow_id.to_string(),
                    label: format!("子工作流 {}", workflow_id),
                    source: anyhow::anyhow!("{}", err_msg),
                }
                .into())
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
            log_db_err!(
                lc.run_db
                    .insert_step_log(
                        step_id.as_str(),
                        lc.run_id,
                        body_id,
                        &format!("{:?}", body_node.node_type),
                        &body_node.label,
                        &step_started
                    )
                    .await
            );

            let result = executor.execute(body_node, lc.ctx).await;
            let step_finished = Local::now().to_rfc3339();
            total_steps += 1;

            match result {
                Ok(NodeResult::Success(output)) => {
                    lc.ctx.set_node_output(body_id, output.clone());
                    let out_str = serde_json::to_string(&output).ok();
                    log_db_err!(
                        lc.run_db
                            .finish_step_log(
                                step_id.as_str(),
                                "success",
                                &step_finished,
                                out_str.as_deref(),
                                None
                            )
                            .await
                    );
                }
                Ok(other) => {
                    log_db_err!(
                        lc.run_db
                            .finish_step_log(
                                step_id.as_str(),
                                "success",
                                &step_finished,
                                Some(&format!("{:?}", other)),
                                None
                            )
                            .await
                    );
                }
                Err(e) => {
                    log_db_err!(
                        lc.run_db
                            .finish_step_log(
                                step_id.as_str(),
                                "failure",
                                &step_finished,
                                None,
                                Some(&e.to_string())
                            )
                            .await
                    );
                    return Err(WorkflowError::NodeExecFailed {
                        node_id: body_id.to_string(),
                        label: body_node.label.clone(),
                        source: e,
                    }
                    .into());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Position, WorkflowAgentTool};

    #[test]
    #[ignore = "explicit isolated cross-process fixture; run tools/verify-settings-restart.mjs"]
    fn settings_restart_read_fixture() {
        let root = home::test_env::settings_restart_fixture_root();
        let _env = home::test_env::AstroMemoryDirGuard::set(&root);
        let configured = environment_provider_configs().unwrap();
        let target = &configured["restart-local"];
        assert_eq!(target.backend_id, "openai");
        assert_eq!(target.config.model, "restart-model");
        assert_eq!(
            target.config.base_url.as_deref(),
            Some("http://127.0.0.1:19999/v1")
        );
    }

    #[test]
    fn headless_provider_registry_uses_shared_toml_and_rejects_invalid_config() {
        let dir = tempfile::tempdir().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        home::settings::write(dir.path(), &["desktop", "providers"], &serde_json::json!({
            "providers":[{"id":"local-test","kind":"ollama","endpoint":"http://127.0.0.1:7777","model":"test","enabled":true}]
        })).unwrap();
        let configs = environment_provider_configs().unwrap();
        assert_eq!(configs["local-test"].config.model, "test");
        assert_eq!(
            configs["local-test"].config.base_url.as_deref(),
            Some("http://127.0.0.1:7777")
        );
        std::fs::write(home::settings::path(dir.path()), "invalid = [").unwrap();
        assert!(environment_provider_configs().is_err());
    }

    fn workflow_with_middle(node_type: NodeType) -> Workflow {
        Workflow {
            id: "workflow-test".into(),
            name: "Workflow Test".into(),
            description: String::new(),
            enabled: true,
            agent_tool: WorkflowAgentTool::deferred(),
            nodes: vec![
                WorkflowNode {
                    id: "trigger".into(),
                    node_type: NodeType::ManualTrigger,
                    label: "trigger".into(),
                    position: Position { x: 0.0, y: 0.0 },
                    config: serde_json::json!({}),
                    disabled: false,
                },
                WorkflowNode {
                    id: "middle".into(),
                    node_type,
                    label: "middle".into(),
                    position: Position { x: 1.0, y: 0.0 },
                    config: serde_json::json!({"prompt_template": "approve"}),
                    disabled: false,
                },
                WorkflowNode {
                    id: "output".into(),
                    node_type: NodeType::Output,
                    label: "output".into(),
                    position: Position { x: 2.0, y: 0.0 },
                    config: serde_json::json!({
                        "output_fields": [{"name": "trigger_input.question"}]
                    }),
                    disabled: false,
                },
            ],
            edges: vec![
                WorkflowEdge {
                    id: "a".into(),
                    source: "trigger".into(),
                    source_handle: None,
                    target: "middle".into(),
                    target_handle: None,
                },
                WorkflowEdge {
                    id: "b".into(),
                    source: "middle".into(),
                    source_handle: None,
                    target: "output".into(),
                    target_handle: None,
                },
            ],
            variables: HashMap::new(),
            created_at: "now".into(),
            updated_at: "now".into(),
            icon: None,
        }
    }

    #[tokio::test]
    async fn explicit_run_id_and_agent_approval_flow_through_execution() {
        let dir = tempfile::tempdir().unwrap();
        let db = WorkflowRunDb::new(dir.path().join("workflow.db"))
            .await
            .unwrap();
        let workflow = workflow_with_middle(NodeType::HumanApproval);
        let result = execute_workflow_as_agent_tool(
            &workflow,
            serde_json::json!({"question": "hello"}),
            &db,
            HashMap::new(),
            "known-run-id".into(),
            "session-1",
            Arc::new(HashMap::new()),
        )
        .await
        .unwrap();

        assert_eq!(result.run_id, "known-run-id");
        assert_eq!(result.status, "success");
        assert_eq!(
            result.output.unwrap()["trigger_input.question"],
            serde_json::json!("hello")
        );
        assert_eq!(
            db.get_run("known-run-id").await.unwrap().unwrap().status,
            "success"
        );
        assert_eq!(
            db.get_run("known-run-id")
                .await
                .unwrap()
                .unwrap()
                .owner_session_id,
            "session-1"
        );
    }

    #[tokio::test]
    async fn pending_approval_is_persisted_as_pending() {
        let dir = tempfile::tempdir().unwrap();
        let db = WorkflowRunDb::new(dir.path().join("workflow.db"))
            .await
            .unwrap();
        let workflow = workflow_with_middle(NodeType::HumanApproval);
        let result = execute_workflow_with_provider_configs_and_run_id(
            &workflow,
            serde_json::json!({"question": "hello"}),
            "manual",
            &db,
            HashMap::new(),
            "pending-run-id".into(),
        )
        .await
        .unwrap();

        assert_eq!(result.status, "pending_approval");
        assert_eq!(
            db.get_run("pending-run-id").await.unwrap().unwrap().status,
            "pending_approval"
        );
    }

    #[tokio::test]
    async fn agent_sub_workflow_uses_the_frozen_snapshot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = WorkflowRunDb::new(dir.path().join("workflow.db"))
            .await
            .expect("workflow db");
        let child = Workflow {
            id: "frozen-child".into(),
            name: "Frozen child".into(),
            description: String::new(),
            enabled: true,
            agent_tool: WorkflowAgentTool::default(),
            nodes: vec![WorkflowNode {
                id: "child-trigger".into(),
                node_type: NodeType::ManualTrigger,
                label: "trigger".into(),
                position: Position { x: 0.0, y: 0.0 },
                config: serde_json::json!({}),
                disabled: false,
            }],
            edges: Vec::new(),
            variables: HashMap::new(),
            created_at: "now".into(),
            updated_at: "now".into(),
            icon: None,
        };
        let mut parent = workflow_with_middle(NodeType::CustomLoop);
        parent.nodes[1].config = serde_json::json!({"workflow_id": child.id.clone()});
        let snapshots = Arc::new(HashMap::from([(child.id.clone(), child)]));

        let result = execute_workflow_as_agent_tool(
            &parent,
            serde_json::json!({}),
            &db,
            HashMap::new(),
            "frozen-run".into(),
            "session-1",
            snapshots,
        )
        .await
        .expect("frozen child executes without reading the default store");
        assert_eq!(result.status, "success");
    }
}
