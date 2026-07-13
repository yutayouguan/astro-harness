//! 多 Agent 串行编排执行器。
//!
//! 从 `orchestration.db` 认领任务，按 `seq` 串行执行子步（已有 Agent 或临时角色），
//! 单步超时 120s；失败则中止后续步骤。对齐 [`crate::cron_exec`] 的 AgentLoop 用法。
//!
//! **不变量**
//! - 同一 `orchestration_id` 仅通过 `try_claim_running` 认领一次
//! - 子步工具表剔除 `orchestration_*`，避免递归编排
//! - 临时角色不新建 Agent workspace，复用 `parent_agent_id` 记忆

use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use futures::StreamExt;
use memory::{
    default_memory_dir, AgentRuntimeConfig, NewUsageEvent, OrchestrationDb, OrchestrationRow,
    OrchestrationSpawnRequest, OrchestrationStatus, StepRow, UsageDb,
};
use providers::registry::ProviderRegistry;
use providers::streaming::Usage;
use providers::trait_::ProviderConfig;
use uuid::Uuid;

use crate::loop_::{AgentConfig, AgentLoop, TurnResult};
use crate::messages::to_provider_messages;

/// 单步执行超时（秒）
const STEP_TIMEOUT_SECS: u64 = 120;
/// Provider 工具跟随最多轮次
const PROVIDER_MAX_ROUNDS: usize = 5;

/// 认领并串行执行一次编排；若已被认领则立即返回 Ok。
pub async fn run_orchestration(req: OrchestrationSpawnRequest) -> anyhow::Result<()> {
    let db = OrchestrationDb::open_default()?;
    if !db.try_claim_running(&req.orchestration_id)? {
        return Ok(());
    }
    let steps = db.list_steps(&req.orchestration_id)?;
    let orch = db
        .get(&req.orchestration_id)?
        .ok_or_else(|| anyhow::anyhow!("orchestration missing: {}", req.orchestration_id))?;

    let mut prev_output = String::new();
    let mut summaries = Vec::new();

    for step in steps {
        db.set_step_running(&step.id)?;
        record_orchestration_edge(&req, &orch, &step, "start", None);

        let prompt = if prev_output.is_empty() {
            step.prompt.clone()
        } else {
            format!(
                "{}\n\n## Previous step output (truncated)\n{}",
                step.prompt,
                truncate_chars(&prev_output, 8_000)
            )
        };

        let result = tokio::time::timeout(
            Duration::from_secs(STEP_TIMEOUT_SECS),
            run_step(&req, &orch, &step, &prompt),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                db.set_step_done(&step.id, &output)?;
                record_orchestration_edge(&req, &orch, &step, "end", Some(true));
                summaries.push(format!(
                    "### {}\n{}",
                    step.role,
                    truncate_chars(&output, 2_000)
                ));
                prev_output = output;
            }
            Ok(Err(e)) => {
                let msg = e.to_string();
                db.set_step_failed(&step.id, &msg)?;
                db.skip_pending_steps_after(&req.orchestration_id, step.seq)?;
                record_orchestration_edge(&req, &orch, &step, "end", Some(false));
                db.set_orchestration_status(
                    &req.orchestration_id,
                    OrchestrationStatus::Failed,
                    Some(&msg),
                    None,
                )?;
                return Ok(());
            }
            Err(_) => {
                let msg = format!("step timeout ({STEP_TIMEOUT_SECS}s)");
                db.set_step_failed(&step.id, &msg)?;
                db.skip_pending_steps_after(&req.orchestration_id, step.seq)?;
                record_orchestration_edge(&req, &orch, &step, "end", Some(false));
                db.set_orchestration_status(
                    &req.orchestration_id,
                    OrchestrationStatus::Failed,
                    Some("timeout"),
                    None,
                )?;
                return Ok(());
            }
        }
    }

    let summary = summaries.join("\n\n");
    db.set_orchestration_status(
        &req.orchestration_id,
        OrchestrationStatus::Done,
        None,
        Some(&summary),
    )?;
    Ok(())
}

async fn run_step(
    req: &OrchestrationSpawnRequest,
    orch: &OrchestrationRow,
    step: &StepRow,
    prompt: &str,
) -> anyhow::Result<String> {
    if req.api_key.trim().is_empty() {
        anyhow::bail!("未配置 API Key，无法执行编排步骤");
    }

    let memory_dir = default_memory_dir();
    let sid = Uuid::new_v4().to_string();

    let (target_agent_id, user_message) = match step.agent_id.as_deref().map(str::trim) {
        Some(id) if !id.is_empty() => (id.to_string(), prompt.to_string()),
        _ => {
            // 临时角色：复用父 Agent 工作区，不 create_agent
            let msg = format!(
                "Role: {}\nGoal: {}\n\n{}",
                step.role, orch.goal, prompt
            );
            (orch.parent_agent_id.clone(), msg)
        }
    };

    let (provider, model, api_key, base_url) =
        resolve_creds(req, Some(&target_agent_id), &memory_dir);

    let mut config = AgentConfig::with_defaults(memory_dir.clone());
    // 覆盖 soul 为该 agent 的 SOUL.md（with_defaults 读的是活跃 agent）
    let ws = memory::agent_workspace_dir(&memory_dir, &target_agent_id);
    if let Ok(soul) = std::fs::read_to_string(ws.join("SOUL.md")) {
        config.soul = soul;
    }
    config.multi_turn = PROVIDER_MAX_ROUNDS;

    let mut agent = AgentLoop::with_session_id_for_agent(config, sid, &target_agent_id)?;
    // 禁止子编排递归
    agent.tool_registry_mut().unregister("orchestration_run");
    agent.tool_registry_mut().unregister("orchestration_status");
    agent.set_chat_credentials(&provider, &model, &api_key, &base_url);

    let turn_result = agent.run_turn(&user_message, "orchestration").await?;
    match turn_result {
        TurnResult::Finished(message) => Ok(message),
        TurnResult::Continue { system_prompt, .. } => {
            let (output, _) =
                run_provider_loop(&mut agent, &provider, &model, &api_key, &base_url, &system_prompt)
                    .await?;
            Ok(output)
        }
        TurnResult::BudgetExhausted => anyhow::bail!("对话轮次预算已用尽"),
        TurnResult::MaxDepth => anyhow::bail!("工具调用轮次已达上限"),
        TurnResult::ToolCalls(_) | TurnResult::Interrupted => {
            anyhow::bail!("编排步骤不支持该轮次结果")
        }
    }
}

fn resolve_creds(
    req: &OrchestrationSpawnRequest,
    agent_id: Option<&str>,
    memory_dir: &Path,
) -> (String, String, String, String) {
    let mut provider = req.provider.clone();
    let mut model = req.model.clone();
    if let Some(aid) = agent_id {
        if let Ok(cfg) = AgentRuntimeConfig::load(memory_dir, aid) {
            if let Some(p) = cfg.provider_id.filter(|s| !s.trim().is_empty()) {
                provider = p;
            }
            if let Some(m) = cfg.model.filter(|s| !s.trim().is_empty()) {
                model = m;
            }
        }
    }
    (
        provider,
        model,
        req.api_key.clone(),
        req.base_url.clone(),
    )
}

async fn run_provider_loop(
    agent: &mut AgentLoop,
    provider_name: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
    initial_system_prompt: &str,
) -> anyhow::Result<(String, Usage)> {
    let providers = ProviderRegistry::new();
    let name = if provider_name.trim().is_empty() {
        "openai"
    } else {
        provider_name
    };
    let provider = providers
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("未知 Provider: {name}"))?;

    let config = ProviderConfig {
        model: if model.trim().is_empty() {
            provider.default_model().to_string()
        } else {
            model.to_string()
        },
        api_key: api_key.to_string(),
        base_url: if base_url.trim().is_empty() {
            None
        } else {
            Some(base_url.to_string())
        },
        ..ProviderConfig::default()
    };

    let mut system_prompt = initial_system_prompt.to_string();
    let mut last_response = String::new();
    let mut total_usage = Usage::default();

    for _round in 0..PROVIDER_MAX_ROUNDS {
        agent.reload_tools_and_mcp().await;
        // 重载后再次剔除编排工具
        agent.tool_registry_mut().unregister("orchestration_run");
        agent.tool_registry_mut().unregister("orchestration_status");

        let messages = to_provider_messages(&system_prompt, &agent.session_messages);
        let tools = agent.tool_registry().schemas_for_api();

        let mut stream = provider
            .chat_stream(messages, tools, &config)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

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
                    })
                    .collect(),
            )
        };
        agent.record_assistant_message_with_tools(&full_response, tc, None, None)?;

        if calls.is_empty() {
            return Ok((last_response, total_usage));
        }

        for call in calls {
            let result = tokio::task::block_in_place(|| {
                agent.handle_tool_call(&call.name, &call.arguments)
            })
            .unwrap_or_else(|e| format!("工具错误: {e}"));

            agent.record_tool_result_with_id(
                Some(&call.id),
                Some(&call.name),
                &format!(
                    "tool={} args={} result={}",
                    call.name, call.arguments, result
                ),
            )?;
        }

        let turn_result = agent
            .run_turn("", "orchestration-tool-followup")
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        system_prompt = match turn_result {
            TurnResult::Continue { system_prompt, .. } => system_prompt,
            TurnResult::Finished(message) => return Ok((message, total_usage)),
            TurnResult::BudgetExhausted => anyhow::bail!("对话轮次预算已用尽"),
            TurnResult::MaxDepth => anyhow::bail!("工具调用轮次已达上限"),
            TurnResult::ToolCalls(_) | TurnResult::Interrupted => {
                anyhow::bail!("编排步骤不支持该轮次结果")
            }
        };
    }

    if last_response.is_empty() {
        anyhow::bail!("模型未返回有效回复");
    }
    Ok((last_response, total_usage))
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out
}

/// 写入 handoff 遥测边；`kind=orchestration` 不计入 Insights calls KPI。
fn record_orchestration_edge(
    req: &OrchestrationSpawnRequest,
    orch: &OrchestrationRow,
    step: &StepRow,
    phase: &str,
    ok: Option<bool>,
) {
    let to = step
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("role:{}", step.role));

    let mut meta = serde_json::json!({
        "orchestration_id": req.orchestration_id,
        "step_id": step.id,
        "seq": step.seq,
        "from": orch.parent_agent_id,
        "to": to,
        "phase": phase,
    });
    if let Some(ok) = ok {
        meta["ok"] = serde_json::Value::Bool(ok);
    }

    UsageDb::try_record(NewUsageEvent {
        ts: Utc::now().to_rfc3339(),
        kind: "orchestration".into(),
        name: "orchestration_step".into(),
        agent_id: orch.parent_agent_id.clone(),
        session_id: orch.session_id.clone(),
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: Some(meta.to_string()),
    });
}
