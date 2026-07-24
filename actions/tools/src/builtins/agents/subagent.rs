//! 临时子 Agent：同步 / 异步真 spawn（单一 `subagent` + `action`）。
//!
//! 经 `delegate` crate 回调执行；未注册 runner/spawner 时返回错误。
//! 用于回合内短暂并行子任务（不建持久 Agent）；串行多角色用 `pipeline`；
//! 新建长期助手请用 `persona_create`（禁止用本工具「创建人设」）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// One item in a batch delegate.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct DelegateTaskArgs {
    pub goal: String,
    /// Context the child needs (parent must pass explicitly).
    #[serde(default)]
    pub context: Option<String>,
    /// `leaf` (default) cannot re-delegate; `orchestrator` may spawn one more level if allowed.
    #[serde(default)]
    pub role: Option<String>,
    /// Toolset whitelist, e.g. `["terminal","file","web"]`; default = parent minus stripped sets.
    #[serde(default)]
    pub toolsets: Option<Vec<String>>,
    /// Max child iterations; default from config child_max_iterations (often 50).
    #[serde(default)]
    pub max_iterations: Option<usize>,
    /// Optional model: `provider:model_id`.
    #[serde(default)]
    pub model: Option<String>,
}

/// Arguments for the unified `subagent` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SubagentArgs {
    /// `run` (default) | `async` | `status` | `collect` | `cancel`.
    #[serde(default)]
    pub action: Option<String>,
    /// Single-task goal (mutually exclusive with `tasks`); for run/async.
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    /// Single-task role: `leaf` | `orchestrator`.
    #[serde(default)]
    pub role: Option<String>,
    /// Single-task toolset whitelist.
    #[serde(default)]
    pub toolsets: Option<Vec<String>>,
    #[serde(default)]
    pub max_iterations: Option<usize>,
    /// Single-task optional model: `provider:model_id`.
    #[serde(default)]
    pub model: Option<String>,
    /// Parallel sub-tasks (1–3 recommended; max 8).
    #[serde(default)]
    pub tasks: Option<Vec<DelegateTaskArgs>>,
    /// Concurrency cap; default from config (often 3).
    #[serde(default)]
    pub max_concurrent: Option<usize>,
    /// Task id for status/collect/cancel.
    #[serde(default)]
    pub task_id: Option<String>,
    /// Max wait seconds for collect; default 600.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// 向注册表登记 `subagent`。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "subagent".to_string(),
        toolset: "subagent".to_string(),
        description:
            "Spawn ephemeral in-turn sub-agent(s) for parallel one-shot work (isolated sessions). \
action=run (default, sync) | async (returns task_id) | status | collect | cancel. \
NOT for durable personas (use persona_create) and NOT for serial multi-role flows (use pipeline). \
Pass full context; children have no parent history."
                .to_string(),
        schema: schema_for_args::<SubagentArgs>(),
        check_fn: None,
        icon: "send",
        ..ToolEntry::lifecycle_defaults().exclusive().orchestrator_and_above()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["subagent"],
    async_named: handle,
}

/// 本模块统一入口：按 `action` 分发。
async fn handle(
    ctx: &mut ToolContext<'_>,
    _name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("run")
        .to_ascii_lowercase();
    match action.as_str() {
        "run" => dispatch(ctx, args),
        "async" => dispatch_async(ctx, args),
        "status" => dispatch_status(args),
        "collect" => dispatch_collect(args).await,
        "cancel" => dispatch_cancel(args),
        other => {
            anyhow::bail!("未知 subagent action: {other}（应为 run|async|status|collect|cancel）")
        }
    }
}

/// 同步执行真委派并返回摘要 JSON。
pub fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let req = build_run_request(ctx, args)?;
    let exec = ctx
        .execution
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no execution dispatch configured"))?;
    exec.run_sync(req)
}

/// 异步启动委派；立即返回 `task_id`。
pub fn dispatch_async(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let req = build_run_request(ctx, args)?;
    let exec = ctx
        .execution
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no execution dispatch configured"))?;
    let exec = std::sync::Arc::clone(exec);
    let task_id =
        delegate::start_delegate_async(req, move |task_id, req| exec.spawn_async(task_id, req))?;
    Ok(serde_json::json!({
        "task_id": task_id,
        "status": "running",
    })
    .to_string())
}

/// 查询异步委派状态。
pub fn dispatch_status(args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: SubagentArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("subagent status 参数无效: {e}"))?;
    let id = parsed.task_id.as_deref().unwrap_or("").trim();
    if id.is_empty() {
        anyhow::bail!("subagent status 需要非空 task_id");
    }
    let rec = delegate::async_delegate_status(id).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(record_to_json(&rec, true))
}

/// 等待异步委派完成。
pub async fn dispatch_collect(args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: SubagentArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("subagent collect 参数无效: {e}"))?;
    let id = parsed.task_id.as_deref().unwrap_or("").trim();
    if id.is_empty() {
        anyhow::bail!("subagent collect 需要非空 task_id");
    }
    let timeout = parsed.timeout_secs.unwrap_or(600).clamp(1, 3600);
    let rec = delegate::async_delegate_collect(id, timeout)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(record_to_json(&rec, false))
}

/// 取消异步委派。
pub fn dispatch_cancel(args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: SubagentArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("subagent cancel 参数无效: {e}"))?;
    let id = parsed.task_id.as_deref().unwrap_or("").trim();
    if id.is_empty() {
        anyhow::bail!("subagent cancel 需要非空 task_id");
    }
    let rec = delegate::async_delegate_cancel(id).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(record_to_json(&rec, true))
}

fn build_run_request(
    ctx: &ToolContext<'_>,
    args: &serde_json::Value,
) -> anyhow::Result<delegate::DelegateRunRequest> {
    if !home::can_spawn_nested() {
        anyhow::bail!(
            "spawn depth limit reached (depth {} >= max {})",
            home::current_spawn_depth(),
            home::effective_max_spawn_depth()
        );
    }
    let parsed: SubagentArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("subagent 参数无效: {e}"))?;
    let task_specs = resolve_tasks(&parsed)?;
    let cfg = hooks::config::load_config_or_default();
    let max_concurrent = parsed
        .max_concurrent
        .unwrap_or(cfg.delegation.max_concurrent_children)
        .clamp(1, 8);
    let max_spawn_depth =
        home::scoped_max_spawn_depth().unwrap_or(cfg.delegation.max_spawn_depth.max(1));
    Ok(delegate::DelegateRunRequest {
        parent_agent_id: ctx.memory.agent_id.clone(),
        parent_session_id: ctx.session_id.clone(),
        provider: ctx.chat_provider.clone(),
        model: ctx.chat_model.clone(),
        api_key: ctx.chat_api_key.clone(),
        base_url: ctx.chat_base_url.clone(),
        chat_targets: ctx.chat_targets.clone(),
        tasks: task_specs,
        max_concurrent,
        caller_depth: home::current_spawn_depth(),
        max_spawn_depth,
        project_root: ctx.project_root.clone(),
        hook_bus: ctx.hook_bus.clone(),
    })
}

fn resolve_tasks(parsed: &SubagentArgs) -> anyhow::Result<Vec<delegate::DelegateTaskSpec>> {
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
            out.push(delegate::DelegateTaskSpec {
                goal: goal.to_string(),
                context: t.context.as_deref().unwrap_or("").trim().to_string(),
                role: delegate::DelegateRole::parse(t.role.as_deref().unwrap_or("leaf")),
                toolsets: t.toolsets.clone(),
                max_iterations: t.max_iterations,
                model: t
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            });
        }
        return Ok(out);
    }

    let goal = parsed.goal.as_deref().unwrap_or("").trim();
    if goal.is_empty() {
        anyhow::bail!("subagent 需要 goal 或 tasks");
    }
    Ok(vec![delegate::DelegateTaskSpec {
        goal: goal.to_string(),
        context: parsed.context.as_deref().unwrap_or("").trim().to_string(),
        role: delegate::DelegateRole::parse(parsed.role.as_deref().unwrap_or("leaf")),
        toolsets: parsed.toolsets.clone(),
        max_iterations: parsed.max_iterations,
        model: parsed
            .model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    }])
}

fn record_to_json(rec: &delegate::AsyncDelegateRecord, truncate_result: bool) -> String {
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

#[cfg(test)]
mod resolve_tests {
    use super::*;

    #[test]
    fn parses_role_toolsets_max_iterations() {
        let parsed = SubagentArgs {
            action: None,
            goal: Some("do it".into()),
            context: Some("ctx".into()),
            role: Some("orchestrator".into()),
            toolsets: Some(vec!["terminal".into(), "file".into()]),
            max_iterations: Some(12),
            model: Some("claude:opus".into()),
            tasks: None,
            max_concurrent: None,
            task_id: None,
            timeout_secs: None,
        };
        let specs = resolve_tasks(&parsed).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].role, delegate::DelegateRole::Orchestrator);
        assert_eq!(
            specs[0].toolsets.as_ref().unwrap(),
            &vec!["terminal".to_string(), "file".to_string()]
        );
        assert_eq!(specs[0].max_iterations, Some(12));
        assert_eq!(specs[0].model.as_deref(), Some("claude:opus"));
    }
}
