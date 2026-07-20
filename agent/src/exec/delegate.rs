//! 同步真委派：并行 spawn 子 AgentLoop，摘要回父。
//!
//! 对齐 ephemeral 子任务语义：隔离会话、受限工具、可选 git worktree、阻塞至完成。

use std::collections::HashSet;
use std::path::PathBuf;

use common::truncate_chars;
use delegate::{
    create_task_worktree, find_git_root, resolve_project_root, DelegateRole, DelegateRunRequest,
    DelegateTaskSpec, WorktreeHandle,
};
use futures::StreamExt;
use home::default_memory_dir;
use providers::registry::ProviderRegistry;
use providers::streaming::Usage;
use providers::trait_::ProviderConfig;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::prompt::messages::to_provider_messages;
use crate::runtime::{AgentConfig, AgentLoop, TurnResult};
use crate::streaming::fallback::try_stream_completion_with_fallback;

const OUTPUT_TRUNCATE: usize = 8_000;

/// 同步执行委派（可在 `block_in_place` / 独立 runtime 中调用）。
pub fn run_delegate_blocking(req: DelegateRunRequest) -> anyhow::Result<String> {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(run_delegate(req)))
    } else {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        rt.block_on(run_delegate(req))
    }
}

/// 异步执行委派并返回合并 JSON 摘要。
pub async fn run_delegate(req: DelegateRunRequest) -> anyhow::Result<String> {
    if req.tasks.is_empty() {
        anyhow::bail!("delegate 需要至少一个任务");
    }
    if req.api_key.trim().is_empty() {
        anyhow::bail!("未配置 API Key，无法执行委派");
    }
    let parent_session_id = req.parent_session_id.clone();
    let max_concurrent = req.max_concurrent.clamp(1, 8);

    let mut join_set = JoinSet::new();
    let mut pending = Vec::new();
    for (idx, task) in req.tasks.clone().into_iter().enumerate() {
        pending.push((idx, task));
    }

    let mut results: Vec<(usize, serde_json::Value)> = Vec::new();
    let mut in_flight = 0usize;
    let mut iter = pending.into_iter();

    loop {
        while in_flight < max_concurrent {
            let Some((idx, task)) = iter.next() else {
                break;
            };
            let child_req = req_clone_creds(&req);
            join_set.spawn(async move {
                let out = run_one_child(child_req, task).await;
                (idx, out)
            });
            in_flight += 1;
        }

        let Some(joined) = join_set.join_next().await else {
            break;
        };
        in_flight = in_flight.saturating_sub(1);
        match joined {
            Ok((idx, Ok(v))) => results.push((idx, v)),
            Ok((idx, Err(e))) => results.push((
                idx,
                serde_json::json!({
                    "status": "failed",
                    "error": e.to_string(),
                }),
            )),
            Err(e) => results.push((
                results.len(),
                serde_json::json!({
                    "status": "failed",
                    "error": format!("join: {e}"),
                }),
            )),
        }
    }

    results.sort_by_key(|(i, _)| *i);
    let tasks: Vec<_> = results.into_iter().map(|(_, v)| v).collect();
    let any_ok = tasks
        .iter()
        .any(|t| t.get("status").and_then(|s| s.as_str()) == Some("ok"));
    Ok(serde_json::json!({
        "delegate": true,
        "parent_session_id": parent_session_id,
        "status": if any_ok { "done" } else { "failed" },
        "tasks": tasks,
    })
    .to_string())
}

fn req_clone_creds(req: &DelegateRunRequest) -> DelegateRunRequest {
    DelegateRunRequest {
        parent_agent_id: req.parent_agent_id.clone(),
        parent_session_id: req.parent_session_id.clone(),
        provider: req.provider.clone(),
        model: req.model.clone(),
        api_key: req.api_key.clone(),
        base_url: req.base_url.clone(),
        chat_targets: req.chat_targets.clone(),
        tasks: vec![],
        max_concurrent: req.max_concurrent,
        caller_depth: req.caller_depth,
        max_spawn_depth: req.max_spawn_depth,
        project_root: req.project_root.clone(),
        hook_bus: req.hook_bus.clone(),
    }
}

fn delegation_cfg() -> hooks::config::DelegationConfig {
    hooks::config::load_config_or_default().delegation
}

/// 子 Agent 独立预算（默认 `delegation.child_max_iterations` = 50，对齐 Hermes）。
fn child_max_rounds(task: &DelegateTaskSpec, cfg: &hooks::config::DelegationConfig) -> usize {
    task.max_iterations
        .unwrap_or(cfg.child_max_iterations)
        .clamp(1, 200)
}

/// 生效角色：orchestrator 仅在配置允许且仍可再嵌套时保留。
fn effective_role(
    requested: DelegateRole,
    depth_ctx: home::SpawnDepthCtx,
    cfg: &hooks::config::DelegationConfig,
) -> DelegateRole {
    if !cfg.orchestrator_enabled {
        return DelegateRole::Leaf;
    }
    if requested == DelegateRole::Orchestrator && !depth_ctx.is_leaf() {
        DelegateRole::Orchestrator
    } else {
        DelegateRole::Leaf
    }
}

/// 有效聊天目标：优先 `chat_targets`，否则由四字段合成。
fn effective_chat_targets(
    creds: &DelegateRunRequest,
    registry: &ProviderRegistry,
) -> Vec<common::ChatTarget> {
    if !creds.chat_targets.is_empty() {
        return creds.chat_targets.clone();
    }
    let backend = if creds.provider.trim().is_empty() {
        "openai".to_string()
    } else {
        creds.provider.clone()
    };
    let model = if creds.model.trim().is_empty() {
        registry
            .get(&backend)
            .map(|p| p.default_model().to_string())
            .unwrap_or_default()
    } else {
        creds.model.clone()
    };
    vec![common::ChatTarget {
        provider_id: backend.clone(),
        backend_id: backend,
        model,
        api_key: creds.api_key.clone(),
        base_url: creds.base_url.clone(),
    }]
}

async fn run_one_child(
    creds: DelegateRunRequest,
    task: DelegateTaskSpec,
) -> anyhow::Result<serde_json::Value> {
    let depth_ctx = home::SpawnDepthCtx::from_caller(creds.caller_depth, creds.max_spawn_depth);
    home::scope_spawn_depth(depth_ctx, run_one_child_inner(creds, task, depth_ctx)).await
}

async fn run_one_child_inner(
    creds: DelegateRunRequest,
    task: DelegateTaskSpec,
    depth_ctx: home::SpawnDepthCtx,
) -> anyhow::Result<serde_json::Value> {
    let cfg = delegation_cfg();
    let role = effective_role(task.role, depth_ctx, &cfg);
    let max_rounds = child_max_rounds(&task, &cfg);

    let mut worktree: Option<WorktreeHandle> = None;
    let mut project_root: Option<PathBuf> = None;

    if cfg.worktree {
        if let Some(repo) = resolve_git_repo_for_delegate(creds.project_root.as_deref()) {
            let tid = Uuid::new_v4().to_string();
            match create_task_worktree(&repo, &tid) {
                Ok(handle) => {
                    project_root = Some(handle.path().to_path_buf());
                    worktree = Some(handle);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "delegate worktree create failed; continuing without");
                }
            }
        }
    } else if let Some(p) = resolve_project_root(creds.project_root.as_deref()) {
        project_root = Some(p);
    }

    let memory_dir = default_memory_dir();
    let sid = Uuid::new_v4().to_string();
    let agent_id = creds.parent_agent_id.clone();

    let mut config = AgentConfig::with_defaults(memory_dir.clone());
    let ws = home::agent_workspace_dir(&memory_dir, &agent_id);
    if let Ok(soul) = std::fs::read_to_string(ws.join("SOUL.md")) {
        config.soul = soul;
    }
    config.multi_turn = max_rounds;
    config.max_turns = max_rounds.saturating_add(2);

    let mut agent = AgentLoop::with_session_id_for_agent(config, sid.clone(), &agent_id)?;
    agent.set_project_root(project_root.clone());
    apply_nested_agent_tool_strips(agent.tool_registry_mut(), depth_ctx, role);
    apply_toolsets_filter(agent.tool_registry_mut(), task.toolsets.as_deref());
    agent.set_chat_credentials(
        &creds.provider,
        &creds.model,
        &creds.api_key,
        &creds.base_url,
    );
    let registry = ProviderRegistry::new();
    let mut targets = effective_chat_targets(&creds, &registry);
    let mut member_temp: Option<f32> = None;
    if let Some(spec_str) = task
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        match common::ModelSpec::parse(spec_str) {
            Ok(spec) => {
                member_temp = spec.temperature;
                if let Some(primary) = targets.first_mut() {
                    *primary = spec.apply_to(primary);
                } else {
                    targets.push(spec.to_chat_target(&creds.api_key, &creds.base_url));
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, model = %spec_str, "delegate task model spec ignored");
            }
        }
    }
    agent.set_chat_targets(targets);
    if let Some(t) = member_temp {
        agent.set_model(
            common::ModelSpec::new(agent.chat_provider(), agent.chat_model()).with_temperature(t),
        );
    }

    fire_subagent_start(&creds, &task, role);

    let role_note = match role {
        DelegateRole::Leaf => {
            "You are a leaf sub-agent: do NOT call delegate / orchestration tools; complete the goal yourself."
        }
        DelegateRole::Orchestrator => {
            "You are an orchestrator sub-agent: you MAY spawn leaf workers via delegate when independent subtasks parallelize well. Prefer leaf role for your children."
        }
    };

    let user_message = if task.context.trim().is_empty() {
        format!(
            "You are a delegated sub-agent. {role_note}\nComplete the goal and reply with a concise summary of what you did, what you found, and any issues.\n\n## Goal\n{}",
            task.goal
        )
    } else {
        format!(
            "You are a delegated sub-agent. {role_note}\nComplete the goal and reply with a concise summary of what you did, what you found, and any issues.\n\n## Goal\n{}\n\n## Context\n{}",
            task.goal, task.context
        )
    };

    let result = async {
        let turn_result = agent.run_turn(&user_message, "delegate").await?;
        let output = match turn_result {
            TurnResult::Finished(message) => message,
            TurnResult::Continue { system_prompt, .. } => {
                let (out, _) = run_provider_loop(
                    &mut agent,
                    &creds,
                    &system_prompt,
                    depth_ctx,
                    role,
                    task.toolsets.as_deref(),
                    max_rounds,
                    &creds.parent_session_id,
                )
                .await?;
                out
            }
            TurnResult::BudgetExhausted => anyhow::bail!("子 Agent 轮次预算已用尽"),
            TurnResult::MaxDepth => anyhow::bail!("子 Agent 工具轮次已达上限"),
            TurnResult::ToolCalls(_) | TurnResult::Interrupted => {
                anyhow::bail!("子 Agent 返回了不支持的轮次结果")
            }
        };

        Ok::<_, anyhow::Error>(serde_json::json!({
            "status": "ok",
            "goal": task.goal,
            "session_id": sid,
            "role": match role {
                DelegateRole::Leaf => "leaf",
                DelegateRole::Orchestrator => "orchestrator",
            },
            "project_root": project_root.as_ref().map(|p| p.display().to_string()),
            "summary": truncate_chars(&output, OUTPUT_TRUNCATE),
        }))
    }
    .await;

    if let Some(handle) = worktree {
        handle.cleanup();
    }

    result
}

fn resolve_git_repo_for_delegate(explicit: Option<&std::path::Path>) -> Option<PathBuf> {
    let candidate = resolve_project_root(explicit)?;
    find_git_root(&candidate)
}

/// 嵌套子 Agent 工具剥离：记忆/建 Agent/clarify/confirm 始终禁用；叶子再禁委派与编排。
pub fn apply_nested_agent_tool_strips(
    registry: &mut tools::ToolRegistry,
    depth_ctx: home::SpawnDepthCtx,
    role: DelegateRole,
) {
    apply_nested_agent_tool_strips_with_role(registry, depth_ctx, role);
}

/// 兼容仅依赖深度的调用方（编排步默认按叶子深度剥离委派）。
pub fn apply_nested_agent_tool_strips_depth_only(
    registry: &mut tools::ToolRegistry,
    depth_ctx: home::SpawnDepthCtx,
) {
    let role = if depth_ctx.is_leaf() {
        DelegateRole::Leaf
    } else {
        DelegateRole::Orchestrator
    };
    apply_nested_agent_tool_strips_with_role(registry, depth_ctx, role);
}

fn apply_nested_agent_tool_strips_with_role(
    registry: &mut tools::ToolRegistry,
    depth_ctx: home::SpawnDepthCtx,
    role: DelegateRole,
) {
    for name in [
        "memory",
        "session_search",
        "create_agent",
        "multi_agent",
        "ask",
        "request_user_location",
    ] {
        registry.unregister(name);
    }
    let strip_delegate = role == DelegateRole::Leaf || depth_ctx.is_leaf();
    if strip_delegate {
        for name in [
            "delegate",
            "delegate_async",
            "delegate_status",
            "delegate_collect",
            "delegate_cancel",
            "orchestration_run",
            "orchestration_status",
        ] {
            registry.unregister(name);
        }
    }
}

/// 将常用别名规范为 Astro toolset id。
pub fn normalize_toolset_name(name: &str) -> String {
    match name.trim().to_ascii_lowercase().as_str() {
        "file" | "files" => "file_ops".into(),
        "web" | "websearch" => "web_search".into(),
        "delegation" | "delegate_task" => "delegate".into(),
        "code" | "code_execution" | "code-execution" => "code_exec".into(),
        other => other.to_string(),
    }
}

/// 白名单过滤：未列出的 toolset 整表 unregister；空/缺省不改。
pub fn apply_toolsets_filter(registry: &mut tools::ToolRegistry, toolsets: Option<&[String]>) {
    let Some(list) = toolsets else {
        return;
    };
    if list.is_empty() {
        return;
    }
    let allowed: HashSet<String> = list.iter().map(|s| normalize_toolset_name(s)).collect();
    let names: Vec<String> = registry
        .all_tools()
        .iter()
        .map(|t| t.name.clone())
        .collect();
    for name in names {
        let ts = home::tool_name_to_toolset(&name);
        if !allowed.contains(ts) {
            registry.unregister(&name);
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_provider_loop(
    agent: &mut AgentLoop,
    creds: &DelegateRunRequest,
    initial_system_prompt: &str,
    depth_ctx: home::SpawnDepthCtx,
    role: DelegateRole,
    toolsets: Option<&[String]>,
    max_rounds: usize,
    parent_session_id: &str,
) -> anyhow::Result<(String, Usage)> {
    let providers = ProviderRegistry::new();
    let targets = effective_chat_targets(creds, &providers);
    let base_config = ProviderConfig::default();

    let mut system_prompt = initial_system_prompt.to_string();
    let mut last_response = String::new();
    let mut total_usage = Usage::default();

    for _round in 0..max_rounds {
        agent.reload_tools_and_mcp().await;
        apply_nested_agent_tool_strips(agent.tool_registry_mut(), depth_ctx, role);
        apply_toolsets_filter(agent.tool_registry_mut(), toolsets);

        let messages = to_provider_messages(&system_prompt, &agent.session_messages);
        let tools = agent.schemas_for_api();

        let (mut stream, _meta) = try_stream_completion_with_fallback(
            &targets,
            &providers,
            messages,
            tools,
            &base_config,
            |from, to, err| {
                tracing::warn!(
                    from_backend = %from.backend_id,
                    from_model = %from.model,
                    to_backend = %to.backend_id,
                    to_model = %to.model,
                    error = %err,
                    "delegate chat failover: switching target before first content"
                );
            },
        )
        .await?;

        let mut full_response = String::new();
        let mut round_usage: Option<Usage> = None;
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| anyhow::anyhow!("{e}"))?;
            if let Some(token) = chunk.token {
                full_response.push_str(&token);
            }
            if let Some(u) = chunk.usage {
                round_usage = Some(u);
            }
        }
        if let Some(u) = round_usage {
            total_usage.add_assign(u);
        }

        if full_response.is_empty() {
            break;
        }

        last_response = full_response.clone();
        let calls = tools::extract_tool_calls(&full_response);
        let tc = if calls.is_empty() {
            None
        } else {
            Some(
                calls
                    .iter()
                    .map(|c| common::message::ToolCall {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                        signature: None,
                    })
                    .collect(),
            )
        };
        agent.record_assistant_message_with_tools(&full_response, tc, None, None)?;

        if calls.is_empty() {
            return Ok((last_response, total_usage));
        }

        for call in calls {
            let mut result =
                tokio::task::block_in_place(|| agent.handle_tool_call(&call.name, &call.arguments))
                    .unwrap_or_else(|e| format!("工具错误: {e}"));
            if let Some(hitl) = crate::streaming::parse_astro_hitl(&result) {
                result = match crate::streaming::try_park_parent_hitl(
                    &call.id,
                    hitl,
                    Some(parent_session_id),
                )
                .await
                {
                    Some(resolved) => resolved,
                    None => {
                        "HITL unavailable in delegated sub-agent (no live parent gate); treated as cancelled."
                            .into()
                    }
                };
            }
            agent.record_tool_result_with_id(
                Some(&call.id),
                Some(&call.name),
                &format!("tool={} result={}", call.name, result),
            )?;
        }

        let turn_result = agent
            .run_turn("", "delegate-tool-followup")
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        system_prompt = match turn_result {
            TurnResult::Continue { system_prompt, .. } => system_prompt,
            TurnResult::Finished(message) => return Ok((message, total_usage)),
            TurnResult::BudgetExhausted => anyhow::bail!("子 Agent 轮次预算已用尽"),
            TurnResult::MaxDepth => anyhow::bail!("子 Agent 工具轮次已达上限"),
            TurnResult::ToolCalls(_) | TurnResult::Interrupted => {
                anyhow::bail!("子 Agent 不支持该轮次结果")
            }
        };
    }

    if last_response.is_empty() {
        anyhow::bail!("子 Agent 未返回有效回复");
    }
    Ok((last_response, total_usage))
}

/// 子 Agent 构造完、真正 `run` 前触发 `subagent_start`（每个 child 一次，观察型）。
///
/// Payload 与现有 `subagent_stop`（`runtime::fire_subagent_stop_from_delegate_result`）配对：
/// `session_id` 用父会话（`subagent_stop` 用子会话，故不可直接复用同一辨识字段），
/// `detail` 携带子角色与任务摘要，供插件/时间线区分具体 child。
fn fire_subagent_start(creds: &DelegateRunRequest, task: &DelegateTaskSpec, role: DelegateRole) {
    let Some(bus) = creds.hook_bus.as_ref() else {
        return;
    };
    let role_str = match role {
        DelegateRole::Leaf => "leaf",
        DelegateRole::Orchestrator => "orchestrator",
    };
    let _ = bus.fire(
        ::hooks::SUBAGENT_START,
        &::hooks::HookPayload {
            session_id: creds.parent_session_id.clone(),
            detail: format!("role={role_str} goal={}", truncate_chars(&task.goal, 200)),
            ..Default::default()
        },
    );
}

/// 供 `multi_agent::Orchestrator` 使用的薄封装。
pub async fn run_subtasks_parallel(
    creds: DelegateRunRequest,
    descriptions: Vec<(String, String)>,
) -> Vec<(String, String)> {
    let tasks: Vec<_> = descriptions
        .into_iter()
        .map(|(id, description)| (id, DelegateTaskSpec::new(description, String::new())))
        .collect();
    let mut out = Vec::new();
    let mut join_set = JoinSet::new();
    let max_c = creds.max_concurrent.clamp(1, 8);
    let mut iter = tasks.into_iter();
    let mut in_flight = 0usize;

    loop {
        while in_flight < max_c {
            let Some((id, task)) = iter.next() else {
                break;
            };
            let c = req_clone_creds(&creds);
            join_set.spawn(async move {
                let r = run_one_child(c, task).await;
                (id, r)
            });
            in_flight += 1;
        }
        let Some(joined) = join_set.join_next().await else {
            break;
        };
        in_flight = in_flight.saturating_sub(1);
        if let Ok((id, res)) = joined {
            let summary = match res {
                Ok(v) => v
                    .get("summary")
                    .and_then(|s| s.as_str())
                    .unwrap_or("ok")
                    .to_string(),
                Err(e) => format!("failed: {e}"),
            };
            out.push((id, summary));
        }
    }
    out
}

#[cfg(test)]
mod strip_tests {
    use super::*;
    use home::SpawnDepthCtx;
    use tools::{register_all, ToolRegistry};

    #[test]
    fn leaf_strips_delegate_clarify_and_memory() {
        let mut reg = ToolRegistry::new();
        register_all(&mut reg);
        apply_nested_agent_tool_strips(
            &mut reg,
            SpawnDepthCtx {
                depth: 1,
                max_depth: 1,
            },
            DelegateRole::Leaf,
        );
        let names: Vec<_> = reg
            .available_tools()
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert!(!names.contains(&"delegate"));
        assert!(!names.contains(&"orchestration_run"));
        assert!(!names.contains(&"ask"));
        assert!(!names.contains(&"request_user_location"));
        assert!(!names.contains(&"create_agent"));
        assert!(!names.contains(&"memory"));
    }

    #[test]
    fn mid_depth_orchestrator_keeps_delegate() {
        let mut reg = ToolRegistry::new();
        register_all(&mut reg);
        apply_nested_agent_tool_strips(
            &mut reg,
            SpawnDepthCtx {
                depth: 1,
                max_depth: 2,
            },
            DelegateRole::Orchestrator,
        );
        let names: Vec<_> = reg
            .available_tools()
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert!(names.contains(&"delegate"));
        assert!(names.contains(&"orchestration_run"));
        assert!(!names.contains(&"memory"));
        assert!(!names.contains(&"ask"));
        assert!(!names.contains(&"request_user_location"));
    }

    #[test]
    fn toolsets_whitelist_keeps_only_terminal() {
        let mut reg = ToolRegistry::new();
        register_all(&mut reg);
        apply_nested_agent_tool_strips(
            &mut reg,
            SpawnDepthCtx {
                depth: 1,
                max_depth: 1,
            },
            DelegateRole::Leaf,
        );
        apply_toolsets_filter(&mut reg, Some(&["terminal".into()]));
        let names: Vec<_> = reg
            .available_tools()
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert!(names.contains(&"terminal"));
        assert!(!names.contains(&"file_ops"));
    }

    #[test]
    fn normalize_aliases() {
        assert_eq!(normalize_toolset_name("file"), "file_ops");
        assert_eq!(normalize_toolset_name("web"), "web_search");
    }
}

#[cfg(test)]
mod subagent_lifecycle_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    /// `subagent_start` 必须在每个 child 真正 run 前触发；与 `finalize_tool_call_result`
    /// 内既有的 `subagent_stop`（Task 5 之前已落地）配对，顺序须为 start → stop。
    ///
    /// provider 故意设为未注册的 backend id，令 `run_provider_loop` 在建连前立即失败
    /// （`registry.get` 返回 `None`），从而无需真实网络请求即可覆盖“child 已构造、
    /// 即将 run”这一时机。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subagent_start_fires_before_subagent_stop_per_child() {
        let mem_dir = TempDir::new().expect("tempdir");
        let _env = home::test_env::AstroMemoryDirGuard::set(mem_dir.path());
        let project_dir = TempDir::new().expect("tempdir"); // 非 git 仓：避免触发真实 worktree

        let bus = Arc::new(::hooks::PluginHookBus::new());
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        let log_start = Arc::clone(&log);
        bus.register(::hooks::SUBAGENT_START, move |payload| {
            log_start
                .lock()
                .unwrap()
                .push(format!("start:{}", payload.session_id));
            ::hooks::HookOutcome::Continue
        });
        let log_stop = Arc::clone(&log);
        bus.register(::hooks::SUBAGENT_STOP, move |payload| {
            log_stop
                .lock()
                .unwrap()
                .push(format!("stop:{}", payload.session_id));
            ::hooks::HookOutcome::Continue
        });

        let req = DelegateRunRequest {
            parent_agent_id: "test-agent".into(),
            parent_session_id: "parent-session".into(),
            provider: "test-nonexistent-provider".into(),
            model: "dummy-model".into(),
            api_key: "dummy-key".into(),
            base_url: String::new(),
            chat_targets: vec![],
            tasks: vec![DelegateTaskSpec::new("do the thing", "")],
            max_concurrent: 1,
            caller_depth: 0,
            max_spawn_depth: 1,
            project_root: Some(project_dir.path().to_path_buf()),
            hook_bus: Some(Arc::clone(&bus)),
        };

        let raw_result = run_delegate(req)
            .await
            .expect("run_delegate should still return a summary JSON on child failure");

        let mut agent =
            AgentLoop::new(AgentConfig::with_defaults(mem_dir.path().to_path_buf())).unwrap();
        agent.set_hook_bus(Arc::clone(&bus));
        let _ = agent
            .finalize_tool_call_result("delegate", &serde_json::json!({}), raw_result)
            .await;

        let entries = log.lock().unwrap().clone();
        assert_eq!(
            entries.len(),
            2,
            "expected exactly one subagent_start and one subagent_stop, got {entries:?}"
        );
        assert!(
            entries[0].starts_with("start:"),
            "subagent_start must fire before subagent_stop, got {entries:?}"
        );
        assert!(
            entries[1].starts_with("stop:"),
            "subagent_stop must fire after subagent_start, got {entries:?}"
        );
    }

    /// 无 hook bus（`hook_bus: None`）时静默跳过，不 panic、不影响子任务执行结果。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subagent_start_skips_silently_without_hook_bus() {
        let mem_dir = TempDir::new().expect("tempdir");
        let _env = home::test_env::AstroMemoryDirGuard::set(mem_dir.path());
        let project_dir = TempDir::new().expect("tempdir");

        let req = DelegateRunRequest {
            parent_agent_id: "test-agent".into(),
            parent_session_id: "parent-session".into(),
            provider: "test-nonexistent-provider".into(),
            model: "dummy-model".into(),
            api_key: "dummy-key".into(),
            base_url: String::new(),
            chat_targets: vec![],
            tasks: vec![DelegateTaskSpec::new("do the thing", "")],
            max_concurrent: 1,
            caller_depth: 0,
            max_spawn_depth: 1,
            project_root: Some(project_dir.path().to_path_buf()),
            hook_bus: None,
        };

        let raw_result = run_delegate(req)
            .await
            .expect("run_delegate should not panic without a hook bus");
        let v: serde_json::Value = serde_json::from_str(&raw_result).unwrap();
        assert_eq!(v["tasks"].as_array().unwrap().len(), 1);
    }
}
