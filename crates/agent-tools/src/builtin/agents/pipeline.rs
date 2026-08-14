//! 多 Agent 串行流水线：单一 `pipeline`（action=run|status|team_*）。
//!
//! `run` 落库后经 spawn hook 后台执行；立即返回 `orchestration_id`。
//! 真正执行在 `agent::exec::orchestration`，本模块不依赖 `agent` crate。
//! 便捷形状：`agents: [role…]` 展开为临时角色 steps。
//! 持久 Team：`team_list` / `team_create` / `team_run`（实现见 `team` 模块）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::builtin::agents::team::{self, TeamMemberArgs};
use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

const MAX_STEPS: usize = 8;

/// One step in a pipeline.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PipelineStepArgs {
    /// Role name (display and temporary role injection).
    pub role: String,
    pub prompt: String,
    /// Existing agent id; omit for a temporary role (uses parent credentials).
    #[serde(default)]
    pub agent_id: Option<String>,
}

/// Arguments for the unified `pipeline` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PipelineArgs {
    /// `run` (default) | `status` | `team_list` | `team_create` | `team_run`.
    #[serde(default)]
    pub action: Option<String>,
    /// Overall goal (run / team_run).
    #[serde(default)]
    pub goal: Option<String>,
    /// Serial steps (1–8); preferred for run.
    #[serde(default)]
    pub steps: Option<Vec<PipelineStepArgs>>,
    /// Convenience: role name list → temporary steps (run).
    #[serde(default)]
    pub agents: Option<Vec<String>>,
    /// Orchestration id (status).
    #[serde(default)]
    pub orchestration_id: Option<String>,
    /// Team id (team_list detail / team_run).
    #[serde(default)]
    pub team_id: Option<String>,
    /// New team id (team_create).
    #[serde(default)]
    pub id: Option<String>,
    /// New team display name (team_create).
    #[serde(default)]
    pub name: Option<String>,
    /// Team mode: coordinate|route|broadcast|tasks.
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub leader_agent_id: Option<String>,
    #[serde(default)]
    pub members: Option<Vec<TeamMemberArgs>>,
    #[serde(default)]
    pub member_id: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub tasks: Option<Vec<String>>,
}

/// 向注册表登记 `pipeline`。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "pipeline".to_string(),
        toolset: "pipeline".to_string(),
        description: "Multi-agent orchestration AND team management. \
Pipeline: action=run (goal + steps[{role,prompt,agent_id?}] or agents=[role…]; returns orchestration_id), action=status. \
Teams: action=team_create (create a persisted team), team_list, team_run (team_id+goal; mode=coordinate|route|broadcast|tasks). \
Use subagent for parallel one-shot tasks; persona_create for durable assistants—not this tool."
            .to_string(),
        schema: schema_for_args::<PipelineArgs>(),
        check_fn: None,
        icon: "git-branch",
        ..ToolEntry::lifecycle_defaults().orchestrator_and_above()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["pipeline"],
    sync_named: handle,
}

fn handle(
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
        "run" => dispatch_run(ctx, args),
        "status" => dispatch_status(args),
        "team_list" => team::dispatch_list(ctx, &team_args_from_pipeline(args, "list")),
        "team_create" => team::dispatch_create(ctx, &team_args_from_pipeline(args, "create")),
        "team_run" => team::dispatch_run(ctx, &team_args_from_pipeline(args, "run")),
        other => anyhow::bail!(
            "未知 pipeline action: {other}（应为 run|status|team_list|team_create|team_run）"
        ),
    }
}

fn team_args_from_pipeline(args: &serde_json::Value, action: &str) -> serde_json::Value {
    let mut out = args.clone();
    if let Some(obj) = out.as_object_mut() {
        obj.insert("action".into(), serde_json::json!(action));
    } else {
        out = serde_json::json!({ "action": action });
    }
    out
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
    let parsed: PipelineArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("pipeline run 参数无效: {e}"))?;
    let goal = parsed.goal.as_deref().unwrap_or("").trim();
    if goal.is_empty() {
        anyhow::bail!("pipeline run 需要非空 goal");
    }

    let steps = resolve_steps(&parsed, goal)?;

    let db = orchestration::OrchestrationDb::open_default()?;
    let orchestration_id = db.create(orchestration::NewOrchestration {
        parent_agent_id: ctx.memory.agent_id.clone(),
        session_id: Some(ctx.session_id.clone()),
        goal: goal.to_string(),
        steps,
        provider: ctx.credentials.provider.clone(),
        model: ctx.credentials.model.clone(),
        api_key: ctx.credentials.api_key.clone(),
        base_url: ctx.credentials.base_url.clone(),
    })?;

    let spawn_req = orchestration::OrchestrationSpawnRequest {
        orchestration_id: orchestration_id.clone(),
        parent_agent_id: ctx.memory.agent_id.clone(),
        provider: ctx.credentials.provider.clone(),
        model: ctx.credentials.model.clone(),
        api_key: ctx.credentials.api_key.clone(),
        base_url: ctx.credentials.base_url.clone(),
        chat_targets: ctx.chat_targets.to_vec(),
        caller_depth: home::current_spawn_depth(),
        max_spawn_depth: home::effective_max_spawn_depth(),
        allow_reclaim: false,
    };
    if let Some(exec) = ctx.execution.as_ref() {
        exec.spawn_orchestration(spawn_req);
    }

    Ok(serde_json::json!({
        "orchestration_id": orchestration_id,
        "status": "queued",
    })
    .to_string())
}

fn resolve_steps(
    parsed: &PipelineArgs,
    goal: &str,
) -> anyhow::Result<Vec<orchestration::NewOrchestrationStep>> {
    if let Some(steps) = &parsed.steps {
        if steps.is_empty() || steps.len() > MAX_STEPS {
            anyhow::bail!("pipeline steps 长度须为 1..={MAX_STEPS}");
        }
        let mut out = Vec::with_capacity(steps.len());
        for (i, step) in steps.iter().enumerate() {
            if step.role.trim().is_empty() || step.prompt.trim().is_empty() {
                anyhow::bail!("pipeline steps[{i}] 需要非空 role 与 prompt");
            }
            out.push(orchestration::NewOrchestrationStep {
                role: step.role.trim().to_string(),
                agent_id: step
                    .agent_id
                    .as_ref()
                    .map(|a| a.trim().to_string())
                    .filter(|a| !a.is_empty()),
                prompt: step.prompt.trim().to_string(),
            });
        }
        return Ok(out);
    }

    let agents = parsed
        .agents
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("pipeline run 需要 steps 或 agents"))?;
    if agents.is_empty() || agents.len() > MAX_STEPS {
        anyhow::bail!("pipeline agents 长度须为 1..={MAX_STEPS}");
    }
    let mut out = Vec::with_capacity(agents.len());
    for (i, role) in agents.iter().enumerate() {
        let role = role.trim();
        if role.is_empty() {
            anyhow::bail!("pipeline agents[{i}] 不能为空");
        }
        out.push(orchestration::NewOrchestrationStep {
            role: role.to_string(),
            agent_id: None,
            prompt: format!("As {role}, help achieve the overall goal.\n\n## Goal\n{goal}"),
        });
    }
    Ok(out)
}

/// 查询编排与步骤状态。
pub fn dispatch_status(args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: PipelineArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("pipeline status 参数无效: {e}"))?;
    let id = parsed.orchestration_id.as_deref().unwrap_or("").trim();
    if id.is_empty() {
        anyhow::bail!("pipeline status 需要 orchestration_id");
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
                    types::truncate_tool_result(&o, 8 * 1024)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_expand_to_steps() {
        let parsed = PipelineArgs {
            action: None,
            goal: Some("写周报".into()),
            steps: None,
            agents: Some(vec!["writer".into(), "editor".into()]),
            orchestration_id: None,
            team_id: None,
            id: None,
            name: None,
            mode: None,
            leader_agent_id: None,
            members: None,
            member_id: None,
            context: None,
            tasks: None,
        };
        let steps = resolve_steps(&parsed, "写周报").unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].role, "writer");
    }

    #[test]
    fn reject_empty_agents() {
        let parsed = PipelineArgs {
            action: None,
            goal: Some("g".into()),
            steps: None,
            agents: Some(vec![]),
            orchestration_id: None,
            team_id: None,
            id: None,
            name: None,
            mode: None,
            leader_agent_id: None,
            members: None,
            member_id: None,
            context: None,
            tasks: None,
        };
        assert!(resolve_steps(&parsed, "g").is_err());
    }

    #[test]
    fn status_requires_id() {
        let err = dispatch_status(&serde_json::json!({ "orchestration_id": "  " }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("orchestration_id"));
    }
}
