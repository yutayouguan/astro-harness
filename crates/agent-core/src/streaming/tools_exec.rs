//! 单轮工具调用执行：串行（HITL/危险命令走 park）与并发（普通工具）两条路径。

use std::sync::Arc;

use providers::PauseControl;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::control::hitl::HitlGate;
use crate::runtime::{
    AgentLoop, StepContext, ToolCallRuntime, ToolExecutionGrants, ToolInvocation, TurnContext,
};

use super::hitl_bridge::{park_astro_hitl, park_confirm, parse_astro_hitl};
use super::lifecycle::emit_async_agent_message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalRoute {
    Deny,
    Allowlist,
    Off,
    Smart,
    Manual,
}

fn call_uses_managed_network(call: &types::ParsedToolCall) -> bool {
    match call.name.as_str() {
        "code_exec" => true,
        "terminal" => match call.arguments.get("action") {
            None => true,
            Some(serde_json::Value::String(action)) => {
                let action = action.trim();
                action.is_empty() || action.eq_ignore_ascii_case("run")
            }
            Some(_) => false,
        },
        _ => false,
    }
}

fn managed_network_policy_for_call(
    call: &types::ParsedToolCall,
    settings: &memory::LoadedPermissionSettings,
    active_profile_id: &str,
) -> Option<types::NetworkPolicy> {
    if !settings.network_proxy_enabled
        || active_profile_id == types::DANGER_FULL_ACCESS_PROFILE
        || !call_uses_managed_network(call)
    {
        return None;
    }

    settings
        .permissions
        .profiles
        .get(active_profile_id)
        .map(|profile| profile.network.clone())
        .filter(|policy| policy.enabled)
}

async fn start_managed_network(
    session: &Arc<AgentLoop>,
    step_context: &StepContext,
    call: &types::ParsedToolCall,
    turn_context: Option<Arc<TurnContext>>,
) -> anyhow::Result<Option<Arc<network_proxy::StartedNetworkProxy>>> {
    let settings = memory::load_permission_settings(session.memory_dir());
    let active_profile_id = step_context
        .turn
        .permission_profile()
        .unwrap_or(settings.selection.profile_id.as_str());
    let Some(policy) = managed_network_policy_for_call(call, &settings, active_profile_id) else {
        return Ok(None);
    };
    let state = Arc::new(network_proxy::NetworkProxyState::new(policy)?);

    let Some(tc) = turn_context else {
        let started = network_proxy::StartedNetworkProxy::start(state).await?;
        return Ok(Some(Arc::new(started)));
    };

    let profile_id = active_profile_id.to_string();
    let command_preview = call
        .arguments
        .get("command")
        .and_then(|v| v.as_str())
        .map(|s| s.chars().take(80).collect::<String>());
    let tool_call_id = call.id.clone();

    let decider = build_network_approval_decider(
        Arc::clone(session),
        tc,
        profile_id,
        tool_call_id,
        command_preview,
    );

    let proxy = network_proxy::NetworkProxy::builder()
        .state(state)
        .policy_decider_arc(decider)
        .build()
        .await?;
    let handle = proxy.run().await?;
    Ok(Some(Arc::new(
        network_proxy::StartedNetworkProxy::from_parts(proxy, handle),
    )))
}

fn build_network_approval_decider(
    session: Arc<AgentLoop>,
    turn_context: Arc<TurnContext>,
    profile_id: String,
    tool_call_id: String,
    command_preview: Option<String>,
) -> Arc<dyn network_proxy::NetworkPolicyDecider> {
    Arc::new(move |request: network_proxy::NetworkPolicyRequest| {
        let session = Arc::clone(&session);
        let turn_context = Arc::clone(&turn_context);
        let profile_id = profile_id.clone();
        let tool_call_id = tool_call_id.clone();
        let command_preview = command_preview.clone();
        Box::pin(async move {
            let approval_service = &session.services.network_approval;
            let host_key = crate::control::network_approval::HostApprovalKey {
                profile_id: profile_id.clone(),
                host: request.host.clone(),
                protocol: request.protocol.approval_protocol(),
                port: request.port,
            };

            if let Some(cached) = approval_service.cached_decision(&host_key) {
                return match cached {
                    crate::control::network_approval::CachedDecision::Allowed => {
                        network_proxy::NetworkDecision::Allow
                    }
                    crate::control::network_approval::CachedDecision::Denied => {
                        network_proxy::NetworkDecision::deny("session_denied")
                    }
                };
            }

            let pending_key = crate::control::network_approval::PendingHostApprovalKey {
                profile_id: profile_id.clone(),
                host: request.host.clone(),
                protocol: request.protocol.approval_protocol(),
            };

            let begin = approval_service.begin_or_join(pending_key);
            match begin {
                crate::control::network_approval::BeginResult::Owner(owner) => {
                    let protocol_name = match request.protocol.approval_protocol() {
                        types::NetworkApprovalProtocol::Http => "http",
                        types::NetworkApprovalProtocol::Https => "https",
                        types::NetworkApprovalProtocol::Socks5Tcp => "socks5",
                        types::NetworkApprovalProtocol::Socks5Udp => "socks5-udp",
                    };

                    let (_, hitl_gate) = session.ensure_thread_controls();
                    let outcome = super::hitl_bridge::park_network_approval(
                        &hitl_gate,
                        &session,
                        &turn_context,
                        &tool_call_id,
                        super::hitl_bridge::NetworkApprovalRequest {
                            host: request.host.clone(),
                            protocol: protocol_name.to_string(),
                            port: request.port,
                            profile_id: profile_id.clone(),
                            command_preview,
                        },
                    )
                    .await;

                    match outcome {
                        Some(o) => {
                            let decision = o.decision.clone();
                            owner.resolve(decision);
                            match o.decision {
                                crate::control::network_approval::PendingApprovalDecision::Allow(
                                    _,
                                ) => network_proxy::NetworkDecision::Allow,
                                crate::control::network_approval::PendingApprovalDecision::Deny => {
                                    network_proxy::NetworkDecision::deny("user_denied")
                                }
                            }
                        }
                        None => {
                            drop(owner);
                            network_proxy::NetworkDecision::deny("hitl_unavailable")
                        }
                    }
                }
                crate::control::network_approval::BeginResult::Joined(rx) => match rx.await {
                    Ok(crate::control::network_approval::PendingApprovalDecision::Allow(_)) => {
                        network_proxy::NetworkDecision::Allow
                    }
                    _ => network_proxy::NetworkDecision::deny("shared_denied"),
                },
            }
        }) as network_proxy::NetworkPolicyDeciderFuture<'_>
    })
}

fn sandbox_policy_for_call(
    session: &AgentLoop,
    step_context: &StepContext,
    call: &types::ParsedToolCall,
    workspace_write_grant: bool,
    managed_network: Option<&Arc<network_proxy::StartedNetworkProxy>>,
) -> Result<Option<sandbox::SandboxPolicy>, crate::runtime::ToolCallError> {
    let preference = step_context.tool_router.sandbox_preference(&call.name);
    if preference == types::SandboxablePreference::Forbid {
        return Ok(None);
    }
    let execution_root = step_context
        .turn
        .project_root()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| session.memory().workspace_dir.clone());
    let mut policy = tools::context::build_command_sandbox_policy(
        session.memory_dir(),
        &execution_root,
        step_context.turn.permission_profile(),
        workspace_write_grant,
        None,
    )
    .map_err(crate::runtime::ToolCallError::from)?;
    if preference == types::SandboxablePreference::Require
        && policy.mode == types::SandboxMode::DangerFullAccess
    {
        policy = sandbox::SandboxPolicy::unrestricted_file_system(
            &execution_root,
            policy.network_access,
        )
        .map_err(anyhow::Error::new)
        .map_err(crate::runtime::ToolCallError::from)?;
    }
    if let Some(started) = managed_network {
        let context = started
            .proxy()
            .prepare(std::collections::HashMap::new())
            .sandbox_context;
        policy = policy.with_managed_network(context);
    }
    Ok(Some(policy))
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

/// 触发 `PostApprovalResponse`（观察型，忽略返回值）：`choice` 为
/// `auto`（辅模型降级）/ `allow`（用户批准）/ `deny`（用户拒绝或 cancelled）/
/// `timeout`（park 超时）/ `unavailable`（无 HITL gate）。
async fn fire_post_approval_response(
    session: &Arc<AgentLoop>,
    session_id: &str,
    turn_id: Option<&str>,
    command: &str,
    choice: &str,
) {
    let agent = session.as_ref();
    agent.fire_hook(
        hooks::POST_APPROVAL_RESPONSE,
        hooks::HookPayload {
            session_id: session_id.to_string(),
            turn_id: turn_id.map(str::to_string),
            tool_name: Some("Bash".to_string()),
            tool_input: Some(serde_json::json!({ "command": command })),
            detail: format!("surface=terminal choice={choice}"),
            ..Default::default()
        },
    );
}

async fn fire_post_permission_response(
    session: &Arc<AgentLoop>,
    session_id: &str,
    turn_id: Option<&str>,
    request: &types::PermissionRequest,
    choice: &str,
) {
    let agent = session.as_ref();
    agent.fire_hook(
        hooks::POST_APPROVAL_RESPONSE,
        hooks::HookPayload {
            session_id: session_id.to_string(),
            turn_id: turn_id.map(str::to_string),
            tool_name: Some(request.tool_name.clone()),
            tool_input: Some(serde_json::json!({ "summary": request.summary })),
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
    thread_memory_mode: types::ThreadMemoryMode,
}

impl PermissionAuditReceipt {
    fn new(
        memory_dir: std::path::PathBuf,
        settings: &memory::LoadedPermissionSettings,
        profile_id: String,
        request: types::PermissionRequest,
        thread_memory_mode: types::ThreadMemoryMode,
    ) -> Self {
        let snapshot_hash = memory::permission_snapshot_hash(settings, &profile_id);
        Self {
            memory_dir,
            profile_id,
            snapshot_hash,
            request,
            thread_memory_mode,
        }
    }

    fn record(
        &self,
        kind: memory::PermissionAuditKind,
        reviewer: Option<types::ApprovalsReviewer>,
        result: Option<&str>,
        duration_ms: Option<u64>,
    ) {
        if self.thread_memory_mode == types::ThreadMemoryMode::Disabled {
            return;
        }
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
    session: &Arc<AgentLoop>,
    selection: &types::SessionPermissions,
    audit: PermissionAuditReceipt,
    turn_context: &TurnContext,
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
    let permission_hook = {
        let agent = session.as_ref();
        agent.fire_permission_request_hook(hooks::HookPayload {
            session_id: request.session_id.clone(),
            turn_id: request.turn_id.clone(),
            tool_name: Some(request.tool_name.clone()),
            tool_input: Some(serde_json::json!({ "summary": request.summary })),
            detail: hook_detail.to_string(),
            ..Default::default()
        })
    };

    match permission_hook {
        hooks::PermissionRequestDecision::Deny(reason) => {
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
                "hook_denied",
                false,
                review_started.elapsed().as_millis() as u64,
            );
            return Some(PermissionPreflight::Denied(format!(
                "Permission denied by hook: {reason}"
            )));
        }
        hooks::PermissionRequestDecision::Allow => {
            fire_post_permission_response(
                session,
                &request.session_id,
                request.turn_id.as_deref(),
                request,
                "allow",
            )
            .await;
            audit.record_review(
                selection.approvals_reviewer,
                "hook_allowed",
                true,
                review_started.elapsed().as_millis() as u64,
            );
            return Some(PermissionPreflight::Granted(Box::new(audit)));
        }
        hooks::PermissionRequestDecision::Abstain => {}
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
            let agent = session.as_ref();
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
    let Some(confirm) = park_confirm(
        gate,
        session.as_ref(),
        turn_context,
        &request.tool_call_id,
        title,
        body,
        false,
    )
    .await
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
    session: &Arc<AgentLoop>,
    call: &types::ParsedToolCall,
    description: &str,
) {
    let (memory_dir, settings, profile_id, session_id, turn_id) = {
        let agent = session.as_ref();
        let settings = memory::load_permission_settings(agent.memory_dir());
        let profile_id = agent
            .permission_profile()
            .unwrap_or_else(|| settings.selection.profile_id.clone());
        (
            agent.memory_dir().to_path_buf(),
            settings,
            profile_id,
            agent.session_id().to_string(),
            agent.current_turn_id().await,
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
    let audit = PermissionAuditReceipt::new(
        memory_dir,
        &settings,
        profile_id,
        request,
        session.config.thread_memory_mode,
    );
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
    session: &Arc<AgentLoop>,
    call: &types::ParsedToolCall,
    turn_context: &TurnContext,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<PermissionPreflight> {
    if !tools::tool_requires_in_process_write(&call.name, &call.arguments) {
        return Some(PermissionPreflight::NotRequired);
    }

    let (session_id, turn_id, profile_id, memory_dir, settings) = {
        let agent = session.as_ref();
        let session_id = agent.session_id().to_string();
        let turn_id = agent.current_turn_id().await;
        let settings = memory::load_permission_settings(agent.memory_dir());
        let profile_id = agent
            .permission_profile()
            .unwrap_or_else(|| settings.selection.profile_id.clone());
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
    let audit = PermissionAuditReceipt::new(
        memory_dir,
        &settings,
        profile_id,
        request,
        session.config.thread_memory_mode,
    );
    review_once_permission(
        session,
        &selection,
        audit,
        turn_context,
        hitl_gate,
        "surface=permission reason=read_only_mutation",
        "批准本次写入",
        &body,
    )
    .await
}

async fn preflight_mcp_tool_approval(
    session: &Arc<AgentLoop>,
    step_context: &StepContext,
    call: &types::ParsedToolCall,
    turn_context: &TurnContext,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<PermissionPreflight> {
    let approval = step_context.tool_router.mcp_approval(&call.name);
    let Some(approval) = approval else {
        return Some(PermissionPreflight::NotRequired);
    };
    let route = approval.route();
    if route == types::McpToolApprovalRoute::Allow {
        return Some(PermissionPreflight::NotRequired);
    }

    let (session_id, turn_id, profile_id, memory_dir, settings) = {
        let agent = session.as_ref();
        let settings = memory::load_permission_settings(agent.memory_dir());
        let profile_id = agent
            .permission_profile()
            .unwrap_or_else(|| settings.selection.profile_id.clone());
        (
            agent.session_id().to_string(),
            agent.current_turn_id().await,
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
    let audit = PermissionAuditReceipt::new(
        memory_dir,
        &settings,
        profile_id,
        request,
        session.config.thread_memory_mode,
    );
    review_once_permission(
        session,
        &selection,
        audit,
        turn_context,
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
    session: &Arc<AgentLoop>,
    call: &types::ParsedToolCall,
    turn_context: &TurnContext,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<PermissionPreflight> {
    if !tools::tool_requires_in_process_network(&call.name) {
        return Some(PermissionPreflight::NotRequired);
    }

    let (session_id, turn_id, profile_id, memory_dir, settings) = {
        let agent = session.as_ref();
        let settings = memory::load_permission_settings(agent.memory_dir());
        let profile_id = agent
            .permission_profile()
            .unwrap_or_else(|| settings.selection.profile_id.clone());
        (
            agent.session_id().to_string(),
            agent.current_turn_id().await,
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
    let audit = PermissionAuditReceipt::new(
        memory_dir,
        &settings,
        profile_id,
        request,
        session.config.thread_memory_mode,
    );
    review_once_permission(
        session,
        &selection,
        audit,
        turn_context,
        hitl_gate,
        "surface=permission reason=network_disabled",
        "批准本次网络访问",
        &body,
    )
    .await
}

async fn review_sandbox_denial(
    session: &Arc<AgentLoop>,
    step_context: &StepContext,
    call: &types::ParsedToolCall,
    output: &sandbox::ExecToolCallOutput,
    turn_context: &TurnContext,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<PermissionPreflight> {
    let settings = memory::load_permission_settings(session.memory_dir());
    let profile_id = step_context
        .turn
        .permission_profile()
        .unwrap_or(settings.selection.profile_id.as_str())
        .to_string();
    let selection = settings.selection.clone();
    let command_preview = call
        .arguments
        .get("command")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let request = types::PermissionRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        session_id: session.session_id().to_string(),
        turn_id: Some(turn_context.sub_id().to_string()),
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        summary: format!(
            "Retry {} once with full local filesystem access after sandbox denial",
            call.name
        ),
        capabilities: vec![
            types::PermissionCapability::ProcessSpawn {
                program: call.name.clone(),
                cwd: None,
            },
            types::PermissionCapability::FileWrite {
                paths: vec!["outside configured writable roots".to_string()],
            },
        ],
        reason: types::PermissionReason::SandboxDenied,
        requested_scope: types::GrantScope::Once,
        command_preview,
        affected_paths: vec!["outside configured writable roots".to_string()],
        network_hosts: Vec::new(),
    };
    let denial_detail = output
        .aggregated_output
        .chars()
        .take(800)
        .collect::<String>();
    let body = format!(
        "The sandbox denied this tool attempt:\n\n```text\n{denial_detail}\n```\n\nRetry this exact call once with full local filesystem access? Network permissions are unchanged, and the grant will not persist."
    );
    let audit = PermissionAuditReceipt::new(
        session.memory_dir().to_path_buf(),
        &settings,
        profile_id,
        request,
        session.config.thread_memory_mode,
    );
    review_once_permission(
        session,
        &selection,
        audit,
        turn_context,
        hitl_gate,
        "surface=permission reason=sandbox_denied",
        "Retry outside sandbox",
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
    session: &Arc<AgentLoop>,
    step_context: Arc<StepContext>,
    calls: &[types::ParsedToolCall],
    pause: &Arc<PauseControl>,
    turn_context: &TurnContext,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<Vec<types::ToolOutput>> {
    execute_tools_serial_inner(session, step_context, calls, pause, turn_context, hitl_gate).await
}

async fn execute_tools_serial_inner(
    session: &Arc<AgentLoop>,
    step_context: Arc<StepContext>,
    calls: &[types::ParsedToolCall],
    pause: &Arc<PauseControl>,
    turn_context: &TurnContext,
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
        if !step_context.advertises_tool(&call.name) {
            out.push(
                format!(
                    "工具 `{}` 不在生成本次调用的 StepContext 中，已拒绝执行。",
                    call.name
                )
                .into(),
            );
            continue;
        }

        let mut workspace_write_grant = false;
        let mut network_grant = tools::InProcessNetworkGrant::default();
        let mut permission_audits = Vec::new();
        if !call.args_parse_error {
            match preflight_mcp_tool_approval(session, &step_context, call, turn_context, hitl_gate)
                .await?
            {
                PermissionPreflight::NotRequired => {}
                PermissionPreflight::Granted(audit) => permission_audits.push(*audit),
                PermissionPreflight::Denied(message) => {
                    out.push(format!(
                        "{message}. Do not retry the same action or attempt a workaround without explicit authorization."
                    ).into());
                    continue;
                }
            }
            match preflight_read_only_write(session, call, turn_context, hitl_gate).await? {
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
            match preflight_in_process_network(session, call, turn_context, hitl_gate).await? {
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
                        // 读取审批模式 + 白名单，并触发 PermissionRequest 钩子
                        let (
                            approval_session_id,
                            approval_turn_id,
                            permissions,
                            allowlist,
                            memory_dir,
                            active_profile_id,
                            permission_settings,
                        ) = {
                            let agent = session.as_ref();
                            let approval_session_id = agent.session_id().to_string();
                            let approval_turn_id = agent.current_turn_id().await;
                            let base = agent.memory_dir().to_path_buf();
                            let permissions = memory::config::load_permission_settings(&base);
                            let active_profile_id = agent
                                .permission_profile()
                                .unwrap_or_else(|| permissions.selection.profile_id.clone());
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
                            session.config.thread_memory_mode,
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
                        let permission_hook =
                            session.fire_permission_request_hook(hooks::HookPayload {
                                session_id: approval_session_id.clone(),
                                turn_id: approval_turn_id.clone(),
                                tool_name: Some("Bash".to_string()),
                                tool_input: Some(serde_json::json!({ "command": cmd })),
                                detail: format!("surface=terminal ask={}", decision.description),
                                ..Default::default()
                            });
                        if let hooks::PermissionRequestDecision::Deny(reason) = &permission_hook {
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
                                "hook_denied",
                                false,
                                approval_started.elapsed().as_millis() as u64,
                            );
                            out.push(
                                format!("Command denied by PermissionRequest hook: {reason}")
                                    .into(),
                            );
                            continue;
                        }
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
                        } else if permission_hook == hooks::PermissionRequestDecision::Allow {
                            fire_post_approval_response(
                                session,
                                &approval_session_id,
                                approval_turn_id.as_deref(),
                                &cmd,
                                "allow",
                            )
                            .await;
                            approval_audit.record_review(
                                permissions.approvals_reviewer,
                                "hook_allowed",
                                true,
                                approval_started.elapsed().as_millis() as u64,
                            );
                            permission_audits.push(approval_audit);
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
                                let agent = session.as_ref();
                                let targets: Vec<_> = agent
                                    .auxiliary_targets(types::AuxiliaryTask::SmartApproval)
                                    .iter()
                                    .map(crate::control::smart_approval::ApprovalTarget::from)
                                    .collect();
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
                                let confirm = park_confirm(
                                    gate,
                                    session.as_ref(),
                                    turn_context,
                                    &call.id,
                                    title,
                                    &body,
                                    true,
                                )
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
            let agent = session.as_ref();
            let memory_dir = agent.memory_dir().to_path_buf();
            let session_id = agent.session_id().to_string();
            let turn_ctx_arc = session.current_turn_context().await;
            let managed_network =
                match start_managed_network(session, &step_context, call, turn_ctx_arc).await {
                    Ok(managed_network) => managed_network,
                    Err(error) => {
                        if session.config.thread_memory_mode == types::ThreadMemoryMode::Enabled {
                            memory::try_append_decision(
                                &memory_dir,
                                memory::DecisionEntry::new(
                                    memory::DecisionKind::ToolFailure,
                                    format!("managed network setup failed: {error}"),
                                )
                                .with_tool(call.name.clone())
                                .with_session(session_id.clone()),
                            );
                        }
                        out.push(
                            format!("Tool error: managed network setup failed: {error}").into(),
                        );
                        continue;
                    }
                };
            let sandbox_policy = match sandbox_policy_for_call(
                agent,
                &step_context,
                call,
                workspace_write_grant,
                managed_network.as_ref(),
            ) {
                Ok(policy) => policy,
                Err(error) => {
                    out.push(format!("Tool error: sandbox setup failed: {error}").into());
                    continue;
                }
            };
            let execution_started = std::time::Instant::now();
            let executed = session.handle_tool_invocation_with_once_grants(
                ToolInvocation {
                    session: Arc::clone(session),
                    step_context: Arc::clone(&step_context),
                    cancellation_token: CancellationToken::new(),
                    call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    payload: call.arguments.clone(),
                },
                ToolExecutionGrants {
                    workspace_write: workspace_write_grant,
                    sandbox_policy,
                    network: network_grant.clone(),
                    managed_network: managed_network.clone(),
                },
            );
            let execution_result = match &executed {
                Ok(_) => "success",
                Err(crate::runtime::ToolCallError::Cancelled) => "cancelled",
                Err(crate::runtime::ToolCallError::SandboxDenied(_)) => "sandbox_denied",
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
                Err(crate::runtime::ToolCallError::SandboxDenied(
                    sandbox::SandboxErr::Denied {
                        output,
                        network_policy_decision,
                    },
                )) => {
                    let denial_output = output.aggregated_output.clone();
                    if network_policy_decision.is_some() {
                        agent
                            .finalize_tool_call_result(
                                &call.name,
                                &call.arguments,
                                denial_output.into(),
                            )
                            .await
                    } else {
                        match review_sandbox_denial(
                            session,
                            &step_context,
                            call,
                            output.as_ref(),
                            turn_context,
                            hitl_gate,
                        )
                        .await?
                        {
                            PermissionPreflight::Granted(retry_audit) => {
                                let execution_root = step_context
                                    .turn
                                    .project_root()
                                    .map(ToOwned::to_owned)
                                    .unwrap_or_else(|| agent.memory().workspace_dir.clone());
                                let mut retry_policy =
                                    match sandbox::SandboxPolicy::unrestricted_file_system(
                                        &execution_root,
                                        false,
                                    ) {
                                        Ok(policy) => policy,
                                        Err(error) => {
                                            out.push(
                                                format!(
                                                "Tool error after sandbox escalation setup: {error}"
                                            )
                                                .into(),
                                            );
                                            continue;
                                        }
                                    };
                                if let Some(started) = managed_network.as_ref() {
                                    let context = started
                                        .proxy()
                                        .prepare(std::collections::HashMap::new())
                                        .sandbox_context;
                                    retry_policy = retry_policy.with_managed_network(context);
                                }
                                let retry_started = std::time::Instant::now();
                                let retry = session.handle_tool_invocation_with_once_grants(
                                    ToolInvocation {
                                        session: Arc::clone(session),
                                        step_context: Arc::clone(&step_context),
                                        cancellation_token: CancellationToken::new(),
                                        call_id: call.id.clone(),
                                        tool_name: call.name.clone(),
                                        payload: call.arguments.clone(),
                                    },
                                    ToolExecutionGrants {
                                        workspace_write: workspace_write_grant,
                                        sandbox_policy: Some(retry_policy),
                                        network: network_grant.clone(),
                                        managed_network: managed_network.clone(),
                                    },
                                );
                                let retry_result = match &retry {
                                    Ok(_) => "escalated",
                                    Err(crate::runtime::ToolCallError::Cancelled) => "cancelled",
                                    Err(crate::runtime::ToolCallError::SandboxDenied(_)) => {
                                        "sandbox_denied"
                                    }
                                    Err(_) => "error",
                                };
                                retry_audit.record(
                                    memory::PermissionAuditKind::Applied,
                                    None,
                                    Some(retry_result),
                                    Some(retry_started.elapsed().as_millis() as u64),
                                );
                                match retry {
                                    Ok(output) => output,
                                    Err(crate::runtime::ToolCallError::Cancelled) => return None,
                                    Err(crate::runtime::ToolCallError::SandboxDenied(
                                        sandbox::SandboxErr::Denied { output, .. },
                                    )) => {
                                        agent
                                            .finalize_tool_call_result(
                                                &call.name,
                                                &call.arguments,
                                                output.aggregated_output.clone().into(),
                                            )
                                            .await
                                    }
                                    Err(error) => {
                                        if session.config.thread_memory_mode
                                            == types::ThreadMemoryMode::Enabled
                                        {
                                            memory::try_append_decision(
                                                &memory_dir,
                                                memory::DecisionEntry::new(
                                                    memory::DecisionKind::ToolFailure,
                                                    error.to_string(),
                                                )
                                                .with_tool(call.name.clone())
                                                .with_session(session_id.clone()),
                                            );
                                        }
                                        format!("Tool error after sandbox escalation: {error}")
                                            .into()
                                    }
                                }
                            }
                            PermissionPreflight::Denied(message) => {
                                agent
                                    .finalize_tool_call_result(
                                        &call.name,
                                        &call.arguments,
                                        format!(
                                            "{denial_output}\n\nSandbox retry denied: {message}"
                                        )
                                        .into(),
                                    )
                                    .await
                            }
                            PermissionPreflight::NotRequired => {
                                agent
                                    .finalize_tool_call_result(
                                        &call.name,
                                        &call.arguments,
                                        denial_output.into(),
                                    )
                                    .await
                            }
                        }
                    }
                }
                Err(e) => {
                    if session.config.thread_memory_mode == types::ThreadMemoryMode::Enabled {
                        memory::try_append_decision(
                            &memory_dir,
                            memory::DecisionEntry::new(
                                memory::DecisionKind::ToolFailure,
                                format!("{e}"),
                            )
                            .with_tool(call.name.clone())
                            .with_session(session_id),
                        );
                    }
                    format!("工具错误: {e}").into()
                }
            }
        };

        // confirm/clarify：astro_hitl → 同回合 park
        if let Some(hitl) = parse_astro_hitl(result.text()) {
            if let Some(gate) = hitl_gate {
                result = park_astro_hitl(gate, session.as_ref(), turn_context, &call.id, hitl)
                    .await?
                    .into();
            } else {
                // 无 HitlGate（单测或未注入闸门）：无法 park，返回说明文案
                result = "HITL gate unavailable; confirmation/clarification could not be shown to the user.".into();
            }
        }

        if let (true, Some(async_message)) = (
            call.name == "send_user_message_async",
            tools::parse_async_user_message(result.text()),
        ) {
            emit_async_agent_message(
                session,
                turn_context,
                format!("{}:async-message", call.id),
                async_message.message,
            )
            .await;
            result = serde_json::json!({"accepted": true}).to_string().into();
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
    session: &Arc<AgentLoop>,
    step_context: Arc<StepContext>,
    calls: &[types::ParsedToolCall],
    pause: &Arc<PauseControl>,
) -> Option<Vec<types::ToolOutput>> {
    if pause.is_cancelled() || !pause.wait_if_paused().await {
        return None;
    }

    let turn_context = Arc::clone(&step_context.turn);
    let runtime = ToolCallRuntime::new(Arc::clone(session), step_context);

    let mut join_set = JoinSet::new();
    for (idx, call) in calls.iter().cloned().enumerate() {
        let runtime = runtime.clone();
        let child_permit = turn_context.track_child();
        join_set.spawn_blocking(move || {
            let _child_permit = child_permit;
            let tool_name = call.name.clone();
            let result = if call.args_parse_error {
                Ok(types::ToolOutput::from(format!(
                    "工具参数 JSON 解析失败: {}",
                    call.arguments
                        .get("_parse_error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("invalid json")
                )))
            } else {
                runtime.handle_tool_call(call, CancellationToken::new())
            };
            (idx, tool_name, result)
        });
    }

    let mut slots: Vec<Option<types::ToolOutput>> = (0..calls.len()).map(|_| None).collect();
    let mut cancelled = false;
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((idx, tool_name, result)) => {
                let result = match result {
                    Ok(output) => output,
                    Err(crate::runtime::ToolCallError::Cancelled) => {
                        cancelled = true;
                        continue;
                    }
                    Err(error) => {
                        if session.config.thread_memory_mode == types::ThreadMemoryMode::Enabled {
                            memory::try_append_decision(
                                session.memory_dir(),
                                memory::DecisionEntry::new(
                                    memory::DecisionKind::ToolFailure,
                                    error.to_string(),
                                )
                                .with_tool(tool_name)
                                .with_session(session.session_id().to_string()),
                            );
                        }
                        format!("工具错误: {error}").into()
                    }
                };
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
    if cancelled {
        return None;
    }
    Some(
        slots
            .into_iter()
            .map(|s| s.unwrap_or_else(|| "工具错误: missing result".into()))
            .collect(),
    )
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
            types::ThreadMemoryMode::Enabled,
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

    #[tokio::test]
    async fn permission_request_hook_can_allow_or_deny_before_hitl() {
        fn request(session_id: &str) -> types::PermissionRequest {
            types::PermissionRequest {
                request_id: "hook-request".into(),
                session_id: session_id.into(),
                turn_id: Some("turn-hook".into()),
                tool_call_id: "call-hook".into(),
                tool_name: "terminal".into(),
                summary: "run command".into(),
                capabilities: Vec::new(),
                reason: types::PermissionReason::RulePrompt,
                requested_scope: types::GrantScope::Once,
                command_preview: Some("echo ok".into()),
                affected_paths: Vec::new(),
                network_hosts: Vec::new(),
            }
        }

        let allow_dir = tempfile::tempdir().unwrap();
        let allow_session = Arc::new(
            AgentLoop::new(crate::runtime::Config::with_defaults(
                allow_dir.path().to_path_buf(),
            ))
            .unwrap(),
        );
        allow_session
            .hook_bus()
            .register(hooks::PERMISSION_REQUEST, |_| hooks::HookOutcome::Allow);
        let allow_request = request(allow_session.session_id());
        let allow_audit = PermissionAuditReceipt::new(
            allow_dir.path().to_path_buf(),
            &memory::LoadedPermissionSettings::default(),
            types::WORKSPACE_PROFILE.into(),
            allow_request,
            types::ThreadMemoryMode::Enabled,
        );
        let allow_context = allow_session.create_turn_context("turn-hook".into()).await;
        let allowed = review_once_permission(
            &allow_session,
            &types::SessionPermissions::ask_for_approval(),
            allow_audit,
            &allow_context,
            None,
            "hook allow",
            "title",
            "body",
        )
        .await;
        assert!(matches!(allowed, Some(PermissionPreflight::Granted(_))));

        let deny_dir = tempfile::tempdir().unwrap();
        let deny_session = Arc::new(
            AgentLoop::new(crate::runtime::Config::with_defaults(
                deny_dir.path().to_path_buf(),
            ))
            .unwrap(),
        );
        deny_session
            .hook_bus()
            .register(hooks::PERMISSION_REQUEST, |_| {
                hooks::HookOutcome::Block("organization policy".into())
            });
        let deny_request = request(deny_session.session_id());
        let deny_audit = PermissionAuditReceipt::new(
            deny_dir.path().to_path_buf(),
            &memory::LoadedPermissionSettings::default(),
            types::WORKSPACE_PROFILE.into(),
            deny_request,
            types::ThreadMemoryMode::Enabled,
        );
        let deny_context = deny_session.create_turn_context("turn-hook".into()).await;
        let denied = review_once_permission(
            &deny_session,
            &types::SessionPermissions::ask_for_approval(),
            deny_audit,
            &deny_context,
            None,
            "hook deny",
            "title",
            "body",
        )
        .await;
        assert!(matches!(
            denied,
            Some(PermissionPreflight::Denied(message)) if message.contains("organization policy")
        ));
    }
}
