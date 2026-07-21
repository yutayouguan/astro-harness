//! 多 Agent 编排工具：异步串行调度子 Agent，并可查询进度。
//!
//! `orchestration_run` 落库后经 spawn hook 后台执行；立即返回 `orchestration_id`。
//! 真正执行在 `agent::exec::orchestration`，本模块不依赖 `agent` crate。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

const MAX_STEPS: usize = 8;

/// One step in an orchestration.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct OrchestrationStepArgs {
    /// Role name (display and temporary role injection).
    pub role: String,
    pub prompt: String,
    /// Existing agent id; omit for a temporary role (uses parent credentials).
    #[serde(default)]
    pub agent_id: Option<String>,
}

/// Arguments for `orchestration_run`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct OrchestrationRunArgs {
    pub goal: String,
    /// Serial steps (1–8).
    pub steps: Vec<OrchestrationStepArgs>,
}

/// Arguments for `orchestration_status`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct OrchestrationStatusArgs {
    /// Orchestration id returned by `orchestration_run`.
    pub orchestration_id: String,
}

/// 向注册表登记编排工具（toolset=`multi_agent`）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "orchestration_run".to_string(),
        toolset: "multi_agent".to_string(),
        description: "Async serial multi-agent orchestration. Returns orchestration_id; poll with orchestration_status. Use delegate for parallel one-shot subtasks."
            .to_string(),
        schema: schema_for_args::<OrchestrationRunArgs>(),
        check_fn: None,
        icon: "git-branch",
            ..ToolEntry::lifecycle_defaults()
    });
    registry.register(ToolEntry {
        name: "orchestration_status".to_string(),
        toolset: "multi_agent".to_string(),
        description:
            "Query status and step outputs of an orchestration started by orchestration_run."
                .to_string(),
        schema: schema_for_args::<OrchestrationStatusArgs>(),
        check_fn: None,
        icon: "list-checks",
        ..ToolEntry::lifecycle_defaults()
    });
}

/// 创建编排并触发后台执行；立即返回 queued JSON。
pub fn dispatch_run(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    if !home::can_spawn_nested() {
        anyhow::bail!(
            "spawn depth limit reached (depth {} >= max {})",
            home::current_spawn_depth(),
            home::effective_max_spawn_depth()
        );
    }
    let parsed: OrchestrationRunArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("orchestration_run 参数无效: {e}"))?;
    validate_run_args(&parsed)?;

    let steps: Vec<_> = parsed
        .steps
        .into_iter()
        .map(|s| orchestration::NewOrchestrationStep {
            role: s.role.trim().to_string(),
            agent_id: s
                .agent_id
                .as_ref()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty()),
            prompt: s.prompt.trim().to_string(),
        })
        .collect();

    let db = orchestration::OrchestrationDb::open_default()?;
    let orchestration_id = db.create(orchestration::NewOrchestration {
        parent_agent_id: ctx.memory.agent_id.clone(),
        session_id: Some(ctx.session_id.clone()),
        goal: parsed.goal.trim().to_string(),
        steps,
        provider: ctx.chat_provider.clone(),
        model: ctx.chat_model.clone(),
        api_key: ctx.chat_api_key.clone(),
        base_url: ctx.chat_base_url.clone(),
    })?;

    let spawn_req = orchestration::OrchestrationSpawnRequest {
        orchestration_id: orchestration_id.clone(),
        parent_agent_id: ctx.memory.agent_id.clone(),
        provider: ctx.chat_provider.clone(),
        model: ctx.chat_model.clone(),
        api_key: ctx.chat_api_key.clone(),
        base_url: ctx.chat_base_url.clone(),
        chat_targets: ctx.chat_targets.clone(),
        caller_depth: home::current_spawn_depth(),
        max_spawn_depth: home::effective_max_spawn_depth(),
        allow_reclaim: false,
    };
    if let Some(spawner) = ctx.orchestration_spawner.as_ref() {
        spawner(spawn_req);
    }

    Ok(serde_json::json!({
        "orchestration_id": orchestration_id,
        "status": "queued",
    })
    .to_string())
}

fn validate_run_args(parsed: &OrchestrationRunArgs) -> anyhow::Result<()> {
    let goal = parsed.goal.trim();
    if goal.is_empty() {
        anyhow::bail!("orchestration_run 需要非空 goal");
    }
    if parsed.steps.is_empty() || parsed.steps.len() > MAX_STEPS {
        anyhow::bail!("orchestration_run steps 长度须为 1..={MAX_STEPS}");
    }
    for (i, step) in parsed.steps.iter().enumerate() {
        if step.role.trim().is_empty() || step.prompt.trim().is_empty() {
            anyhow::bail!("orchestration_run steps[{i}] 需要非空 role 与 prompt");
        }
    }
    Ok(())
}

/// 查询编排与步骤状态。
pub fn dispatch_status(args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: OrchestrationStatusArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("orchestration_status 参数无效: {e}"))?;
    let id = parsed.orchestration_id.trim();
    if id.is_empty() {
        anyhow::bail!("orchestration_status 需要 orchestration_id");
    }

    let db = orchestration::OrchestrationDb::open_default()?;
    let orch = db
        .get(id)?
        .ok_or_else(|| anyhow::anyhow!("orchestration 不存在: {id}"))?;
    let steps = db.list_steps(id)?;

    let step_json: Vec<_> = steps
        .into_iter()
        .map(|s| {
            let output = s.output.map(|o| {
                if o.len() > 8 * 1024 {
                    common::truncate_tool_result(&o, 8 * 1024)
                } else {
                    o
                }
            });
            serde_json::json!({
                "seq": s.seq,
                "role": s.role,
                "agent_id": s.agent_id,
                "status": s.status,
                "output": output,
                "error": s.error,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "orchestration_id": orch.id,
        "parent_agent_id": orch.parent_agent_id,
        "goal": orch.goal,
        "status": orch.status,
        "error": orch.error,
        "result_summary": orch.result_summary,
        "created_at": orch.created_at,
        "updated_at": orch.updated_at,
        "finished_at": orch.finished_at,
        "steps": step_json,
    })
    .to_string())
}

/// 本模块统一入口：`orchestration_run` / `orchestration_status`。
fn handle(
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    match name {
        "orchestration_run" => dispatch_run(ctx, args),
        "orchestration_status" => dispatch_status(args),
        other => anyhow::bail!("未知编排工具: {other}"),
    }
}

crate::submit_builtin_tool! {
    register: register,
    names: ["orchestration_run", "orchestration_status"],
    sync_named: handle,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_empty_steps() {
        let parsed = OrchestrationRunArgs {
            goal: "g".into(),
            steps: vec![],
        };
        let err = validate_run_args(&parsed).unwrap_err().to_string();
        assert!(err.contains("1..="));
    }

    #[test]
    fn reject_too_many_steps() {
        let steps = (0..9)
            .map(|i| OrchestrationStepArgs {
                role: format!("r{i}"),
                prompt: "p".into(),
                agent_id: None,
            })
            .collect();
        let parsed = OrchestrationRunArgs {
            goal: "g".into(),
            steps,
        };
        assert!(validate_run_args(&parsed).is_err());
    }

    #[test]
    fn accept_valid_steps() {
        let parsed = OrchestrationRunArgs {
            goal: "写周报".into(),
            steps: vec![OrchestrationStepArgs {
                role: "writer".into(),
                prompt: "起草".into(),
                agent_id: None,
            }],
        };
        assert!(validate_run_args(&parsed).is_ok());
    }

    #[test]
    fn status_requires_id() {
        let err = dispatch_status(&serde_json::json!({ "orchestration_id": "  " }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("orchestration_id"));
    }
}
