//! 单轮工具调用执行：串行（HITL/危险命令走 park）与并发（普通工具）两条路径。

use std::sync::Arc;

use providers::PauseControl;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;

use crate::control::hitl::HitlGate;
use crate::runtime::AgentLoop;

use super::hitl_bridge::{
    park_astro_hitl, park_confirm, parse_astro_hitl, ParentHitlCtx, PARENT_HITL_CTX,
};
use super::types::MultiTurnStreamItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalRoute {
    Deny,
    Allowlist,
    Off,
    Smart,
    Manual,
}

fn approval_route(
    command: &str,
    selection: &types::SessionPermissions,
    allowlist: &[String],
) -> ApprovalRoute {
    if tools::is_hardline_blocked(command).is_some() {
        ApprovalRoute::Deny
    } else if tools::matches_allowlist(command, allowlist) {
        ApprovalRoute::Allowlist
    } else {
        match (
            selection.approval_policy,
            selection.approvals_reviewer,
        ) {
            (types::ApprovalPolicy::Never, _) => ApprovalRoute::Off,
            (_, types::ApprovalsReviewer::AutoReview) => ApprovalRoute::Smart,
            (_, types::ApprovalsReviewer::User) => ApprovalRoute::Manual,
        }
    }
}

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

pub(crate) fn tool_may_require_permission(name: &str, args: &serde_json::Value) -> bool {
    match name {
        // 权限 profile 和命令规则都可能要求 park；统一走串行 preflight。
        "terminal" | "code_exec" => true,
        "file_ops" => matches!(
            args.get("operation").and_then(|value| value.as_str()),
            Some("write" | "append" | "delete" | "mkdir" | "patch" | "move" | "copy")
        ),
        _ => false,
    }
}

/// 串行执行；`None` 表示已处理 cancel/断开，调用方应直接 return。
pub(crate) async fn execute_tools_serial(
    session: &Arc<Mutex<AgentLoop>>,
    calls: &[types::ParsedToolCall],
    pause: &Arc<PauseControl>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<Vec<types::ToolOutput>> {
    let ctx = hitl_gate.map(|gate| ParentHitlCtx {
        gate: gate.clone(),
        tx: tx.clone(),
        run_id: run_id.to_string(),
    });
    PARENT_HITL_CTX
        .scope(
            ctx,
            execute_tools_serial_inner(session, calls, pause, tx, run_id, hitl_gate),
        )
        .await
}

async fn execute_tools_serial_inner(
    session: &Arc<Mutex<AgentLoop>>,
    calls: &[types::ParsedToolCall],
    pause: &Arc<PauseControl>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<Vec<types::ToolOutput>> {
    let mut out: Vec<types::ToolOutput> = Vec::with_capacity(calls.len());
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
                    types::ApprovalAction::Deny => {
                        out.push(format!(
                            "Command denied by policy (dangerous: {}). Do not retry without changing the command.",
                            decision.description
                        ).into());
                        continue;
                    }
                    types::ApprovalAction::Auto => {
                        // 放行，继续执行
                    }
                    types::ApprovalAction::Ask => {
                        let cmd = call
                            .arguments
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        // 读取审批模式 + 白名单，并触发 PRE_APPROVAL_REQUEST 钩子
                        let (approval_session_id, approval_turn_id, permissions, allowlist, memory_dir) = {
                            let agent = session.lock().await;
                            let approval_session_id = agent.session_id().to_string();
                            let approval_turn_id = agent.current_turn_id().map(str::to_string);
                            let base = agent.memory_dir().to_path_buf();
                            let permissions = memory::config::load_permission_settings(&base);
                            agent.fire_hook(
                                hooks::PRE_APPROVAL_REQUEST,
                                hooks::HookPayload {
                                    session_id: approval_session_id.clone(),
                                    turn_id: approval_turn_id.clone(),
                                    message: Some(cmd.clone()),
                                    detail: format!(
                                        "surface=terminal ask={}",
                                        decision.description
                                    ),
                                    ..Default::default()
                                },
                            );
                            (
                                approval_session_id,
                                approval_turn_id,
                                permissions.selection,
                                permissions.legacy_command_allowlist,
                                base,
                            )
                        };

                        let route = approval_route(&cmd, &permissions, &allowlist);
                        // 防御性兜底：即使规则分级未来发生漂移，hardline 仍不可进入 HITL 放行。
                        if route == ApprovalRoute::Deny {
                            out.push(
                                "Command denied by hardline policy. Do not retry without changing the command."
                                    .into(),
                            );
                            continue;
                        } else if route == ApprovalRoute::Allowlist {
                            fire_post_approval_response(
                                session,
                                &approval_session_id,
                                approval_turn_id.as_deref(),
                                &cmd,
                                "allowlist",
                            )
                            .await;
                        } else if route == ApprovalRoute::Off {
                            fire_post_approval_response(
                                session,
                                &approval_session_id,
                                approval_turn_id.as_deref(),
                                &cmd,
                                "auto",
                            )
                            .await;
                        } else {
                            // 仅 Smart 模式尝试辅模型降级；Manual 直接弹卡
                            let smart_action = if route == ApprovalRoute::Smart {
                                let agent = session.lock().await;
                                let targets: Vec<_> = agent
                                    .auxiliary_targets(types::AuxiliaryTask::SmartApproval)
                                    .iter()
                                    .map(crate::control::smart_approval::ApprovalTarget::from)
                                    .collect();
                                drop(agent);
                                crate::control::smart_approval::maybe_smart_downgrade_ask(
                                    &cmd,
                                    decision.description,
                                    &targets,
                                )
                                .await
                            } else {
                                types::ApprovalAction::Ask
                            };
                            if smart_action == types::ApprovalAction::Auto {
                                tracing::info!(
                                    command = %cmd,
                                    reason = decision.description,
                                    "smart approval auto-approved dangerous command"
                                );
                                fire_post_approval_response(
                                    session,
                                    &approval_session_id,
                                    approval_turn_id.as_deref(),
                                    &cmd,
                                    "auto",
                                )
                                .await;
                            } else if route == ApprovalRoute::Smart {
                                fire_post_approval_response(
                                    session,
                                    &approval_session_id,
                                    approval_turn_id.as_deref(),
                                    &cmd,
                                    "deny",
                                )
                                .await;
                                out.push(
                                    "Command denied by automatic approval review. Do not retry the same action or attempt a workaround without explicit user authorization."
                                        .into(),
                                );
                                continue;
                            } else if let Some(gate) = hitl_gate {
                                let title = "批准危险命令";
                                let body = format!(
                                    "检测到潜在危险操作（{}）：\n\n```\n{cmd}\n```",
                                    decision.description
                                );
                                let confirm =
                                    park_confirm(gate, tx, run_id, &call.id, title, &body, true)
                                        .await?;
                                let choice = match confirm.status.as_str() {
                                    "timeout" => "timeout",
                                    _ if confirm.approved => "allow",
                                    _ => "deny",
                                };
                                fire_post_approval_response(
                                    session,
                                    &approval_session_id,
                                    approval_turn_id.as_deref(),
                                    &cmd,
                                    choice,
                                )
                                .await;
                                if !confirm.approved {
                                    out.push(
                                        "Command denied by user (dangerous-command approval). Do not retry the same command without explicit user request.".into(),
                                    );
                                    continue;
                                }
                                // 「批准并永久放行」→ 写入用户白名单，后续同命令自动放行
                                if confirm.always {
                                    if let Err(e) =
                                        memory::config::add_command_to_allowlist(&memory_dir, &cmd)
                                    {
                                        tracing::warn!(error = %e, "failed to persist command allowlist");
                                    } else {
                                        tracing::info!(command = %cmd, "added command to approval allowlist");
                                    }
                                }
                            } else {
                                fire_post_approval_response(
                                    session,
                                    &approval_session_id,
                                    approval_turn_id.as_deref(),
                                    &cmd,
                                    "unavailable",
                                )
                                .await;
                                out.push(
                                    format!(
                                    "Command blocked: dangerous ({}) and no HITL gate available.",
                                    decision.description
                                )
                                    .into(),
                                );
                                continue;
                            }
                        }
                    }
                }
            }
        }

        let mut result: types::ToolOutput = if call.args_parse_error {
            format!(
                "工具参数 JSON 解析失败: {}",
                call.arguments
                    .get("_parse_error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("invalid json")
            )
            .into()
        } else {
            let mut agent = session.lock().await;
            let memory_dir = agent.memory_dir().to_path_buf();
            let session_id = agent.session_id().to_string();
            match tokio::task::block_in_place(|| {
                agent.handle_tool_call(&call.name, &call.arguments)
            }) {
                Ok(output) => output,
                Err(crate::runtime::ToolCallError::Cancelled) => return None,
                Err(e) => {
                    memory::try_append_decision(
                        &memory_dir,
                        memory::DecisionEntry::new(
                            memory::DecisionKind::ToolFailure,
                            format!("{e}"),
                        )
                        .with_tool(call.name.clone())
                        .with_session(session_id),
                    );
                    format!("工具错误: {e}").into()
                }
            }
        };

        // confirm/clarify：astro_hitl → 同回合 park
        if let Some(hitl) = parse_astro_hitl(result.text()) {
            if let Some(gate) = hitl_gate {
                result = park_astro_hitl(gate, tx, run_id, &call.id, hitl)
                    .await?
                    .into();
            } else {
                // 无 HitlGate（单测或未注入闸门）：无法 park，返回说明文案
                result = "HITL gate unavailable; confirmation/clarification could not be shown to the user.".into();
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
    calls: &[types::ParsedToolCall],
    pause: &Arc<PauseControl>,
) -> Option<Vec<types::ToolOutput>> {
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
            credentials: types::ModelCredentials {
                provider: agent.chat_provider().to_string(),
                model: agent.chat_model().to_string(),
                api_key: agent.chat_api_key().to_string(),
                base_url: agent.chat_base_url().to_string(),
            },
            chat_targets: agent.chat_targets().to_vec(),
            image_gen_targets: agent.image_gen_targets().clone(),
            execution: agent.execution(),
            hook_bus: Some(agent.hook_bus()),
        }
    };

    let mut join_set = JoinSet::new();
    for (idx, call) in calls.iter().cloned().enumerate() {
        let snap = snap.clone();
        join_set.spawn_blocking(move || {
            let result: types::ToolOutput = if call.args_parse_error {
                format!(
                    "工具参数 JSON 解析失败: {}",
                    call.arguments
                        .get("_parse_error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("invalid json")
                )
                .into()
            } else {
                run_tool_on_snapshot(&snap, &call.name, &call.arguments)
            };
            (idx, result)
        });
    }

    let mut slots: Vec<Option<types::ToolOutput>> = (0..calls.len()).map(|_| None).collect();
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((idx, result)) => {
                if let Some(slot) = slots.get_mut(idx) {
                    *slot = Some(result);
                }
            }
            Err(e) => {
                // 标记失败占位
                let msg: types::ToolOutput = format!("工具错误: join failed: {e}").into();
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
    credentials: types::ModelCredentials,
    chat_targets: Vec<types::ChatTarget>,
    image_gen_targets: types::ImageGenTargets,
    execution: Arc<dyn tools::ExecutionDispatch>,
    hook_bus: Option<Arc<hooks::PluginHookBus>>,
}

fn run_tool_on_snapshot(
    snap: &ToolExecSnapshot,
    name: &str,
    args: &serde_json::Value,
) -> types::ToolOutput {
    // 纵深防御：并发路径没有审批闸门，此处硬拦 hardline 命令，
    // 即便路由判定漏了（见 terminal_needs_approval），也不会执行不可恢复操作。
    if name == "terminal" {
        if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
            if let Some(desc) = tools::is_hardline_blocked(cmd) {
                return format!(
                    "Command denied by policy (dangerous: {desc}). Do not retry without changing the command."
                ).into();
            }
        }
    }
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return format!("工具错误: runtime: {e}").into(),
    };
    rt.block_on(async {
        let mut memory =
            match memory::MemoryManager::for_agent(snap.memory_dir.clone(), &snap.agent_id) {
                Ok(m) => m,
                Err(e) => return format!("工具错误: memory: {e}").into(),
            };
        let sessions =
            match session::SessionStore::open_sessions_dir(&snap.memory_dir.join("sessions")) {
                Ok(s) => s,
                Err(e) => return format!("工具错误: sessions: {e}").into(),
            };
        let mut ctx = tools::ToolContext {
            memory: &mut memory,
            sessions: &sessions,
            memory_dir: snap.memory_dir.clone(),
            workspace_dir: snap.workspace_dir.clone(),
            project_root: snap.project_root.clone(),
            image_gen_targets: &snap.image_gen_targets,
            session_id: snap.session_id.clone(),
            turn_id: snap.turn_id.clone(),
            credentials: &snap.credentials,
            chat_targets: &snap.chat_targets,
            execution: Some(snap.execution.clone()),
            hook_bus: snap.hook_bus.clone(),
        };
        tools::dispatch_tool(|_| true, &mut ctx, name, args, None)
            .await
            .unwrap_or_else(|e| {
                memory::try_append_decision(
                    &snap.memory_dir,
                    memory::DecisionEntry::new(memory::DecisionKind::ToolFailure, format!("{e}"))
                        .with_tool(name.to_string())
                        .with_session(snap.session_id.clone()),
                );
                format!("工具错误: {e}").into()
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn term(cmd: &str) -> serde_json::Value {
        json!({ "command": cmd })
    }

    #[test]
    fn dangerous_commands_force_serial() {
        // hardline(Deny) 必须强制串行——否则会经并发路径绕过 Deny 拦截
        assert!(terminal_needs_approval(
            "terminal",
            &term("mkfs.ext4 /dev/sdb1")
        ));
        assert!(terminal_needs_approval(
            "terminal",
            &term("dd if=/dev/zero of=/dev/sda")
        ));
        // Ask 也强制串行（需 HITL 卡）
        assert!(terminal_needs_approval(
            "terminal",
            &term("rm -rf /tmp/project")
        ));
    }

    #[test]
    fn safe_and_auto_commands_allow_concurrent() {
        // Auto 白名单与安全命令无需串行
        assert!(!terminal_needs_approval(
            "terminal",
            &term("rm -rf node_modules")
        ));
        assert!(!terminal_needs_approval("terminal", &term("ls -la")));
        assert!(!terminal_needs_approval("terminal", &term("cargo test")));
        // 非 terminal 工具永不触发
        assert!(!terminal_needs_approval("file_ops", &term("mkfs")));
    }

    #[test]
    fn approval_modes_and_allowlist_route_correctly() {
        let ask = "rm -rf /tmp/project";
        let none: Vec<String> = Vec::new();
        assert_eq!(
            approval_route(ask, types::ApprovalMode::Smart, &none),
            ApprovalRoute::Smart
        );
        assert_eq!(
            approval_route(ask, types::ApprovalMode::Manual, &none),
            ApprovalRoute::Manual
        );
        assert_eq!(
            approval_route(ask, types::ApprovalMode::Off, &none),
            ApprovalRoute::Off
        );

        let allowlist = vec![ask.to_string()];
        assert_eq!(
            approval_route(ask, types::ApprovalMode::Manual, &allowlist),
            ApprovalRoute::Allowlist
        );
    }

    #[test]
    fn hardline_wins_over_off_and_allowlist() {
        let command = "mkfs.ext4 /dev/sdb1";
        let allowlist = vec!["mkfs*".to_string()];
        assert_eq!(
            approval_route(command, types::ApprovalMode::Off, &allowlist),
            ApprovalRoute::Deny
        );
    }
}
