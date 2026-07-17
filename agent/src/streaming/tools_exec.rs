//! 单轮工具调用执行：串行（HITL/危险命令走 park）与并发（普通工具）两条路径。

use std::sync::Arc;

use providers::streaming::PauseControl;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;

use crate::control::hitl::HitlGate;
use crate::runtime::AgentLoop;

use super::hitl_bridge::{
    park_astro_hitl, park_confirm, parse_astro_hitl, ParentHitlCtx, PARENT_HITL_CTX,
};
use super::types::MultiTurnStreamItem;

/// 触发 `post_approval_response`（观察型，忽略返回值）：`choice` 为
/// `auto`（辅模型降级）/ `allow`（用户批准）/ `deny`（用户拒绝或 cancelled）/
/// `timeout`（park 超时）/ `unavailable`（无 HITL gate）。
async fn fire_post_approval_response(
    session: &Arc<Mutex<AgentLoop>>,
    session_id: &str,
    turn_id: Option<&str>,
    command: &str,
    choice: &str,
) {
    let agent = session.lock().await;
    agent.fire_hook(
        hooks::POST_APPROVAL_RESPONSE,
        hooks::HookPayload {
            session_id: session_id.to_string(),
            turn_id: turn_id.map(str::to_string),
            message: Some(command.to_string()),
            detail: format!("surface=terminal choice={choice}"),
            ..Default::default()
        },
    );
}

pub(crate) fn terminal_needs_approval(name: &str, args: &serde_json::Value) -> bool {
    if name != "terminal" {
        return false;
    }
    let cmd = args
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    matches!(
        tools::classify_dangerous_command(cmd).map(|d| d.action),
        Some(tools::ApprovalAction::Ask)
    )
}

/// 串行执行；`None` 表示已处理 cancel/断开，调用方应直接 return。
pub(crate) async fn execute_tools_serial(
    session: &Arc<Mutex<AgentLoop>>,
    calls: &[tools::ParsedToolCall],
    pause: &Arc<PauseControl>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<Vec<String>> {
    let ctx = hitl_gate.map(|gate| ParentHitlCtx {
        gate: gate.clone(),
        tx: tx.clone(),
        run_id: run_id.to_string(),
    });
    PARENT_HITL_CTX
        .scope(ctx, execute_tools_serial_inner(session, calls, pause, tx, run_id, hitl_gate))
        .await
}

async fn execute_tools_serial_inner(
    session: &Arc<Mutex<AgentLoop>>,
    calls: &[tools::ParsedToolCall],
    pause: &Arc<PauseControl>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(calls.len());
    for call in calls {
        if pause.is_cancelled() {
            return None;
        }
        if !pause.wait_if_paused().await {
            return None;
        }

        // 危险 terminal：deny / auto / ask
        if call.name == "terminal" && !call.args_parse_error {
            if let Some(decision) = call
                .arguments
                .get("command")
                .and_then(|v| v.as_str())
                .and_then(tools::classify_dangerous_command)
            {
                match decision.action {
                    tools::ApprovalAction::Deny => {
                        out.push(format!(
                            "Command denied by policy (dangerous: {}). Do not retry without changing the command.",
                            decision.description
                        ));
                        continue;
                    }
                    tools::ApprovalAction::Auto => {
                        // 放行，继续执行
                    }
                    tools::ApprovalAction::Ask => {
                        let cmd = call
                            .arguments
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let (approval_session_id, approval_turn_id) = {
                            let agent = session.lock().await;
                            let approval_session_id = agent.session_id().to_string();
                            let approval_turn_id =
                                agent.current_turn_id().map(str::to_string);
                            agent.fire_hook(
                                hooks::PRE_APPROVAL_REQUEST,
                                hooks::HookPayload {
                                    session_id: approval_session_id.clone(),
                                    turn_id: approval_turn_id.clone(),
                                    message: Some(cmd.to_string()),
                                    detail: format!(
                                        "surface=terminal ask={}",
                                        decision.description
                                    ),
                                    ..Default::default()
                                },
                            );
                            (approval_session_id, approval_turn_id)
                        };
                        // 可选辅模型降级 Ask → Auto（读取 ChatRequest 注入的 SmartApproval 目标链）
                        let smart_action = {
                            let agent = session.lock().await;
                            let targets: Vec<_> = agent
                                .auxiliary_targets(common::AuxiliaryTask::SmartApproval)
                                .iter()
                                .map(crate::control::smart_approval::ApprovalTarget::from)
                                .collect();
                            let providers = agent.providers_arc();
                            drop(agent);
                            crate::control::smart_approval::maybe_smart_downgrade_ask(
                                cmd,
                                decision.description,
                                providers.as_ref(),
                                &targets,
                            )
                            .await
                        };
                        if smart_action == tools::ApprovalAction::Auto {
                            tracing::info!(
                                command = %cmd,
                                reason = decision.description,
                                "smart approval auto-approved dangerous command"
                            );
                            fire_post_approval_response(
                                session,
                                &approval_session_id,
                                approval_turn_id.as_deref(),
                                cmd,
                                "auto",
                            )
                            .await;
                            // 放行，继续执行
                        } else if let Some(gate) = hitl_gate {
                            let title = "批准危险命令";
                            let body = format!(
                                "检测到潜在危险操作（{}）：\n\n```\n{cmd}\n```",
                                decision.description
                            );
                            let confirm =
                                park_confirm(gate, tx, run_id, &call.id, title, &body).await?;
                            let choice = match confirm.status.as_str() {
                                "timeout" => "timeout",
                                _ if confirm.approved => "allow",
                                _ => "deny",
                            };
                            fire_post_approval_response(
                                session,
                                &approval_session_id,
                                approval_turn_id.as_deref(),
                                cmd,
                                choice,
                            )
                            .await;
                            if !confirm.approved {
                                out.push(
                                    "Command denied by user (dangerous-command approval). Do not retry the same command without explicit user request.".to_string(),
                                );
                                continue;
                            }
                        } else {
                            fire_post_approval_response(
                                session,
                                &approval_session_id,
                                approval_turn_id.as_deref(),
                                cmd,
                                "unavailable",
                            )
                            .await;
                            out.push(format!(
                                "Command blocked: dangerous ({}) and no HITL gate available.",
                                decision.description
                            ));
                            continue;
                        }
                    }
                }
            }
        }

        let mut result = if call.args_parse_error {
            format!(
                "工具参数 JSON 解析失败: {}",
                call.arguments
                    .get("_parse_error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("invalid json")
            )
        } else {
            let mut agent = session.lock().await;
            let memory_dir = agent.memory_dir().to_path_buf();
            let session_id = agent.session_id().to_string();
            tokio::task::block_in_place(|| {
                agent.handle_tool_call(&call.name, &call.arguments)
            })
            .unwrap_or_else(|e| {
                memory::try_append_decision(
                    &memory_dir,
                    memory::DecisionEntry::new(
                        memory::DecisionKind::ToolFailure,
                        format!("{e}"),
                    )
                    .with_tool(call.name.clone())
                    .with_session(session_id),
                );
                format!("工具错误: {e}")
            })
        };

        // confirm/clarify：astro_hitl → 同回合 park
        if let Some(hitl) = parse_astro_hitl(&result) {
            if let Some(gate) = hitl_gate {
                result = park_astro_hitl(gate, tx, run_id, &call.id, hitl).await?;
            } else {
                // 无 HitlGate（单测或未注入闸门）：无法 park，返回说明文案
                result = "HITL gate unavailable; confirmation/clarification could not be shown to the user.".to_string();
            }
        }

        if pause.is_cancelled() {
            return None;
        }
        out.push(result);
    }
    Some(out)
}

/// 并发执行非 interactive/exclusive 工具；按调用顺序返回结果。
pub(crate) async fn execute_tools_concurrent(
    session: &Arc<Mutex<AgentLoop>>,
    calls: &[tools::ParsedToolCall],
    pause: &Arc<PauseControl>,
) -> Option<Vec<String>> {
    if pause.is_cancelled() || !pause.wait_if_paused().await {
        return None;
    }

    let snap = {
        let agent = session.lock().await;
        ToolExecSnapshot {
            memory_dir: agent.memory_dir().to_path_buf(),
            agent_id: agent.agent_id().to_string(),
            workspace_dir: agent.workspace_dir(),
            project_root: agent.project_root().cloned(),
            session_id: agent.session_id().to_string(),
            turn_id: agent.current_turn_id().map(str::to_string),
            chat_api_key: agent.chat_api_key().to_string(),
            chat_base_url: agent.chat_base_url().to_string(),
            chat_provider: agent.chat_provider().to_string(),
            chat_model: agent.chat_model().to_string(),
            chat_targets: agent.chat_targets().to_vec(),
            image_gen_targets: agent.image_gen_targets().clone(),
            providers: agent.providers_arc(),
            delegate_runner: agent.delegate_runner(),
            async_spawner: agent.async_spawner(),
            orchestration_spawner: agent.orchestration_spawner(),
            hook_bus: Some(agent.hook_bus()),
        }
    };

    let mut join_set = JoinSet::new();
    for (idx, call) in calls.iter().cloned().enumerate() {
        let snap = snap.clone();
        join_set.spawn_blocking(move || {
            let result = if call.args_parse_error {
                format!(
                    "工具参数 JSON 解析失败: {}",
                    call.arguments
                        .get("_parse_error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("invalid json")
                )
            } else {
                run_tool_on_snapshot(&snap, &call.name, &call.arguments)
            };
            (idx, result)
        });
    }

    let mut slots: Vec<Option<String>> = (0..calls.len()).map(|_| None).collect();
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((idx, result)) => {
                if let Some(slot) = slots.get_mut(idx) {
                    *slot = Some(result);
                }
            }
            Err(e) => {
                // 标记失败占位
                let msg = format!("工具错误: join failed: {e}");
                if let Some(empty_idx) = slots.iter().position(|s| s.is_none()) {
                    slots[empty_idx] = Some(msg);
                }
            }
        }
    }
    Some(
        slots
            .into_iter()
            .map(|s| s.unwrap_or_else(|| "工具错误: missing result".into()))
            .collect(),
    )
}

#[derive(Clone)]
struct ToolExecSnapshot {
    memory_dir: std::path::PathBuf,
    agent_id: String,
    workspace_dir: std::path::PathBuf,
    project_root: Option<std::path::PathBuf>,
    session_id: String,
    turn_id: Option<String>,
    chat_api_key: String,
    chat_base_url: String,
    chat_provider: String,
    chat_model: String,
    chat_targets: Vec<common::ChatTarget>,
    image_gen_targets: tools::ImageGenTargets,
    providers: Arc<providers::registry::ProviderRegistry>,
    delegate_runner: delegate::DelegateRunner,
    async_spawner: delegate::DelegateAsyncSpawner,
    orchestration_spawner: orchestration::OrchestrationSpawner,
    hook_bus: Option<Arc<hooks::PluginHookBus>>,
}

fn run_tool_on_snapshot(
    snap: &ToolExecSnapshot,
    name: &str,
    args: &serde_json::Value,
) -> String {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return format!("工具错误: runtime: {e}"),
    };
    rt.block_on(async {
        let mut memory = match memory::MemoryManager::for_agent(
            snap.memory_dir.clone(),
            &snap.agent_id,
        ) {
            Ok(m) => m,
            Err(e) => return format!("工具错误: memory: {e}"),
        };
        let sessions = match session::SessionStore::open_sessions_dir(
            &snap.memory_dir.join("sessions"),
        ) {
            Ok(s) => s,
            Err(e) => return format!("工具错误: sessions: {e}"),
        };
        let mut ctx = tools::ToolContext {
            memory: &mut memory,
            sessions: &sessions,
            memory_dir: snap.memory_dir.clone(),
            workspace_dir: snap.workspace_dir.clone(),
            project_root: snap.project_root.clone(),
            image_gen_targets: &snap.image_gen_targets,
            providers: snap.providers.as_ref(),
            session_id: snap.session_id.clone(),
            turn_id: snap.turn_id.clone(),
            chat_api_key: snap.chat_api_key.clone(),
            chat_base_url: snap.chat_base_url.clone(),
            chat_provider: snap.chat_provider.clone(),
            chat_model: snap.chat_model.clone(),
            chat_targets: snap.chat_targets.clone(),
            delegate_runner: Some(snap.delegate_runner.clone()),
            async_spawner: Some(snap.async_spawner.clone()),
            orchestration_spawner: Some(snap.orchestration_spawner.clone()),
            hook_bus: snap.hook_bus.clone(),
        };
        tools::dispatch_tool(|_| true, &mut ctx, name, args)
            .await
            .unwrap_or_else(|e| {
                memory::try_append_decision(
                    &snap.memory_dir,
                    memory::DecisionEntry::new(
                        memory::DecisionKind::ToolFailure,
                        format!("{e}"),
                    )
                    .with_tool(name.to_string())
                    .with_session(snap.session_id.clone()),
                );
                format!("工具错误: {e}")
            })
    })
}
