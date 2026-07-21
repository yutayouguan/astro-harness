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
    /// Optional model: `provider:model_id` (e.g. `claude:claude-sonnet-4-5`).
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct TeamRunArgs {
    pub team_id: String,
    pub goal: String,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub member_id: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub tasks: Option<Vec<String>>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "team".to_string(),
        toolset: "orchestrate".to_string(),
        description: "Manage persisted Teams. action=list|create|run. \
list: optional team_id; create: id+name+members; run: team_id+goal (mode=coordinate|route|broadcast|tasks)."
            .to_string(),
        schema: schema_for_args::<TeamArgs>(),
        check_fn: None,
        icon: "users",
        ..ToolEntry::lifecycle_defaults()
    });
}

/// Unified args for `team` (fields used depend on action).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TeamArgs {
    /// `list` | `create` | `run`.
    pub action: String,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub leader_agent_id: Option<String>,
    #[serde(default)]
    pub members: Option<Vec<TeamMemberArgs>>,
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub member_id: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub tasks: Option<Vec<String>>,
}

pub fn dispatch_list(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: TeamArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("team list 参数无效: {e}"))?;
    if let Some(id) = parsed
        .team_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let team = orchestration::load_team(&ctx.memory_dir, id)?;
        return Ok(serde_json::to_string_pretty(&team)?);
    }
    let teams = orchestration::list_teams(&ctx.memory_dir)?;
    Ok(serde_json::to_string_pretty(&teams)?)
}

pub fn dispatch_create(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: TeamArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("team create 参数无效: {e}"))?;
    let id = parsed
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("team create 需要 id"))?
        .to_string();
    let name = parsed
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("team create 需要 name"))?
        .to_string();
    let members = parsed
        .members
        .clone()
        .filter(|m| !m.is_empty())
        .ok_or_else(|| anyhow::anyhow!("team create 需要 members"))?;
    let mode = parsed
        .mode
        .as_deref()
        .map(orchestration::TeamMode::parse)
        .transpose()?
        .unwrap_or_default();
    let team = orchestration::TeamDefinition {
        id,
        name,
        mode,
        leader_agent_id: parsed
            .leader_agent_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        members: members
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
                model: m
                    .model
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
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

    let unified: TeamArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("team run 参数无效: {e}"))?;
    let parsed = TeamRunArgs {
        team_id: unified
            .team_id
            .unwrap_or_default()
            .trim()
            .to_string(),
        goal: unified.goal.unwrap_or_default().trim().to_string(),
        mode: unified.mode,
        member_id: unified.member_id,
        context: unified.context,
        tasks: unified.tasks,
    };
    let team_id = parsed.team_id.trim();
    if team_id.is_empty() {
        anyhow::bail!("team run 需要 team_id");
    }
    let goal = parsed.goal.trim();
    if goal.is_empty() {
        anyhow::bail!("team run 需要 goal");
    }

    let team = orchestration::load_team(&ctx.memory_dir, team_id)?;
    let mode = parsed
        .mode
        .as_deref()
        .map(orchestration::TeamMode::parse)
        .transpose()?
        .unwrap_or(team.mode);

    if mode == orchestration::TeamMode::Tasks {
        return run_tasks_mode(ctx, &team, &parsed, goal);
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
            model: member.model.clone(),
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

/// `tasks`：按任务列表串行委派；共享任务板把前序结果传给后续成员。
fn run_tasks_mode(
    ctx: &ToolContext<'_>,
    team: &orchestration::TeamDefinition,
    parsed: &TeamRunArgs,
    goal: &str,
) -> anyhow::Result<String> {
    let work = build_task_work(team, parsed.tasks.as_ref(), parsed.member_id.as_deref())?;
    let runner = ctx
        .delegate_runner
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no delegate runner configured"))?;

    let mut board: Vec<serde_json::Value> = Vec::new();
    let mut step_results = Vec::new();

    for (idx, item) in work.iter().enumerate() {
        let board_text = if board.is_empty() {
            "(empty — you are the first step)".to_string()
        } else {
            board
                .iter()
                .enumerate()
                .map(|(i, v)| format!("### Step {}\n{}", i + 1, v))
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        let mut ctx_body = member_context(team, item.member, parsed.context.as_deref());
        ctx_body.push_str("\n## Shared Task Board\n");
        ctx_body.push_str(&board_text);
        ctx_body.push('\n');
        ctx_body.push_str(&format!(
            "\n## Current Task ({}/{})\n{}\n",
            idx + 1,
            work.len(),
            item.task
        ));

        let req = delegate::DelegateRunRequest {
            parent_agent_id: ctx.memory.agent_id.clone(),
            parent_session_id: ctx.session_id.clone(),
            provider: ctx.chat_provider.clone(),
            model: ctx.chat_model.clone(),
            api_key: ctx.chat_api_key.clone(),
            base_url: ctx.chat_base_url.clone(),
            chat_targets: ctx.chat_targets.clone(),
            max_concurrent: 1,
            tasks: vec![delegate::DelegateTaskSpec {
                goal: format!(
                    "You are a member of Team \"{}\" running in Tasks mode.\n\nMember role: {}\nMember id: {}\n\nOverall goal:\n{}\n\nFocus on the Current Task below; use the Shared Task Board for prior results.",
                    team.name, item.member.role, item.member.id, goal
                ),
                context: ctx_body,
                role: delegate::DelegateRole::Leaf,
                toolsets: item.member.toolsets.clone(),
                max_iterations: None,
                model: item.member.model.clone(),
            }],
            caller_depth: home::current_spawn_depth(),
            max_spawn_depth: home::effective_max_spawn_depth(),
            project_root: ctx.project_root.clone(),
            hook_bus: ctx.hook_bus.clone(),
        };
        let raw = runner(req)?;
        let parsed_result = serde_json::from_str::<serde_json::Value>(&raw)
            .unwrap_or(serde_json::Value::String(raw.clone()));
        board.push(serde_json::json!({
            "member_id": item.member.id,
            "role": item.member.role,
            "task": item.task,
            "result": &parsed_result,
        }));
        step_results.push(serde_json::json!({
            "step": idx + 1,
            "member_id": item.member.id,
            "role": item.member.role,
            "task": item.task,
            "result": parsed_result,
        }));
    }

    Ok(serde_json::json!({
        "team_id": team.id,
        "team_name": team.name,
        "mode": orchestration::TeamMode::Tasks,
        "respond_directly": false,
        "steps": step_results,
        "shared_board": board,
    })
    .to_string())
}

struct TaskWorkItem<'a> {
    member: &'a orchestration::TeamMember,
    task: String,
}

fn build_task_work<'a>(
    team: &'a orchestration::TeamDefinition,
    tasks: Option<&Vec<String>>,
    member_id: Option<&str>,
) -> anyhow::Result<Vec<TaskWorkItem<'a>>> {
    let members = select_members(team, orchestration::TeamMode::Tasks, member_id)?;
    if members.is_empty() {
        anyhow::bail!("tasks 模式需要至少一个成员");
    }
    let explicit: Vec<String> = tasks
        .map(|v| {
            v.iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    if explicit.is_empty() {
        // 缺省：每个成员一步，任务=按角色推进整体目标
        return Ok(members
            .into_iter()
            .map(|m| TaskWorkItem {
                task: format!("From your role ({}), advance the overall goal.", m.role),
                member: m,
            })
            .collect());
    }

    // 显式任务：按成员顺序循环分配
    let mut out = Vec::with_capacity(explicit.len());
    for (i, task) in explicit.into_iter().enumerate() {
        let member = members[i % members.len()];
        out.push(TaskWorkItem { member, task });
    }
    Ok(out)
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
        orchestration::TeamMode::Tasks => {
            // tasks：未指定 member_id 时全体按序参与；指定则仅该成员循环执行任务
            Ok(team.members.iter().collect())
        }
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
        if let Some(model) = &m.model {
            ctx.push_str(&format!(" [model: {model}]"));
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

/// 本模块统一入口：按 `action` 分发。
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
        .ok_or_else(|| anyhow::anyhow!("team 需要 action=list|create|run"))?
        .to_ascii_lowercase();
    match action.as_str() {
        "list" => dispatch_list(ctx, args),
        "create" => dispatch_create(ctx, args),
        "run" => dispatch_run(ctx, args),
        other => anyhow::bail!("未知 team action: {other}（应为 list|create|run）"),
    }
}

crate::submit_builtin_tool! {
    register: register,
    names: ["team"],
    sync_named: handle,
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
                    model: None,
                },
                orchestration::TeamMember {
                    id: "b".into(),
                    role: "B".into(),
                    description: None,
                    agent_id: None,
                    toolsets: None,
                    model: Some("claude:opus".into()),
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

    #[test]
    fn tasks_default_one_step_per_member() {
        let team = sample_team();
        let work = build_task_work(&team, None, None).unwrap();
        assert_eq!(work.len(), 2);
        assert_eq!(work[0].member.id, "a");
        assert_eq!(work[1].member.id, "b");
    }

    #[test]
    fn tasks_explicit_round_robin() {
        let team = sample_team();
        let tasks = vec!["t1".into(), "t2".into(), "t3".into()];
        let work = build_task_work(&team, Some(&tasks), None).unwrap();
        assert_eq!(work.len(), 3);
        assert_eq!(work[0].member.id, "a");
        assert_eq!(work[1].member.id, "b");
        assert_eq!(work[2].member.id, "a");
        assert_eq!(work[2].task, "t3");
    }

    #[test]
    fn tasks_member_id_restricts_assignee() {
        let team = sample_team();
        let tasks = vec!["only-b".into()];
        let work = build_task_work(&team, Some(&tasks), Some("b")).unwrap();
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].member.id, "b");
    }
}
