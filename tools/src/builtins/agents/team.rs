//! Agno-inspired Team tools.
//!
//! The first slice keeps execution on top of the existing delegate runtime:
//! members are constrained by a persisted TeamDefinition, while route/broadcast
//! are mapped to one or many delegate tasks.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TeamMemberArgs {
    pub id: String,
    pub role: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub toolsets: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TeamCreateArgs {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub leader_agent_id: Option<String>,
    pub members: Vec<TeamMemberArgs>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TeamRunArgs {
    pub team_id: String,
    pub goal: String,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub member_id: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TeamIdArgs {
    #[serde(default)]
    pub team_id: Option<String>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "team_list".to_string(),
        toolset: "multi_agent".to_string(),
        description: "List persisted Team definitions from ~/.astro/teams, or inspect one team by team_id."
            .to_string(),
        schema: schema_for_args::<TeamIdArgs>(),
        check_fn: None,
        icon: "users",
    });
    registry.register(ToolEntry {
        name: "team_create".to_string(),
        toolset: "multi_agent".to_string(),
        description: "Create or replace a persisted Team definition. Modes: coordinate, route, broadcast, tasks (tasks is stored but not executable yet)."
            .to_string(),
        schema: schema_for_args::<TeamCreateArgs>(),
        check_fn: None,
        icon: "users-plus",
    });
    registry.register(ToolEntry {
        name: "team_run".to_string(),
        toolset: "multi_agent".to_string(),
        description: "Run a persisted Team using coordinate, route, or broadcast. Uses the existing delegate runtime; route requires member_id when the team has multiple members."
            .to_string(),
        schema: schema_for_args::<TeamRunArgs>(),
        check_fn: None,
        icon: "network",
    });
}

pub fn dispatch_list(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: TeamIdArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("team_list 参数无效: {e}"))?;
    if let Some(id) = parsed.team_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let team = orchestration::load_team(&ctx.memory_dir, id)?;
        return Ok(serde_json::to_string_pretty(&team)?);
    }
    let teams = orchestration::list_teams(&ctx.memory_dir)?;
    Ok(serde_json::to_string_pretty(&teams)?)
}

pub fn dispatch_create(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: TeamCreateArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("team_create 参数无效: {e}"))?;
    let mode = parsed
        .mode
        .as_deref()
        .map(orchestration::TeamMode::parse)
        .transpose()?
        .unwrap_or_default();
    let team = orchestration::TeamDefinition {
        id: parsed.id.trim().to_string(),
        name: parsed.name.trim().to_string(),
        mode,
        leader_agent_id: parsed
            .leader_agent_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        members: parsed
            .members
            .into_iter()
            .map(|m| orchestration::TeamMember {
                id: m.id.trim().to_string(),
                role: m.role.trim().to_string(),
                description: m
                    .description
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                agent_id: m
                    .agent_id
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                toolsets: m.toolsets,
            })
            .collect(),
    };
    orchestration::save_team(&ctx.memory_dir, &team)?;
    Ok(serde_json::json!({
        "status": "saved",
        "team_id": team.id,
        "path": orchestration::team_path(&ctx.memory_dir, &team.id),
        "mode": team.mode,
        "members": team.members.len(),
    })
    .to_string())
}

pub fn dispatch_run(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    if !home::can_spawn_nested() {
        anyhow::bail!(
            "spawn depth limit reached (depth {} >= max {})",
            home::current_spawn_depth(),
            home::effective_max_spawn_depth()
        );
    }

    let parsed: TeamRunArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("team_run 参数无效: {e}"))?;
    let team_id = parsed.team_id.trim();
    if team_id.is_empty() {
        anyhow::bail!("team_run 需要 team_id");
    }
    let goal = parsed.goal.trim();
    if goal.is_empty() {
        anyhow::bail!("team_run 需要 goal");
    }

    let team = orchestration::load_team(&ctx.memory_dir, team_id)?;
    let mode = parsed
        .mode
        .as_deref()
        .map(orchestration::TeamMode::parse)
        .transpose()?
        .unwrap_or(team.mode);
    if mode == orchestration::TeamMode::Tasks {
        anyhow::bail!("team_run 暂未实现 tasks 模式；请使用 coordinate/route/broadcast");
    }

    let members = select_members(&team, mode, parsed.member_id.as_deref())?;
    let tasks: Vec<_> = members
        .iter()
        .map(|member| delegate::DelegateTaskSpec {
            goal: member_goal(&team, member, goal, mode),
            context: member_context(&team, member, parsed.context.as_deref()),
            role: delegate::DelegateRole::Leaf,
            toolsets: member.toolsets.clone(),
            max_iterations: None,
        })
        .collect();

    let runner = ctx
        .delegate_runner
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no delegate runner configured"))?;
    let req = delegate::DelegateRunRequest {
        parent_agent_id: ctx.memory.agent_id.clone(),
        parent_session_id: ctx.session_id.clone(),
        provider: ctx.chat_provider.clone(),
        model: ctx.chat_model.clone(),
        api_key: ctx.chat_api_key.clone(),
        base_url: ctx.chat_base_url.clone(),
        chat_targets: ctx.chat_targets.clone(),
        max_concurrent: if mode == orchestration::TeamMode::Broadcast {
            members.len().clamp(1, 8)
        } else {
            1
        },
        tasks,
        caller_depth: home::current_spawn_depth(),
        max_spawn_depth: home::effective_max_spawn_depth(),
        project_root: ctx.project_root.clone(),
        hook_bus: ctx.hook_bus.clone(),
    };
    let delegate_result = runner(req)?;

    Ok(serde_json::json!({
        "team_id": team.id,
        "team_name": team.name,
        "mode": mode,
        "respond_directly": mode == orchestration::TeamMode::Route,
        "members": members.iter().map(|m| serde_json::json!({
            "id": m.id,
            "role": m.role,
            "agent_id": m.agent_id,
        })).collect::<Vec<_>>(),
        "result": serde_json::from_str::<serde_json::Value>(&delegate_result)
            .unwrap_or(serde_json::Value::String(delegate_result)),
    })
    .to_string())
}

fn select_members<'a>(
    team: &'a orchestration::TeamDefinition,
    mode: orchestration::TeamMode,
    member_id: Option<&str>,
) -> anyhow::Result<Vec<&'a orchestration::TeamMember>> {
    let selected_id = member_id.map(str::trim).filter(|s| !s.is_empty());
    if let Some(id) = selected_id {
        let member = team
            .members
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| anyhow::anyhow!("team member not found: {id}"))?;
        return Ok(vec![member]);
    }
    match mode {
        orchestration::TeamMode::Route if team.members.len() != 1 => {
            anyhow::bail!("route 模式下 team 有多个成员时必须指定 member_id")
        }
        orchestration::TeamMode::Route => Ok(vec![&team.members[0]]),
        orchestration::TeamMode::Coordinate | orchestration::TeamMode::Broadcast => {
            Ok(team.members.iter().collect())
        }
        orchestration::TeamMode::Tasks => anyhow::bail!("tasks mode is not executable"),
    }
}

fn member_goal(
    team: &orchestration::TeamDefinition,
    member: &orchestration::TeamMember,
    goal: &str,
    mode: orchestration::TeamMode,
) -> String {
    format!(
        "You are a member of Team \"{}\" running in {:?} mode.\n\nMember role: {}\nMember id: {}\n\nTeam goal:\n{}",
        team.name, mode, member.role, member.id, goal
    )
}

fn member_context(
    team: &orchestration::TeamDefinition,
    member: &orchestration::TeamMember,
    extra_context: Option<&str>,
) -> String {
    let mut ctx = String::new();
    ctx.push_str("## Team Members\n");
    for m in &team.members {
        ctx.push_str(&format!("- {}: {}", m.id, m.role));
        if let Some(desc) = &m.description {
            ctx.push_str(&format!(" — {desc}"));
        }
        if let Some(agent_id) = &m.agent_id {
            ctx.push_str(&format!(" (agent_id: {agent_id})"));
        }
        ctx.push('\n');
    }
    if let Some(desc) = &member.description {
        ctx.push_str("\n## Your Member Description\n");
        ctx.push_str(desc);
        ctx.push('\n');
    }
    if let Some(extra) = extra_context.map(str::trim).filter(|s| !s.is_empty()) {
        ctx.push_str("\n## Caller Context\n");
        ctx.push_str(extra);
        ctx.push('\n');
    }
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_team() -> orchestration::TeamDefinition {
        orchestration::TeamDefinition {
            id: "t".into(),
            name: "T".into(),
            mode: orchestration::TeamMode::Coordinate,
            leader_agent_id: None,
            members: vec![
                orchestration::TeamMember {
                    id: "a".into(),
                    role: "A".into(),
                    description: None,
                    agent_id: None,
                    toolsets: None,
                },
                orchestration::TeamMember {
                    id: "b".into(),
                    role: "B".into(),
                    description: None,
                    agent_id: None,
                    toolsets: None,
                },
            ],
        }
    }

    #[test]
    fn route_requires_member_for_multi_member_team() {
        let team = sample_team();
        assert!(select_members(&team, orchestration::TeamMode::Route, None).is_err());
        let selected = select_members(&team, orchestration::TeamMode::Route, Some("a")).unwrap();
        assert_eq!(selected[0].id, "a");
    }

    #[test]
    fn broadcast_selects_all_members() {
        let team = sample_team();
        let selected = select_members(&team, orchestration::TeamMode::Broadcast, None).unwrap();
        assert_eq!(selected.len(), 2);
    }
}
