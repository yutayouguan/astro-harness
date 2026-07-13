//! 委派工具：同步 / 异步真 spawn 子 Agent（Hermes `delegate_task` 对齐）。
//!
//! 经 `memory` OnceLock 回调执行；未注册 runner/spawner 时返回错误。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// 批量委派中的单项。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct DelegateTaskArgs {
    /// 子任务目标。
    pub goal: String,
    /// 子 Agent 所需上下文（父须显式传入）。
    #[serde(default)]
    pub context: Option<String>,
}

/// `delegate` / `delegate_async` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct DelegateArgs {
    /// 单任务目标（与 `task` / `tasks` 三选一）。
    #[serde(default)]
    pub goal: Option<String>,
    /// 兼容旧字段：等同于 `goal`。
    #[serde(default)]
    pub task: Option<String>,
    /// 单任务上下文。
    #[serde(default)]
    pub context: Option<String>,
    /// 并行子任务（1～3 建议；上限 8）。
    #[serde(default)]
    pub tasks: Option<Vec<DelegateTaskArgs>>,
    /// 并行上限，默认 3。
    #[serde(default)]
    pub max_concurrent: Option<usize>,
    /// 遗留字段，忽略（真委派复用父 Agent）。
    #[serde(default)]
    pub agent_type: Option<String>,
}

/// `delegate_status` / `delegate_cancel` 参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct DelegateTaskIdArgs {
    /// 由 `delegate_async` 返回的任务 id。
    pub task_id: String,
}

/// `delegate_collect` 参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct DelegateCollectArgs {
    /// 由 `delegate_async` 返回的任务 id。
    pub task_id: String,
    /// 最长等待秒数，默认 600。
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// 向注册表登记委派相关工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "delegate".to_string(),
        toolset: "delegate".to_string(),
        description: "Spawn isolated sub-agent(s) to work on goal(s) in parallel (max 3 by default). Blocks until done; only summaries return. Pass full context—sub-agents have no parent history. Prefer orchestration_run for multi-step serial pipelines; use delegate_async to not block the parent turn.".to_string(),
        schema: schema_for_args::<DelegateArgs>(),
        check_fn: None,
        icon: "send",
    });
    registry.register(ToolEntry {
        name: "delegate_async".to_string(),
        toolset: "delegate".to_string(),
        description: "Start delegated sub-agent(s) in the background. Returns task_id immediately; poll with delegate_status or wait with delegate_collect. Same args as delegate.".to_string(),
        schema: schema_for_args::<DelegateArgs>(),
        check_fn: None,
        icon: "send",
    });
    registry.register(ToolEntry {
        name: "delegate_status".to_string(),
        toolset: "delegate".to_string(),
        description: "Query status of a background delegate started by delegate_async.".to_string(),
        schema: schema_for_args::<DelegateTaskIdArgs>(),
        check_fn: None,
        icon: "list-checks",
    });
    registry.register(ToolEntry {
        name: "delegate_collect".to_string(),
        toolset: "delegate".to_string(),
        description: "Wait until a background delegate finishes (or timeout) and return its result.".to_string(),
        schema: schema_for_args::<DelegateCollectArgs>(),
        check_fn: None,
        icon: "hourglass",
    });
    registry.register(ToolEntry {
        name: "delegate_cancel".to_string(),
        toolset: "delegate".to_string(),
        description: "Cancel a background delegate if still running. Best-effort; in-flight children may finish but result is discarded.".to_string(),
        schema: schema_for_args::<DelegateTaskIdArgs>(),
        check_fn: None,
        icon: "x",
    });
}

/// 同步执行真委派并返回摘要 JSON。
pub fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let req = build_run_request(ctx, args)?;
    memory::run_delegate_sync(req)
}

/// 异步启动委派；立即返回 `task_id`。
pub fn dispatch_async(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let req = build_run_request(ctx, args)?;
    let task_id = memory::start_delegate_async(req)?;
    Ok(serde_json::json!({
        "task_id": task_id,
        "status": "running",
    })
    .to_string())
}

/// 查询异步委派状态。
pub fn dispatch_status(args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: DelegateTaskIdArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("delegate_status 参数无效: {e}"))?;
    let id = parsed.task_id.trim();
    if id.is_empty() {
        anyhow::bail!("delegate_status 需要非空 task_id");
    }
    let rec = memory::async_delegate_status(id).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(record_to_json(&rec, true))
}

/// 等待异步委派完成。
pub async fn dispatch_collect(args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: DelegateCollectArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("delegate_collect 参数无效: {e}"))?;
    let id = parsed.task_id.trim();
    if id.is_empty() {
        anyhow::bail!("delegate_collect 需要非空 task_id");
    }
    let timeout = parsed.timeout_secs.unwrap_or(600).clamp(1, 3600);
    let rec = memory::async_delegate_collect(id, timeout)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(record_to_json(&rec, false))
}

/// 取消异步委派。
pub fn dispatch_cancel(args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: DelegateTaskIdArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("delegate_cancel 参数无效: {e}"))?;
    let id = parsed.task_id.trim();
    if id.is_empty() {
        anyhow::bail!("delegate_cancel 需要非空 task_id");
    }
    let rec = memory::async_delegate_cancel(id).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(record_to_json(&rec, true))
}

fn build_run_request(
    ctx: &ToolContext<'_>,
    args: &serde_json::Value,
) -> anyhow::Result<memory::DelegateRunRequest> {
    if !memory::can_spawn_nested() {
        anyhow::bail!(
            "spawn depth limit reached (depth {} >= max {})",
            memory::current_spawn_depth(),
            memory::effective_max_spawn_depth()
        );
    }
    let parsed: DelegateArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("delegate 参数无效: {e}"))?;
    let task_specs = resolve_tasks(&parsed)?;
    let max_concurrent = parsed.max_concurrent.unwrap_or(3).clamp(1, 8);
    Ok(memory::DelegateRunRequest {
        parent_agent_id: ctx.memory.agent_id.clone(),
        parent_session_id: ctx.session_id.clone(),
        provider: ctx.chat_provider.clone(),
        model: ctx.chat_model.clone(),
        api_key: ctx.chat_api_key.clone(),
        base_url: ctx.chat_base_url.clone(),
        tasks: task_specs,
        max_concurrent,
        caller_depth: memory::current_spawn_depth(),
        max_spawn_depth: memory::effective_max_spawn_depth(),
    })
}

fn resolve_tasks(parsed: &DelegateArgs) -> anyhow::Result<Vec<memory::DelegateTaskSpec>> {
    if let Some(tasks) = &parsed.tasks {
        if tasks.is_empty() {
            anyhow::bail!("tasks 不能为空");
        }
        if tasks.len() > 8 {
            anyhow::bail!("tasks 最多 8 项");
        }
        let mut out = Vec::with_capacity(tasks.len());
        for t in tasks {
            let goal = t.goal.trim();
            if goal.is_empty() {
                anyhow::bail!("tasks[].goal 不能为空");
            }
            out.push(memory::DelegateTaskSpec {
                goal: goal.to_string(),
                context: t.context.as_deref().unwrap_or("").trim().to_string(),
            });
        }
        return Ok(out);
    }

    let goal = parsed
        .goal
        .as_deref()
        .or(parsed.task.as_deref())
        .unwrap_or("")
        .trim();
    if goal.is_empty() {
        anyhow::bail!("delegate 需要 goal、task 或 tasks");
    }
    Ok(vec![memory::DelegateTaskSpec {
        goal: goal.to_string(),
        context: parsed.context.as_deref().unwrap_or("").trim().to_string(),
    }])
}

fn record_to_json(rec: &memory::AsyncDelegateRecord, truncate_result: bool) -> String {
    let mut result = rec.result_json.clone();
    if truncate_result && result.len() > 2_000 {
        result.truncate(2_000);
        result.push('…');
    }
    let mut error = rec.error.clone();
    if truncate_result && error.len() > 500 {
        error.truncate(500);
        error.push('…');
    }
    serde_json::json!({
        "task_id": rec.id,
        "status": rec.status,
        "parent_session_id": rec.parent_session_id,
        "result": if result.is_empty() { serde_json::Value::Null } else {
            serde_json::from_str(&result).unwrap_or(serde_json::Value::String(result))
        },
        "error": if error.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(error) },
        "created_at": rec.created_at,
        "finished_at": if rec.finished_at.is_empty() { serde_json::Value::Null } else {
            serde_json::Value::String(rec.finished_at.clone())
        },
    })
    .to_string()
}
