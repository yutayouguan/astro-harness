//! 多代理协作工具：将角色列表映射为串行编排并真执行。
//!
//! `goal` + `agents[]` → `orchestration_run` 等价步骤（临时角色）；
//! 即时并行子任务请用 `delegate` / `delegate_async`。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

const MAX_AGENTS: usize = 8;

/// `multi_agent` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MultiAgentArgs {
    /// 总体目标（不可为空）。
    pub goal: String,
    /// 各子代理的角色描述列表（1～8），按顺序串行执行。
    pub agents: Vec<String>,
}

/// 向注册表登记 `multi_agent` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "multi_agent".to_string(),
        toolset: "multi_agent".to_string(),
        description: "Start a real serial multi-agent run from goal + role list (maps to orchestration). Returns orchestration_id; poll with orchestration_status. For parallel one-shot subtasks use delegate / delegate_async."
            .to_string(),
        schema: schema_for_args::<MultiAgentArgs>(),
        check_fn: None,
        icon: "users",
    });
}

/// 将角色列表落库为编排并触发后台执行。
pub fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    if !memory::can_spawn_nested() {
        anyhow::bail!(
            "spawn depth limit reached (depth {} >= max {})",
            memory::current_spawn_depth(),
            memory::effective_max_spawn_depth()
        );
    }

    let parsed: MultiAgentArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("multi_agent 参数无效: {e}"))?;
    let goal = parsed.goal.trim();
    if goal.is_empty() {
        anyhow::bail!("multi_agent 需要 goal");
    }
    if parsed.agents.is_empty() || parsed.agents.len() > MAX_AGENTS {
        anyhow::bail!("multi_agent agents 长度须为 1..={MAX_AGENTS}");
    }

    let mut steps = Vec::with_capacity(parsed.agents.len());
    for (i, role) in parsed.agents.iter().enumerate() {
        let role = role.trim();
        if role.is_empty() {
            anyhow::bail!("multi_agent agents[{i}] 不能为空");
        }
        steps.push(memory::NewOrchestrationStep {
            role: role.to_string(),
            agent_id: None,
            prompt: format!("As {role}, help achieve the overall goal.\n\n## Goal\n{goal}"),
        });
    }

    let db = memory::OrchestrationDb::open_default()?;
    let orchestration_id = db.create(memory::NewOrchestration {
        parent_agent_id: ctx.memory.agent_id.clone(),
        session_id: Some(ctx.session_id.clone()),
        goal: goal.to_string(),
        steps,
        provider: ctx.chat_provider.clone(),
        model: ctx.chat_model.clone(),
        api_key: ctx.chat_api_key.clone(),
        base_url: ctx.chat_base_url.clone(),
    })?;

    let spawn_req = memory::OrchestrationSpawnRequest {
        orchestration_id: orchestration_id.clone(),
        parent_agent_id: ctx.memory.agent_id.clone(),
        provider: ctx.chat_provider.clone(),
        model: ctx.chat_model.clone(),
        api_key: ctx.chat_api_key.clone(),
        base_url: ctx.chat_base_url.clone(),
        chat_targets: ctx.chat_targets.clone(),
        caller_depth: memory::current_spawn_depth(),
        max_spawn_depth: memory::effective_max_spawn_depth(),
        allow_reclaim: false,
    };
    if let Some(spawner) = ctx.orchestration_spawner.as_ref() {
        spawner(spawn_req);
    }

    Ok(serde_json::json!({
        "orchestration_id": orchestration_id,
        "status": "queued",
        "via": "multi_agent",
    })
    .to_string())
}
