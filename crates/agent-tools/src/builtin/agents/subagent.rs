//! Codex-style first-class subagent thread tools.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct SpawnAgentArgs {
    #[serde(default, alias = "message")]
    task: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    model_reasoning_effort: Option<String>,
    /// none|all|N. Defaults to all parent user/assistant messages.
    #[serde(default)]
    fork_turns: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct ListAgentsArgs {
    #[serde(default)]
    include_closed: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct ThreadIdArgs {
    #[serde(default, alias = "target")]
    thread_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct SendMessageArgs {
    #[serde(default, alias = "target")]
    thread_id: Option<String>,
    message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct WaitAgentsArgs {
    #[serde(default)]
    thread_ids: Vec<String>,
    #[serde(default, alias = "thread_id")]
    target: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

pub fn register(registry: &mut ToolRegistry) {
    let lifecycle = ToolEntry::lifecycle_defaults;
    registry.register(ToolEntry {
        name: "spawn_agent".into(),
        toolset: "subagents".into(),
        description: "Spawn a first-class subagent thread. The child has its own context and tool loop; manage it with list/read/wait/send/interrupt/close.".into(),
        schema: schema_for_args::<SpawnAgentArgs>(),
        check_fn: None,
        icon: "bot",
        ..lifecycle()
    });
    registry.register(ToolEntry {
        name: "list_agents".into(),
        toolset: "subagents".into(),
        description: "List subagent threads owned by the current parent session.".into(),
        schema: schema_for_args::<ListAgentsArgs>(),
        check_fn: None,
        icon: "list-tree",
        ..lifecycle()
    });
    registry.register(ToolEntry {
        name: "read_agent".into(),
        toolset: "subagents".into(),
        description: "Inspect one subagent thread and its conversation messages.".into(),
        schema: schema_for_args::<ThreadIdArgs>(),
        check_fn: None,
        icon: "messages-square",
        ..lifecycle()
    });
    registry.register(ToolEntry {
        name: "send_message_to_agent".into(),
        toolset: "subagents".into(),
        description:
            "Steer a live subagent by queueing a follow-up message at the next model boundary."
                .into(),
        schema: schema_for_args::<SendMessageArgs>(),
        check_fn: None,
        icon: "send",
        ..lifecycle()
    });
    registry.register(ToolEntry {
        name: "followup_task".into(),
        toolset: "subagents".into(),
        description: "Codex-compatible alias: steer a subagent thread with a follow-up task."
            .into(),
        schema: schema_for_args::<SendMessageArgs>(),
        check_fn: None,
        icon: "send",
        ..lifecycle()
    });
    registry.register(ToolEntry {
        name: "send_message".into(),
        toolset: "subagents".into(),
        description: "Codex-compatible alias: send a message to an existing subagent thread."
            .into(),
        schema: schema_for_args::<SendMessageArgs>(),
        check_fn: None,
        icon: "send",
        ..lifecycle()
    });
    registry.register(ToolEntry {
        name: "wait_agents".into(),
        toolset: "subagents".into(),
        description: "Wait for requested subagent threads to finish their current turns and return their summaries.".into(),
        schema: schema_for_args::<WaitAgentsArgs>(),
        check_fn: None,
        icon: "clock",
        ..lifecycle()
    });
    registry.register(ToolEntry {
        name: "wait_agent".into(),
        toolset: "subagents".into(),
        description: "Codex-compatible alias: wait for one target thread, or all active threads when target is omitted.".into(),
        schema: schema_for_args::<WaitAgentsArgs>(),
        check_fn: None,
        icon: "clock",
        ..lifecycle()
    });
    registry.register(ToolEntry {
        name: "interrupt_agent".into(),
        toolset: "subagents".into(),
        description: "Interrupt the currently running turn of a subagent thread.".into(),
        schema: schema_for_args::<ThreadIdArgs>(),
        check_fn: None,
        icon: "circle-stop",
        ..lifecycle()
    });
    registry.register(ToolEntry {
        name: "close_agent".into(),
        toolset: "subagents".into(),
        description: "Close a subagent thread and release its live controls.".into(),
        schema: schema_for_args::<ThreadIdArgs>(),
        check_fn: None,
        icon: "x",
        ..lifecycle()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: [
        "spawn_agent",
        "list_agents",
        "read_agent",
        "send_message_to_agent",
        "followup_task",
        "send_message",
        "wait_agents",
        "wait_agent",
        "interrupt_agent",
        "close_agent"
    ],
    async_named: handle,
}

async fn handle(
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let dispatch = ctx
        .execution
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no agent thread dispatch configured"))?;
    match name {
        "spawn_agent" => {
            let parsed: SpawnAgentArgs = parse(name, args)?;
            let task = parsed.task.as_deref().unwrap_or_default().trim();
            if task.is_empty() {
                anyhow::bail!("spawn_agent requires a non-empty task");
            }
            let project_root = ctx.project_root.as_deref();
            let settings = subagents::load_agents_settings(&ctx.memory_dir, project_root);
            if !settings.enabled {
                anyhow::bail!("subagent threads are disabled by [agents].enabled");
            }
            let catalog = subagents::load_agent_catalog(&ctx.memory_dir, project_root);
            let agent_name = parsed
                .agent
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("default");
            let parent_model = format!("{}:{}", ctx.credentials.provider, ctx.credentials.model);
            let parent_sandbox = current_sandbox_mode(ctx);
            let resolved = subagents::resolve_agent(
                &catalog,
                &settings,
                agent_name,
                parsed.model.as_deref(),
                parsed.model_reasoning_effort.as_deref(),
                Some(&parent_model),
                Some(&parent_sandbox),
            )?;
            let active =
                subagents::AgentThreadStore::open_default()?.count_active(&ctx.session_id)?;
            if active >= settings.max_concurrent_threads_per_session {
                anyhow::bail!(
                    "subagent concurrency limit reached ({active}/{})",
                    settings.max_concurrent_threads_per_session
                );
            }
            let definition = resolved.definition;
            let request = subagents::SpawnAgentRequest {
                parent_session_id: ctx.session_id.clone(),
                parent_agent_id: ctx.agent_id(),
                task: task.to_string(),
                agent_name: definition.name,
                developer_instructions: definition.developer_instructions,
                context_snapshot: fork_context(ctx, parsed.fork_turns.as_deref())?,
                model: resolved.model,
                model_reasoning_effort: resolved.model_reasoning_effort,
                sandbox_mode: resolved.sandbox_mode,
                mcp_servers: definition.mcp_servers,
                skills_config: definition.skills.config,
                chat_targets: ctx.chat_targets.to_vec(),
                project_root: ctx.project_root.clone(),
                hook_bus: ctx.hook_bus.clone(),
                interrupt_message: settings.interrupt_message,
            };
            Ok(serde_json::to_string(
                &dispatch.spawn_agent(request).await?,
            )?)
        }
        "list_agents" => {
            let parsed: ListAgentsArgs = parse(name, args)?;
            Ok(serde_json::to_string(
                &dispatch
                    .list_agents(subagents::ListAgentThreadsRequest {
                        parent_session_id: ctx.session_id.clone(),
                        include_closed: parsed.include_closed.unwrap_or(false),
                    })
                    .await?,
            )?)
        }
        "read_agent" => {
            let parsed: ThreadIdArgs = parse(name, args)?;
            let (thread, messages) = dispatch
                .read_agent(subagents::ReadAgentThreadRequest {
                    parent_session_id: ctx.session_id.clone(),
                    thread_id: parsed.thread_id()?,
                })
                .await?;
            Ok(serde_json::json!({ "thread": thread, "messages": messages }).to_string())
        }
        "send_message_to_agent" | "followup_task" | "send_message" => {
            let parsed: SendMessageArgs = parse(name, args)?;
            if parsed.message.trim().is_empty() {
                anyhow::bail!("send_message_to_agent requires a non-empty message");
            }
            Ok(serde_json::to_string(
                &dispatch
                    .send_message(subagents::SendAgentMessageRequest {
                        parent_session_id: ctx.session_id.clone(),
                        thread_id: parsed.thread_id()?,
                        message: parsed.message.trim().to_string(),
                    })
                    .await?,
            )?)
        }
        "wait_agents" | "wait_agent" => {
            let parsed: WaitAgentsArgs = parse(name, args)?;
            let requested_ids = parsed.requested_ids();
            let ids = if requested_ids.is_empty() {
                dispatch
                    .list_agents(subagents::ListAgentThreadsRequest {
                        parent_session_id: ctx.session_id.clone(),
                        include_closed: false,
                    })
                    .await?
                    .into_iter()
                    .filter(|thread| {
                        matches!(
                            thread.status,
                            subagents::AgentThreadStatus::Pending
                                | subagents::AgentThreadStatus::Running
                        )
                    })
                    .map(|thread| thread.id)
                    .collect()
            } else {
                requested_ids
            };
            Ok(serde_json::to_string(
                &dispatch
                    .wait_agents(subagents::WaitAgentThreadsRequest {
                        parent_session_id: ctx.session_id.clone(),
                        thread_ids: ids,
                        timeout_ms: parsed.timeout_ms.unwrap_or(120_000).clamp(0, 600_000),
                    })
                    .await?,
            )?)
        }
        "interrupt_agent" => {
            let parsed: ThreadIdArgs = parse(name, args)?;
            Ok(serde_json::to_string(
                &dispatch
                    .interrupt_agent(subagents::InterruptAgentRequest {
                        parent_session_id: ctx.session_id.clone(),
                        thread_id: parsed.thread_id()?,
                    })
                    .await?,
            )?)
        }
        "close_agent" => {
            let parsed: ThreadIdArgs = parse(name, args)?;
            Ok(serde_json::to_string(
                &dispatch
                    .close_agent(subagents::CloseAgentRequest {
                        parent_session_id: ctx.session_id.clone(),
                        thread_id: parsed.thread_id()?,
                    })
                    .await?,
            )?)
        }
        _ => anyhow::bail!("unknown agent thread tool: {name}"),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(name: &str, args: &serde_json::Value) -> anyhow::Result<T> {
    serde_json::from_value(args.clone())
        .map_err(|error| anyhow::anyhow!("{name} arguments: {error}"))
}

fn non_empty_id(value: &str) -> anyhow::Result<&str> {
    let value = value.trim();
    if value.is_empty() {
        anyhow::bail!("thread_id cannot be empty");
    }
    Ok(value)
}

impl ThreadIdArgs {
    fn thread_id(&self) -> anyhow::Result<String> {
        Ok(non_empty_id(self.thread_id.as_deref().unwrap_or_default())?.to_string())
    }
}

impl SendMessageArgs {
    fn thread_id(&self) -> anyhow::Result<String> {
        Ok(non_empty_id(self.thread_id.as_deref().unwrap_or_default())?.to_string())
    }
}

impl WaitAgentsArgs {
    fn requested_ids(&self) -> Vec<String> {
        if !self.thread_ids.is_empty() {
            return self.thread_ids.clone();
        }
        self.target
            .as_deref()
            .map(str::trim)
            .filter(|target| !target.is_empty())
            .map(|target| vec![target.to_string()])
            .unwrap_or_default()
    }
}

fn current_sandbox_mode(ctx: &ToolContext<'_>) -> String {
    let profile = ctx.permission_profile.clone().unwrap_or_else(|| {
        memory::load_permission_settings(&ctx.memory_dir)
            .selection
            .profile_id
    });
    match profile.as_str() {
        types::READ_ONLY_PROFILE => "read-only".into(),
        types::WORKSPACE_PROFILE => "workspace-write".into(),
        types::DANGER_FULL_ACCESS_PROFILE => "danger-full-access".into(),
        other => other.to_string(),
    }
}

fn fork_context(ctx: &ToolContext<'_>, fork_turns: Option<&str>) -> anyhow::Result<String> {
    let mode = fork_turns
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("all");
    if mode.eq_ignore_ascii_case("none") {
        return Ok(String::new());
    }
    let mut messages: Vec<_> = ctx
        .sessions
        .get_messages(&ctx.session_id)?
        .into_iter()
        .filter(|message| matches!(message.role.as_str(), "user" | "assistant"))
        .collect();
    if !mode.eq_ignore_ascii_case("all") {
        let turns = mode
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("fork_turns must be none, all, or a positive integer"))?;
        if turns == 0 {
            return Ok(String::new());
        }
        let keep = turns.saturating_mul(2);
        if messages.len() > keep {
            messages.drain(..messages.len() - keep);
        }
    }
    let mut out = String::new();
    for message in messages {
        let content = message.content.unwrap_or_default();
        if content.trim().is_empty() {
            continue;
        }
        out.push_str(&format!(
            "## {}\n{}\n\n",
            message.role,
            types::truncate_chars(&content, 6_000)
        ));
        if out.len() >= 32_000 {
            break;
        }
    }
    Ok(types::truncate_chars(&out, 32_000))
}
