//! 单轮工具调用执行：串行（HITL/危险命令走 park）与并发（普通工具）两条路径。

use std::sync::Arc;

use providers::PauseControl;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;

use crate::control::hitl::HitlGate;
use crate::runtime::AgentLoop;

use super::hitl_bridge::{park_astro_hitl, park_confirm, parse_astro_hitl};
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
        match (selection.approval_policy, selection.approvals_reviewer) {
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

async fn fire_post_permission_response(
    session: &Arc<Mutex<AgentLoop>>,
    session_id: &str,
    turn_id: Option<&str>,
    request: &types::PermissionRequest,
    choice: &str,
) {
    let agent = session.lock().await;
    agent.fire_hook(
        hooks::POST_APPROVAL_RESPONSE,
        hooks::HookPayload {
            session_id: session_id.to_string(),
            turn_id: turn_id.map(str::to_string),
            message: Some(request.summary.clone()),
            detail: format!("surface=permission choice={choice}"),
            ..Default::default()
        },
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PermissionPreflight {
    NotRequired,
    Granted(Box<PermissionAuditReceipt>),
    Denied(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PermissionAuditReceipt {
    memory_dir: std::path::PathBuf,
    profile_id: String,
    snapshot_hash: String,
    request: types::PermissionRequest,
}

impl PermissionAuditReceipt {
    fn new(
        memory_dir: std::path::PathBuf,
        settings: &memory::LoadedPermissionSettings,
        profile_id: String,
        request: types::PermissionRequest,
    ) -> Self {
        let snapshot_hash = memory::permission_snapshot_hash(settings, &profile_id);
        Self {
            memory_dir,
            profile_id,
            snapshot_hash,
            request,
        }
    }

    fn record(
        &self,
        kind: memory::PermissionAuditKind,
        reviewer: Option<types::ApprovalsReviewer>,
        result: Option<&str>,
        duration_ms: Option<u64>,
    ) {
        let mut event = memory::PermissionAuditEvent::new(
            kind,
            &self.request,
            self.profile_id.clone(),
            self.snapshot_hash.clone(),
        );
        if let Some(reviewer) = reviewer {
            event = event.with_reviewer(reviewer);
        }
        if let Some(result) = result {
            event = event.with_result(result);
        }
        if let Some(duration_ms) = duration_ms {
            event = event.with_duration_ms(duration_ms);
        }
        memory::try_append_permission_audit(&self.memory_dir, event);
    }

    fn record_review(
        &self,
        reviewer: types::ApprovalsReviewer,
        result: &str,
        granted: bool,
        duration_ms: u64,
    ) {
        self.record(
            memory::PermissionAuditKind::Reviewed,
            Some(reviewer),
            Some(result),
            Some(duration_ms),
        );
        self.record(
            if granted {
                memory::PermissionAuditKind::Granted
            } else {
                memory::PermissionAuditKind::Denied
            },
            Some(reviewer),
            Some(result),
            Some(duration_ms),
        );
    }
}

#[allow(clippy::too_many_arguments)]
async fn review_once_permission(
    session: &Arc<Mutex<AgentLoop>>,
    selection: &types::SessionPermissions,
    audit: PermissionAuditReceipt,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    hitl_gate: Option<&Arc<HitlGate>>,
    hook_detail: &str,
    title: &str,
    body: &str,
) -> Option<PermissionPreflight> {
    let request = &audit.request;
    let review_started = std::time::Instant::now();
    audit.record(
        memory::PermissionAuditKind::Evaluated,
        None,
        Some("approval_required"),
        None,
    );
    audit.record(
        memory::PermissionAuditKind::Requested,
        Some(selection.approvals_reviewer),
        None,
        None,
    );
    {
        let agent = session.lock().await;
        agent.fire_hook(
            hooks::PRE_APPROVAL_REQUEST,
            hooks::HookPayload {
                session_id: request.session_id.clone(),
                turn_id: request.turn_id.clone(),
                message: Some(request.summary.clone()),
                detail: hook_detail.to_string(),
                ..Default::default()
            },
        );
    }

    if selection.approval_policy == types::ApprovalPolicy::Never {
        fire_post_permission_response(
            session,
            &request.session_id,
            request.turn_id.as_deref(),
            request,
            "deny",
        )
        .await;
        audit.record_review(
            selection.approvals_reviewer,
            "approval_policy_never",
            false,
            review_started.elapsed().as_millis() as u64,
        );
        return Some(PermissionPreflight::Denied(
            "Permission denied: approval policy is never".to_string(),
        ));
    }

    if selection.approvals_reviewer == types::ApprovalsReviewer::AutoReview {
        let targets = {
            let agent = session.lock().await;
            agent
                .auxiliary_targets(types::AuxiliaryTask::SmartApproval)
                .iter()
                .map(crate::control::smart_approval::ApprovalTarget::from)
                .collect::<Vec<_>>()
        };
        let action =
            crate::control::smart_approval::maybe_smart_downgrade_ask(request, &targets).await;
        if action == types::ApprovalAction::Auto {
            fire_post_permission_response(
                session,
                &request.session_id,
                request.turn_id.as_deref(),
                request,
                "auto",
            )
            .await;
            audit.record_review(
                selection.approvals_reviewer,
                "auto_approved",
                true,
                review_started.elapsed().as_millis() as u64,
            );
            return Some(PermissionPreflight::Granted(Box::new(audit)));
        }
        fire_post_permission_response(
            session,
            &request.session_id,
            request.turn_id.as_deref(),
            request,
            "deny",
        )
        .await;
        audit.record_review(
            selection.approvals_reviewer,
            "auto_denied",
            false,
            review_started.elapsed().as_millis() as u64,
        );
        return Some(PermissionPreflight::Denied(
            "Permission denied by automatic approval review".to_string(),
        ));
    }

    let Some(gate) = hitl_gate else {
        fire_post_permission_response(
            session,
            &request.session_id,
            request.turn_id.as_deref(),
            request,
            "unavailable",
        )
        .await;
        audit.record_review(
            selection.approvals_reviewer,
            "reviewer_unavailable",
            false,
            review_started.elapsed().as_millis() as u64,
        );
        return Some(PermissionPreflight::Denied(
            "Permission blocked: user approval is unavailable".to_string(),
        ));
    };
    let Some(confirm) =
        park_confirm(gate, tx, run_id, &request.tool_call_id, title, body, false).await
    else {
        audit.record_review(
            selection.approvals_reviewer,
            "cancelled",
            false,
            review_started.elapsed().as_millis() as u64,
        );
        return None;
    };
    let choice = match confirm.status.as_str() {
        "timeout" => "timeout",
        _ if confirm.approved => "allow_once",
        _ => "deny",
    };
    fire_post_permission_response(
        session,
        &request.session_id,
        request.turn_id.as_deref(),
        request,
        choice,
    )
    .await;
    audit.record_review(
        selection.approvals_reviewer,
        choice,
        confirm.approved,
        review_started.elapsed().as_millis() as u64,
    );
    if confirm.approved {
        Some(PermissionPreflight::Granted(Box::new(audit)))
    } else {
        Some(PermissionPreflight::Denied(
            "Permission denied by user".to_string(),
        ))
    }
}

fn affected_write_paths(name: &str, args: &serde_json::Value) -> Vec<String> {
    if name == "file_ops" {
        return ["path", "dest"]
            .into_iter()
            .filter_map(|key| args.get(key).and_then(|value| value.as_str()))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
    }
    let logical_path = match name {
        "todo" => "workspace/plans",
        "memory" => "agent/MEMORY.md or USER.md",
        "skills" => "skills directory",
        "pin_context" => "workspace/pinned-context.json",
        "cron" => "~/.astro/cron/jobs.json",
        "persona_create" => "~/.astro/agents",
        "image_gen" => "workspace/images",
        "video_gen" => "workspace/videos",
        "speech_gen" => "workspace/audio",
        "music_gen" => "workspace/music",
        _ => "local state",
    };
    vec![logical_path.to_string()]
}

async fn audit_hardline_terminal_denial(
    session: &Arc<Mutex<AgentLoop>>,
    call: &types::ParsedToolCall,
    description: &str,
) {
    let (memory_dir, settings, profile_id, session_id, turn_id) = {
        let agent = session.lock().await;
        let settings = memory::load_permission_settings(agent.memory_dir());
        let profile_id = agent
            .permission_profile()
            .unwrap_or(&settings.selection.profile_id)
            .to_string();
        (
            agent.memory_dir().to_path_buf(),
            settings,
            profile_id,
            agent.session_id().to_string(),
            agent.current_turn_id().map(str::to_string),
        )
    };
    let request = types::PermissionRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        session_id,
        turn_id,
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        summary: format!("Command denied by hardline policy: {description}"),
        capabilities: vec![types::PermissionCapability::ProcessSpawn {
            program: "sh".to_string(),
            cwd: None,
        }],
        reason: types::PermissionReason::UntrustedCommand,
        requested_scope: types::GrantScope::Once,
        command_preview: None,
        affected_paths: Vec::new(),
        network_hosts: Vec::new(),
    };
    let audit = PermissionAuditReceipt::new(memory_dir, &settings, profile_id, request);
    audit.record(
        memory::PermissionAuditKind::Evaluated,
        None,
        Some("hardline_denied"),
        None,
    );
    audit.record(
        memory::PermissionAuditKind::Denied,
        None,
        Some("hardline_denied"),
        Some(0),
    );
}

async fn preflight_read_only_write(
    session: &Arc<Mutex<AgentLoop>>,
    call: &types::ParsedToolCall,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<PermissionPreflight> {
    if !tools::tool_requires_in_process_write(&call.name, &call.arguments) {
        return Some(PermissionPreflight::NotRequired);
    }

    let (session_id, turn_id, profile_id, memory_dir, settings) = {
        let agent = session.lock().await;
        let session_id = agent.session_id().to_string();
        let turn_id = agent.current_turn_id().map(str::to_string);
        let settings = memory::load_permission_settings(agent.memory_dir());
        let profile_id = agent
            .permission_profile()
            .unwrap_or(&settings.selection.profile_id)
            .to_string();
        if profile_id != types::READ_ONLY_PROFILE {
            return Some(PermissionPreflight::NotRequired);
        }
        (
            session_id,
            turn_id,
            profile_id,
            agent.memory_dir().to_path_buf(),
            settings,
        )
    };
    let selection = settings.selection.clone();

    let affected_paths = affected_write_paths(&call.name, &call.arguments);
    let request = types::PermissionRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.clone(),
        turn_id: turn_id.clone(),
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        summary: format!(
            "Allow {} to modify the listed local state for this call",
            call.name
        ),
        capabilities: vec![types::PermissionCapability::FileWrite {
            paths: affected_paths.clone(),
        }],
        reason: types::PermissionReason::ReadOnlyMutation,
        requested_scope: types::GrantScope::Once,
        command_preview: None,
        affected_paths,
        network_hosts: Vec::new(),
    };

    let paths = request.affected_paths.join("\n- ");
    let body = format!(
        "当前为只读模式。是否仅允许本次 `{}` 执行下列写入？\n\n影响路径：\n- {}\n\n不会修改全局权限，也不会提升为完全访问。",
        call.name, paths
    );
    let audit = PermissionAuditReceipt::new(memory_dir, &settings, profile_id, request);
    review_once_permission(
        session,
        &selection,
        audit,
        tx,
        run_id,
        hitl_gate,
        "surface=permission reason=read_only_mutation",
        "批准本次写入",
        &body,
    )
    .await
}

async fn preflight_mcp_tool_approval(
    session: &Arc<Mutex<AgentLoop>>,
    call: &types::ParsedToolCall,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<PermissionPreflight> {
    let approval = {
        let agent = session.lock().await;
        agent
            .tool_registry()
            .get(&call.name)
            .and_then(|entry| entry.mcp_approval.clone())
    };
    let Some(approval) = approval else {
        return Some(PermissionPreflight::NotRequired);
    };
    let route = approval.route();
    if route == types::McpToolApprovalRoute::Allow {
        return Some(PermissionPreflight::NotRequired);
    }

    let (session_id, turn_id, profile_id, memory_dir, settings) = {
        let agent = session.lock().await;
        let settings = memory::load_permission_settings(agent.memory_dir());
        let profile_id = agent
            .permission_profile()
            .unwrap_or(&settings.selection.profile_id)
            .to_string();
        (
            agent.session_id().to_string(),
            agent.current_turn_id().map(str::to_string),
            profile_id,
            agent.memory_dir().to_path_buf(),
            settings,
        )
    };
    let mut selection = settings.selection.clone();
    if route == types::McpToolApprovalRoute::UserReview {
        selection.approvals_reviewer = types::ApprovalsReviewer::User;
    }

    let target = format!("{}/{}", approval.server_id, approval.native_name);
    let request = types::PermissionRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        session_id,
        turn_id,
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        summary: format!("Allow MCP tool {target} for this call"),
        capabilities: vec![types::PermissionCapability::ExternalSideEffect {
            category: "mcp_tool".to_string(),
            target: target.clone(),
        }],
        reason: types::PermissionReason::RulePrompt,
        requested_scope: types::GrantScope::Once,
        command_preview: None,
        affected_paths: Vec::new(),
        network_hosts: Vec::new(),
    };
    let annotations = &approval.annotations;
    let body = format!(
        "MCP Server 请求调用 `{target}`。是否仅批准本次调用？\n\n审批模式：`{}`\n风险提示：只读={}，破坏性={}，开放世界={}\n\nServer 提供的 annotations 仅作提示；批准不会绕过沙箱或其他权限检查。",
        approval.mode.as_str(),
        annotations.read_only_hint.unwrap_or(false),
        annotations.destructive_hint.unwrap_or(true),
        annotations.open_world_hint.unwrap_or(true),
    );
    let audit = PermissionAuditReceipt::new(memory_dir, &settings, profile_id, request);
    review_once_permission(
        session,
        &selection,
        audit,
        tx,
        run_id,
        hitl_gate,
        &format!(
            "surface=mcp reason=tool_policy mode={}",
            approval.mode.as_str()
        ),
        "批准 MCP 工具调用",
        &body,
    )
    .await
}

async fn preflight_in_process_network(
    session: &Arc<Mutex<AgentLoop>>,
    call: &types::ParsedToolCall,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<PermissionPreflight> {
    if !tools::tool_requires_in_process_network(&call.name) {
        return Some(PermissionPreflight::NotRequired);
    }

    let (session_id, turn_id, profile_id, memory_dir, settings) = {
        let agent = session.lock().await;
        let settings = memory::load_permission_settings(agent.memory_dir());
        let profile_id = agent
            .permission_profile()
            .unwrap_or(&settings.selection.profile_id)
            .to_string();
        (
            agent.session_id().to_string(),
            agent.current_turn_id().map(str::to_string),
            profile_id,
            agent.memory_dir().to_path_buf(),
            settings,
        )
    };
    let selection = settings.selection.clone();

    match profile_id.as_str() {
        types::DANGER_FULL_ACCESS_PROFILE => {
            return Some(PermissionPreflight::NotRequired);
        }
        types::READ_ONLY_PROFILE | types::WORKSPACE_PROFILE => {}
        custom => {
            return Some(PermissionPreflight::Denied(format!(
                "Permission denied: custom profile {custom:?} has no resolved tool-network policy"
            )));
        }
    }

    let hosts = tools::in_process_network_hosts(&call.name, &call.arguments);
    let request = types::PermissionRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        session_id,
        turn_id,
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        summary: format!("Allow {} to access the network for this call", call.name),
        capabilities: vec![types::PermissionCapability::Network {
            hosts: hosts.clone(),
        }],
        reason: types::PermissionReason::NetworkDisabled,
        requested_scope: types::GrantScope::Once,
        command_preview: None,
        affected_paths: Vec::new(),
        network_hosts: hosts.clone(),
    };
    let host_list = if hosts.is_empty() {
        "- 请求参数中的远程主机".to_string()
    } else {
        format!("- {}", hosts.join("\n- "))
    };
    let body = format!(
        "当前模式未直接授予进程内网络访问。是否仅允许本次 `{}` 访问以下主机？\n\n{}\n\n该授权不会开放 terminal/code_exec 网络，也不会持久化。",
        call.name, host_list
    );
    let audit = PermissionAuditReceipt::new(memory_dir, &settings, profile_id, request);
    review_once_permission(
        session,
        &selection,
        audit,
        tx,
        run_id,
        hitl_gate,
        "surface=permission reason=network_disabled",
        "批准本次网络访问",
        &body,
    )
    .await
}

pub(crate) fn tool_may_require_permission(name: &str, args: &serde_json::Value) -> bool {
    if tools::tool_requires_in_process_network(name) {
        return true;
    }
    match name {
        // 权限 profile 和命令规则都可能要求 park；统一走串行 preflight。
        "terminal" | "code_exec" => true,
        _ => tools::tool_requires_in_process_write(name, args),
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
    execute_tools_serial_inner(session, calls, pause, tx, run_id, hitl_gate).await
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

        let mut workspace_write_grant = false;
        let mut network_grant = tools::InProcessNetworkGrant::default();
        let mut permission_audits = Vec::new();
        if !call.args_parse_error {
            match preflight_mcp_tool_approval(session, call, tx, run_id, hitl_gate).await? {
                PermissionPreflight::NotRequired => {}
                PermissionPreflight::Granted(audit) => permission_audits.push(*audit),
                PermissionPreflight::Denied(message) => {
                    out.push(format!(
                        "{message}. Do not retry the same action or attempt a workaround without explicit authorization."
                    ).into());
                    continue;
                }
            }
            match preflight_read_only_write(session, call, tx, run_id, hitl_gate).await? {
                PermissionPreflight::NotRequired => {}
                PermissionPreflight::Granted(audit) => {
                    workspace_write_grant = true;
                    permission_audits.push(*audit);
                }
                PermissionPreflight::Denied(message) => {
                    out.push(format!(
                        "{message}. Do not retry the same action or attempt a workaround without explicit authorization."
                    ).into());
                    continue;
                }
            }
            match preflight_in_process_network(session, call, tx, run_id, hitl_gate).await? {
                PermissionPreflight::NotRequired => {}
                PermissionPreflight::Granted(audit) => {
                    network_grant = tools::InProcessNetworkGrant::for_hosts(
                        tools::in_process_network_hosts(&call.name, &call.arguments),
                    );
                    permission_audits.push(*audit);
                }
                PermissionPreflight::Denied(message) => {
                    out.push(format!(
                        "{message}. Do not retry the same action or attempt a workaround without explicit authorization."
                    ).into());
                    continue;
                }
            }
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
                        audit_hardline_terminal_denial(session, call, decision.description).await;
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
                        let (
                            approval_session_id,
                            approval_turn_id,
                            permissions,
                            allowlist,
                            memory_dir,
                            active_profile_id,
                            permission_settings,
                        ) = {
                            let agent = session.lock().await;
                            let approval_session_id = agent.session_id().to_string();
                            let approval_turn_id = agent.current_turn_id().map(str::to_string);
                            let base = agent.memory_dir().to_path_buf();
                            let permissions = memory::config::load_permission_settings(&base);
                            let active_profile_id = agent
                                .permission_profile()
                                .unwrap_or(&permissions.selection.profile_id)
                                .to_string();
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
                                permissions.selection.clone(),
                                permissions.legacy_command_allowlist.clone(),
                                base,
                                active_profile_id,
                                permissions,
                            )
                        };

                        let route = approval_route(&cmd, &permissions, &allowlist);
                        let permission_request = types::PermissionRequest {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            session_id: approval_session_id.clone(),
                            turn_id: approval_turn_id.clone(),
                            tool_call_id: call.id.clone(),
                            tool_name: call.name.clone(),
                            summary: format!(
                                "Run a command requiring approval: {}",
                                decision.description
                            ),
                            capabilities: vec![types::PermissionCapability::ProcessSpawn {
                                program: "sh".to_string(),
                                cwd: None,
                            }],
                            reason: types::PermissionReason::UntrustedCommand,
                            requested_scope: types::GrantScope::Once,
                            command_preview: Some(cmd.clone()),
                            affected_paths: Vec::new(),
                            network_hosts: Vec::new(),
                        };
                        let approval_audit = PermissionAuditReceipt::new(
                            memory_dir.clone(),
                            &permission_settings,
                            active_profile_id,
                            permission_request.clone(),
                        );
                        let approval_started = std::time::Instant::now();
                        approval_audit.record(
                            memory::PermissionAuditKind::Evaluated,
                            None,
                            Some(match route {
                                ApprovalRoute::Deny => "hardline_denied",
                                ApprovalRoute::Allowlist => "allowlist",
                                ApprovalRoute::Off => "approval_disabled",
                                ApprovalRoute::Smart => "auto_review_required",
                                ApprovalRoute::Manual => "user_review_required",
                            }),
                            None,
                        );
                        if matches!(route, ApprovalRoute::Smart | ApprovalRoute::Manual) {
                            approval_audit.record(
                                memory::PermissionAuditKind::Requested,
                                Some(permissions.approvals_reviewer),
                                None,
                                None,
                            );
                        }
                        // 防御性兜底：即使规则分级未来发生漂移，hardline 仍不可进入 HITL 放行。
                        if route == ApprovalRoute::Deny {
                            approval_audit.record(
                                memory::PermissionAuditKind::Denied,
                                None,
                                Some("hardline_denied"),
                                Some(approval_started.elapsed().as_millis() as u64),
                            );
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
                            approval_audit.record(
                                memory::PermissionAuditKind::Granted,
                                None,
                                Some("allowlist"),
                                Some(approval_started.elapsed().as_millis() as u64),
                            );
                            permission_audits.push(approval_audit);
                        } else if route == ApprovalRoute::Off {
                            fire_post_approval_response(
                                session,
                                &approval_session_id,
                                approval_turn_id.as_deref(),
                                &cmd,
                                "auto",
                            )
                            .await;
                            approval_audit.record(
                                memory::PermissionAuditKind::Granted,
                                None,
                                Some("approval_disabled"),
                                Some(approval_started.elapsed().as_millis() as u64),
                            );
                            permission_audits.push(approval_audit);
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
                                    &permission_request,
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
                                approval_audit.record_review(
                                    permissions.approvals_reviewer,
                                    "auto_approved",
                                    true,
                                    approval_started.elapsed().as_millis() as u64,
                                );
                                permission_audits.push(approval_audit);
                            } else if route == ApprovalRoute::Smart {
                                fire_post_approval_response(
                                    session,
                                    &approval_session_id,
                                    approval_turn_id.as_deref(),
                                    &cmd,
                                    "deny",
                                )
                                .await;
                                approval_audit.record_review(
                                    permissions.approvals_reviewer,
                                    "auto_denied",
                                    false,
                                    approval_started.elapsed().as_millis() as u64,
                                );
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
                                approval_audit.record_review(
                                    permissions.approvals_reviewer,
                                    choice,
                                    confirm.approved,
                                    approval_started.elapsed().as_millis() as u64,
                                );
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
                                permission_audits.push(approval_audit);
                            } else {
                                fire_post_approval_response(
                                    session,
                                    &approval_session_id,
                                    approval_turn_id.as_deref(),
                                    &cmd,
                                    "unavailable",
                                )
                                .await;
                                approval_audit.record_review(
                                    permissions.approvals_reviewer,
                                    "reviewer_unavailable",
                                    false,
                                    approval_started.elapsed().as_millis() as u64,
                                );
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
            let execution_started = std::time::Instant::now();
            let executed = tokio::task::block_in_place(|| {
                if workspace_write_grant || !network_grant.is_empty() {
                    agent.handle_tool_call_with_once_grants(
                        &call.name,
                        &call.arguments,
                        workspace_write_grant,
                        network_grant,
                    )
                } else {
                    agent.handle_tool_call(&call.name, &call.arguments)
                }
            });
            let execution_result = match &executed {
                Ok(_) => "success",
                Err(crate::runtime::ToolCallError::Cancelled) => "cancelled",
                Err(_) => "error",
            };
            for audit in &permission_audits {
                audit.record(
                    memory::PermissionAuditKind::Applied,
                    None,
                    Some(execution_result),
                    Some(execution_started.elapsed().as_millis() as u64),
                );
            }
            match executed {
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
            permission_profile: agent.permission_profile().map(str::to_string),
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
    execution: Arc<dyn tools::AgentThreadDispatch>,
    permission_profile: Option<String>,
    hook_bus: Option<Arc<hooks::PluginHookBus>>,
}

fn run_tool_on_snapshot(
    snap: &ToolExecSnapshot,
    name: &str,
    args: &serde_json::Value,
) -> types::ToolOutput {
    // 纵深防御：并发路径没有审批闸门，此处硬拦 hardline 命令，
    // 即便路由判定漏了（见 tool_may_require_permission），也不会执行不可恢复操作。
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
            permission_profile: snap.permission_profile.clone(),
            hook_bus: snap.hook_bus.clone(),
            workspace_write_grant: false,
            network_grant: tools::InProcessNetworkGrant::default(),
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
        assert!(tool_may_require_permission(
            "terminal",
            &term("mkfs.ext4 /dev/sdb1")
        ));
        assert!(tool_may_require_permission(
            "terminal",
            &term("dd if=/dev/zero of=/dev/sda")
        ));
        // Ask 也强制串行（需 HITL 卡）
        assert!(tool_may_require_permission(
            "terminal",
            &term("rm -rf /tmp/project")
        ));
    }

    #[test]
    fn process_and_mutating_file_tools_force_serial_preflight() {
        assert!(tool_may_require_permission(
            "terminal",
            &term("rm -rf node_modules")
        ));
        assert!(tool_may_require_permission("terminal", &term("ls -la")));
        assert!(tool_may_require_permission(
            "code_exec",
            &serde_json::json!({})
        ));
        assert!(tool_may_require_permission(
            "file_ops",
            &serde_json::json!({"operation": "mv"})
        ));
        assert!(!tool_may_require_permission(
            "file_ops",
            &serde_json::json!({"operation": "read"})
        ));
        assert!(tool_may_require_permission(
            "todo",
            &serde_json::json!({"action": "create"})
        ));
        assert!(tool_may_require_permission(
            "skills",
            &serde_json::json!({"action": "manage"})
        ));
        assert!(!tool_may_require_permission(
            "skills",
            &serde_json::json!({"action": "load"})
        ));
        assert!(tool_may_require_permission(
            "web_search",
            &serde_json::json!({"query": "rust"})
        ));
        assert!(tool_may_require_permission(
            "web_fetch",
            &serde_json::json!({"url": "https://example.com"})
        ));
    }

    #[test]
    fn permission_request_paths_include_both_move_endpoints() {
        assert_eq!(
            affected_write_paths(
                "file_ops",
                &serde_json::json!({"path": "src/a.rs", "dest": "src/b.rs"})
            ),
            vec!["src/a.rs", "src/b.rs"]
        );
        assert_eq!(
            affected_write_paths("todo", &serde_json::json!({})),
            vec!["workspace/plans"]
        );
    }

    #[test]
    fn approval_modes_and_allowlist_route_correctly() {
        let ask = "rm -rf /tmp/project";
        let none: Vec<String> = Vec::new();
        assert_eq!(
            approval_route(ask, &types::SessionPermissions::approve_for_me(), &none),
            ApprovalRoute::Smart
        );
        assert_eq!(
            approval_route(ask, &types::SessionPermissions::ask_for_approval(), &none),
            ApprovalRoute::Manual
        );
        assert_eq!(
            approval_route(ask, &types::SessionPermissions::full_access(), &none),
            ApprovalRoute::Off
        );

        let allowlist = vec![ask.to_string()];
        assert_eq!(
            approval_route(
                ask,
                &types::SessionPermissions::ask_for_approval(),
                &allowlist
            ),
            ApprovalRoute::Allowlist
        );
    }

    #[test]
    fn hardline_wins_over_off_and_allowlist() {
        let command = "mkfs.ext4 /dev/sdb1";
        let allowlist = vec!["mkfs*".to_string()];
        assert_eq!(
            approval_route(
                command,
                &types::SessionPermissions::full_access(),
                &allowlist
            ),
            ApprovalRoute::Deny
        );
    }

    #[test]
    fn permission_audit_receipt_records_applied_result() {
        let dir = tempfile::tempdir().unwrap();
        let settings = memory::LoadedPermissionSettings::default();
        let request = types::PermissionRequest {
            request_id: "request-1".into(),
            session_id: "session-1".into(),
            turn_id: Some("turn-1".into()),
            tool_call_id: "call-1".into(),
            tool_name: "web_fetch".into(),
            summary: "fetch approved host".into(),
            capabilities: vec![types::PermissionCapability::Network {
                hosts: vec!["example.com".into()],
            }],
            reason: types::PermissionReason::NetworkDisabled,
            requested_scope: types::GrantScope::Once,
            command_preview: None,
            affected_paths: Vec::new(),
            network_hosts: vec!["example.com".into()],
        };
        let receipt = PermissionAuditReceipt::new(
            dir.path().to_path_buf(),
            &settings,
            types::WORKSPACE_PROFILE.to_string(),
            request,
        );
        receipt.record(
            memory::PermissionAuditKind::Applied,
            None,
            Some("success"),
            Some(12),
        );

        let events = memory::list_recent_permission_audits(dir.path(), 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, memory::PermissionAuditKind::Applied);
        assert_eq!(events[0].result.as_deref(), Some("success"));
        assert_eq!(events[0].duration_ms, Some(12));
    }
}
