//! 单轮工具调用执行：串行（HITL/危险命令走 park）与并发（普通工具）两条路径。

use std::sync::Arc;

use providers::PauseControl;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::control::hitl::HitlGate;
use crate::runtime::{
    AgentLoop, StepContext, ToolCallRuntime, ToolExecutionGrants, ToolInvocation, TurnContext,
};

use super::hitl_bridge::{park_astro_hitl, park_confirm, parse_astro_hitl, ConfirmPresentation};
use super::lifecycle::emit_async_agent_message;

use crate::control::smart_approval::{SmartApprovalContext, TurnSummary};

async fn build_smart_approval_context(session: &Arc<AgentLoop>) -> Option<SmartApprovalContext> {
    let history = session.tail_history(5);
    let recent: Vec<TurnSummary> = history
        .iter()
        .rev()
        .filter_map(|item| {
            let role = item.role().unwrap_or_else(|| {
                if item.is_tool_output() {
                    "tool"
                } else {
                    "assistant"
                }
            });
            let text = item.text();
            if text.is_empty() {
                return None;
            }
            Some(TurnSummary {
                role: role.to_string(),
                content_preview: crate::control::smart_approval::truncate_preview(&text),
            })
        })
        .collect();
    if recent.is_empty() {
        return None;
    }
    Some(SmartApprovalContext {
        recent_turns: recent,
        current_task_description: None,
        tool_call_chain: vec![],
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalRoute {
    Deny,
    Allowlist,
    TypeAllowlist,
    Off,
    Smart,
    Manual,
}

fn call_uses_managed_network(call: &types::ParsedToolCall) -> bool {
    match call.name.as_str() {
        "code_exec" => true,
        "exec_command" => match call.arguments.get("action") {
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
    if !step_context.turn.network_access() {
        return Ok(None);
    }
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

                    let (_, hitl_gate, _) = session.ensure_thread_controls();
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
    let mut policy = tools::context::build_command_sandbox_policy_with_roots(
        session.memory_dir(),
        &execution_root,
        step_context.turn.workspace_roots(),
        step_context.turn.permission_profile(),
        workspace_write_grant,
        None,
    )
    .map_err(crate::runtime::ToolCallError::from)?;
    policy.network_access &= step_context.turn.network_access();
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
    risk: &str,
    selection: &types::SessionPermissions,
    allowlist: &[String],
    type_allowlist: &[memory::CommandTypeRule],
) -> ApprovalRoute {
    if tools::is_hardline_blocked(command).is_some() {
        ApprovalRoute::Deny
    } else if tools::matches_allowlist(command, allowlist) {
        ApprovalRoute::Allowlist
    } else if tools::matches_command_type_allowlist(command, risk, type_allowlist) {
        ApprovalRoute::TypeAllowlist
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
    presentation: ConfirmPresentation<'_>,
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
        let hook_request = agent.permission_request_hook(
            request.turn_id.clone(),
            request.tool_name.clone(),
            request.tool_call_id.clone(),
            serde_json::json!({ "summary": request.summary, "detail": hook_detail }),
        );
        agent.run_permission_request_hook(hook_request)
    };

    match permission_hook.decision {
        Some(hooks::PermissionHookDecision::Deny { message }) => {
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
                "Permission denied by hook: {message}"
            )));
        }
        Some(hooks::PermissionHookDecision::Allow) => {
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
        None => {}
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
        let smart_ctx = build_smart_approval_context(session).await;
        let action = crate::control::smart_approval::maybe_smart_downgrade_ask(
            request,
            &targets,
            smart_ctx.as_ref(),
        )
        .await;
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
        // 辅模型未放行时回退到用户手动审批（有 HITL gate 的情况下）
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
        presentation,
        false,
        None,
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
    } else if confirm.status == "timeout" {
        Some(PermissionPreflight::Denied(
            "Tool error: permission approval request timed out".to_string(),
        ))
    } else {
        Some(PermissionPreflight::Denied(
            "Permission denied by user".to_string(),
        ))
    }
}

fn affected_write_paths(name: &str, _args: &serde_json::Value) -> Vec<String> {
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
        ConfirmPresentation::Text {
            title: "批准本次写入",
            body: &body,
        },
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
        ConfirmPresentation::Text {
            title: "批准 MCP 工具调用",
            body: &body,
        },
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
        ConfirmPresentation::SandboxRetry {
            denial_detail: &denial_detail,
        },
    )
    .await
}

pub(crate) fn tool_may_require_permission(name: &str, args: &serde_json::Value) -> bool {
    match name {
        // 权限 profile 和命令规则都可能要求 park；统一走串行 preflight。
        "exec_command" | "code_exec" => true,
        _ => {
            tools::browser::approval_class(name, args).is_some()
                || tools::tool_requires_in_process_write(name, args)
        }
    }
}

#[derive(serde::Deserialize)]
struct CodeModeWaitArgs {
    cell_id: String,
    #[serde(default = "default_code_mode_wait_ms")]
    yield_time_ms: u64,
    #[serde(default = "default_code_mode_max_tokens")]
    max_tokens: usize,
    #[serde(default)]
    terminate: bool,
}

fn default_code_mode_wait_ms() -> u64 {
    10_000
}

fn default_code_mode_max_tokens() -> usize {
    10_000
}

fn code_mode_nested_tools(
    session: &AgentLoop,
) -> Vec<crate::runtime::code_mode::NestedToolMetadata> {
    let registry = session
        .services
        .tool_registry
        .read()
        .expect("tool registry lock poisoned");
    let mut by_identifier = std::collections::BTreeMap::new();
    for entry in registry.available_tools() {
        if matches!(entry.name.as_str(), "exec" | "wait" | "tool_search")
            || entry.exposure == types::ToolExposure::Hidden
        {
            continue;
        }
        let child_name = entry
            .name
            .strip_prefix(&format!("{}_", entry.namespace))
            .or_else(|| entry.name.strip_prefix(&format!("{}.", entry.namespace)))
            .unwrap_or(&entry.name);
        let wire_name = if entry.namespace.is_empty() {
            entry.name.clone()
        } else {
            format!("{}.{}", entry.namespace, child_name)
        };
        let name = crate::runtime::code_mode::normalize_identifier(&wire_name);
        by_identifier.entry(name.clone()).or_insert_with(|| {
            // 与 Codex 一致：ALL_TOOLS 只保留 name/description，但 description
            // 自带精确调用声明，因此不需要维护另一套 getToolSchema API。
            let description = tools::render_code_mode_tool_description(
                &name,
                &entry.description,
                &tools::sanitize_tool_schema(entry.schema.clone()),
                entry.freeform_format.as_ref(),
            );
            crate::runtime::code_mode::NestedToolMetadata {
                name,
                wire_name,
                description,
            }
        });
    }
    by_identifier.into_values().collect()
}

fn code_mode_nested_result(output: types::ToolOutput) -> serde_json::Value {
    let (text, media) = output.into_parts();
    if media.is_empty() {
        return serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
    }
    serde_json::json!({
        "content": [{"type":"text","text":text}],
        "media": media,
    })
}

#[allow(clippy::too_many_arguments)]
async fn drive_code_mode_cell(
    session: &Arc<AgentLoop>,
    step_context: Arc<StepContext>,
    cell_id: &str,
    yield_time_ms: u64,
    max_tokens: usize,
    pause: &Arc<PauseControl>,
    turn_context: &TurnContext,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Result<types::ToolOutput, crate::runtime::ToolCallError> {
    let started = std::time::Instant::now();
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_millis(yield_time_ms.min(300_000));
    let mut output = Vec::new();
    let status = loop {
        if pause.is_cancelled() {
            let _ = session.services.code_mode.terminate(cell_id).await;
            return Err(crate::runtime::ToolCallError::Cancelled);
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break format!("Script running with cell ID {cell_id}");
        }
        match session
            .services
            .code_mode
            .next_event(cell_id, remaining)
            .await
            .map_err(crate::runtime::ToolCallError::from)?
        {
            crate::runtime::code_mode::NextEvent::TimedOut => {
                break format!("Script running with cell ID {cell_id}");
            }
            crate::runtime::code_mode::NextEvent::Closed(error) => {
                session.services.code_mode.close(cell_id).await;
                output.push(format!("Script error:\n{error}"));
                break "Script failed".to_string();
            }
            crate::runtime::code_mode::NextEvent::Event(event) => match event {
                crate::runtime::code_mode::RuntimeEvent::Content {
                    kind,
                    value,
                    detail,
                } => {
                    let rendered = value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string());
                    if kind == "text" {
                        output.push(rendered);
                    } else {
                        let detail = detail
                            .map(|value| format!(", detail={value}"))
                            .unwrap_or_default();
                        output.push(format!("[{kind}{detail}] {rendered}"));
                    }
                }
                crate::runtime::code_mode::RuntimeEvent::Store { key, value } => {
                    session.services.code_mode.update_store(key, value).await;
                }
                crate::runtime::code_mode::RuntimeEvent::Yield => {
                    break format!("Script running with cell ID {cell_id}");
                }
                crate::runtime::code_mode::RuntimeEvent::Result { error } => {
                    session.services.code_mode.close(cell_id).await;
                    if let Some(error) = error {
                        output.push(format!("Script error:\n{error}"));
                        break "Script failed".to_string();
                    }
                    break "Script completed".to_string();
                }
                crate::runtime::code_mode::RuntimeEvent::ToolCall { id, name, input } => {
                    let result = if matches!(name.as_str(), "exec" | "wait") {
                        Err("exec cannot invoke exec or wait as a nested tool".to_string())
                    } else {
                        let nested =
                            types::ParsedToolCall::with_id(format!("exec-{id}"), name, input);
                        match Box::pin(execute_tools_serial_inner(
                            session,
                            Arc::clone(&step_context),
                            std::slice::from_ref(&nested),
                            pause,
                            turn_context,
                            hitl_gate,
                        ))
                        .await
                        {
                            Some(mut values) => values
                                .pop()
                                .map(code_mode_nested_result)
                                .ok_or_else(|| "nested tool returned no result".to_string()),
                            None => return Err(crate::runtime::ToolCallError::Cancelled),
                        }
                    };
                    session
                        .services
                        .code_mode
                        .send_tool_result(cell_id, &id, result)
                        .await
                        .map_err(crate::runtime::ToolCallError::from)?;
                }
            },
        }
    };
    let wall_time = ((started.elapsed().as_secs_f32() * 10.0).round()) / 10.0;
    let body = crate::runtime::code_mode::truncate_output(output.join("\n"), max_tokens);
    Ok(format!("{status}\nWall time {wall_time:.1} seconds\nOutput:\n{body}").into())
}

#[allow(clippy::too_many_arguments)]
async fn execute_code_mode_tool(
    session: &Arc<AgentLoop>,
    step_context: Arc<StepContext>,
    call: &types::ParsedToolCall,
    pause: &Arc<PauseControl>,
    turn_context: &TurnContext,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Result<types::ToolOutput, crate::runtime::ToolCallError> {
    if session.cancel.is_cancelled() {
        return Err(crate::runtime::ToolCallError::Cancelled);
    }
    session.increment_tool_round().await?;
    let mut arguments = call.arguments.clone();
    if call.name == "exec" {
        let request = session.pre_tool_use_request(
            session.current_turn_id().await,
            call.name.clone(),
            call.id.clone(),
            arguments.clone(),
        );
        let outcome = session.run_pre_tool_use_hook(request);
        if outcome.should_block {
            let reason = outcome
                .block_reason
                .unwrap_or_else(|| "PreToolUse hook blocked tool execution".into());
            return Ok(format!("[blocked by hook] {reason}").into());
        }
        if let Some(updated_input) = outcome.updated_input {
            arguments = updated_input;
        }
    }
    if let Err(message) = tools::check_tool_call(turn_context.mode(), &call.name, &arguments) {
        return Ok(message.into());
    }
    let agent_id = session.memory().agent_id.clone();
    let _ = home::record_tool_call(&agent_id, &call.name, &arguments);
    let turn_id = session.current_turn_id().await;
    let _ = usage::record_tool_call(
        &agent_id,
        &call.name,
        &arguments,
        Some(session.session_id()),
        turn_id.as_deref(),
    )
    .await;

    let output = match call.name.as_str() {
        "exec" => {
            let source = arguments
                .as_str()
                .or_else(|| arguments.get("input").and_then(serde_json::Value::as_str))
                .ok_or_else(|| anyhow::anyhow!("exec expects raw JavaScript source text"))?;
            let source =
                crate::runtime::code_mode::parse_exec_source(source).map_err(anyhow::Error::msg)?;
            let tools = code_mode_nested_tools(session);
            let execution_root = step_context
                .turn
                .project_root()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| session.memory().workspace_dir.clone());
            let cell_id = session
                .services
                .code_mode
                .execute(&source, &tools, &execution_root)
                .await
                .map_err(crate::runtime::ToolCallError::from)?;
            drive_code_mode_cell(
                session,
                step_context,
                &cell_id,
                source.yield_time_ms,
                source.max_output_tokens,
                pause,
                turn_context,
                hitl_gate,
            )
            .await?
        }
        "wait" => {
            let args: CodeModeWaitArgs = serde_json::from_value(arguments.clone())
                .map_err(|error| anyhow::anyhow!("invalid wait arguments: {error}"))?;
            if args.terminate {
                let terminated = session
                    .services
                    .code_mode
                    .terminate(&args.cell_id)
                    .await
                    .map_err(crate::runtime::ToolCallError::from)?;
                return Ok(if terminated {
                    "Script terminated\nWall time 0.0 seconds\nOutput:\n".into()
                } else {
                    format!(
                        "Script failed\nWall time 0.0 seconds\nOutput:\nScript error:\nexec cell {} not found",
                        args.cell_id
                    )
                    .into()
                });
            }
            if let Err(error) = session.services.code_mode.resume(&args.cell_id).await {
                return Ok(format!(
                    "Script failed\nWall time 0.0 seconds\nOutput:\nScript error:\n{error}"
                )
                .into());
            }
            drive_code_mode_cell(
                session,
                step_context,
                &args.cell_id,
                args.yield_time_ms,
                args.max_tokens,
                pause,
                turn_context,
                hitl_gate,
            )
            .await?
        }
        _ => unreachable!("not a Code Mode control tool"),
    };
    if call.name == "exec" {
        Ok(session
            .finalize_tool_call_result(&call.name, &arguments, output)
            .await)
    } else {
        Ok(output)
    }
}

async fn preflight_browser_action(
    session: &Arc<AgentLoop>,
    call: &types::ParsedToolCall,
    turn_context: &TurnContext,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<PermissionPreflight> {
    let Some(class) =
        tools::browser::effective_approval_class(session.session_id(), &call.name, &call.arguments)
            .await
    else {
        return Some(PermissionPreflight::NotRequired);
    };
    let origin = tools::browser::current_origin(session.session_id())
        .await
        .unwrap_or_else(|| "current page".to_string());
    if class == tools::browser::BrowserApprovalClass::StateChanging
        && tools::browser::approval_rule_matches(session.memory_dir(), &origin, class)
    {
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
    if selection.approval_policy == types::ApprovalPolicy::Never {
        return Some(PermissionPreflight::NotRequired);
    }
    let target = call
        .arguments
        .get("selector")
        .or_else(|| call.arguments.get("text"))
        .and_then(|value| value.as_str())
        .unwrap_or("page element");
    let request = types::PermissionRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        session_id,
        turn_id,
        tool_call_id: call.id.clone(),
        tool_name: call.name.clone(),
        summary: format!(
            "Allow browser {} on {} ({})",
            call.name,
            origin,
            class.as_str()
        ),
        capabilities: vec![types::PermissionCapability::ExternalSideEffect {
            category: format!("browser_{}", class.as_str()),
            target: origin.clone(),
        }],
        reason: types::PermissionReason::RulePrompt,
        requested_scope: types::GrantScope::Once,
        command_preview: None,
        affected_paths: Vec::new(),
        network_hosts: vec![origin.clone()],
    };
    let audit = PermissionAuditReceipt::new(
        memory_dir.clone(),
        &settings,
        profile_id,
        request,
        session.config.thread_memory_mode,
    );
    let started = std::time::Instant::now();
    audit.record(
        memory::PermissionAuditKind::Evaluated,
        None,
        Some("browser_action_approval_required"),
        None,
    );
    audit.record(
        memory::PermissionAuditKind::Requested,
        Some(selection.approvals_reviewer),
        None,
        None,
    );
    let hook_request = session.permission_request_hook(
        audit.request.turn_id.clone(),
        call.name.clone(),
        call.id.clone(),
        serde_json::json!({
            "arguments": call.arguments,
            "detail": format!("surface=browser origin={origin} class={}", class.as_str()),
        }),
    );
    let permission_hook = session.run_permission_request_hook(hook_request);
    if let Some(hooks::PermissionHookDecision::Deny { message }) = permission_hook.decision.as_ref()
    {
        fire_post_permission_response(
            session,
            &audit.request.session_id,
            audit.request.turn_id.as_deref(),
            &audit.request,
            "deny",
        )
        .await;
        audit.record_review(
            selection.approvals_reviewer,
            "hook_denied",
            false,
            started.elapsed().as_millis() as u64,
        );
        return Some(PermissionPreflight::Denied(format!(
            "Browser action denied by hook: {message}"
        )));
    }
    if permission_hook.decision == Some(hooks::PermissionHookDecision::Allow) {
        fire_post_permission_response(
            session,
            &audit.request.session_id,
            audit.request.turn_id.as_deref(),
            &audit.request,
            "allow",
        )
        .await;
        audit.record_review(
            selection.approvals_reviewer,
            "hook_allowed",
            true,
            started.elapsed().as_millis() as u64,
        );
        return Some(PermissionPreflight::Granted(Box::new(audit)));
    }
    if class == tools::browser::BrowserApprovalClass::StateChanging
        && selection.approvals_reviewer == types::ApprovalsReviewer::AutoReview
    {
        let targets = session
            .auxiliary_targets(types::AuxiliaryTask::SmartApproval)
            .iter()
            .map(crate::control::smart_approval::ApprovalTarget::from)
            .collect::<Vec<_>>();
        let smart_ctx = build_smart_approval_context(session).await;
        if crate::control::smart_approval::maybe_smart_downgrade_ask(
            &audit.request,
            &targets,
            smart_ctx.as_ref(),
        )
        .await
            == types::ApprovalAction::Auto
        {
            fire_post_permission_response(
                session,
                &audit.request.session_id,
                audit.request.turn_id.as_deref(),
                &audit.request,
                "auto",
            )
            .await;
            audit.record_review(
                selection.approvals_reviewer,
                "auto_approved",
                true,
                started.elapsed().as_millis() as u64,
            );
            return Some(PermissionPreflight::Granted(Box::new(audit)));
        }
    }

    let Some(gate) = hitl_gate else {
        fire_post_permission_response(
            session,
            &audit.request.session_id,
            audit.request.turn_id.as_deref(),
            &audit.request,
            "unavailable",
        )
        .await;
        audit.record_review(
            selection.approvals_reviewer,
            "reviewer_unavailable",
            false,
            started.elapsed().as_millis() as u64,
        );
        return Some(PermissionPreflight::Denied(
            "Browser action blocked: user approval is unavailable".to_string(),
        ));
    };
    let title = if class == tools::browser::BrowserApprovalClass::Sensitive {
        "批准敏感网页操作"
    } else {
        "批准网页操作"
    };
    let body = format!(
        "Agent 请求在 `{origin}` 执行 `{}`。\n\n目标：`{target}`\n风险级别：`{}`\n\n敏感操作不会提供永久放行。",
        call.name,
        class.as_str()
    );
    let allow_always = class == tools::browser::BrowserApprovalClass::StateChanging;
    let Some(confirm) = park_confirm(
        gate,
        session.as_ref(),
        turn_context,
        &call.id,
        ConfirmPresentation::Text { title, body: &body },
        allow_always,
        None,
    )
    .await
    else {
        fire_post_permission_response(
            session,
            &audit.request.session_id,
            audit.request.turn_id.as_deref(),
            &audit.request,
            "cancelled",
        )
        .await;
        audit.record_review(
            selection.approvals_reviewer,
            "cancelled",
            false,
            started.elapsed().as_millis() as u64,
        );
        return None;
    };
    let choice = if !confirm.approved {
        "deny"
    } else if confirm.always && allow_always {
        "allow_always"
    } else {
        "allow_once"
    };
    fire_post_permission_response(
        session,
        &audit.request.session_id,
        audit.request.turn_id.as_deref(),
        &audit.request,
        choice,
    )
    .await;
    audit.record_review(
        selection.approvals_reviewer,
        choice,
        confirm.approved,
        started.elapsed().as_millis() as u64,
    );
    if confirm.status == "timeout" {
        return Some(PermissionPreflight::Denied(
            "Tool error: browser approval request timed out".to_string(),
        ));
    }
    if !confirm.approved {
        return Some(PermissionPreflight::Denied(
            "Browser action denied by user".to_string(),
        ));
    }
    if confirm.always && allow_always {
        if let Err(error) = tools::browser::add_approval_rule(&memory_dir, &origin, class) {
            tracing::warn!(%error, %origin, "failed to persist browser approval rule");
        }
    }
    Some(PermissionPreflight::Granted(Box::new(audit)))
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
        if !step_context.routes_tool(&call.name) {
            out.push(format!("工具 `{}` 未在本次 StepContext 注册，无法执行。", call.name).into());
            continue;
        }

        let mut workspace_write_grant = false;
        let mut permission_audits = Vec::new();
        if !call.args_parse_error {
            match preflight_browser_action(session, call, turn_context, hitl_gate).await? {
                PermissionPreflight::NotRequired => {}
                PermissionPreflight::Granted(audit) => permission_audits.push(*audit),
                PermissionPreflight::Denied(message) => {
                    out.push(format!("{message}. Do not retry the same action without explicit authorization.").into());
                    continue;
                }
            }
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
        }

        // 危险 terminal：deny / auto / ask
        if call.name == "exec_command" && !call.args_parse_error {
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
                            type_allowlist,
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
                                permissions.command_type_allowlist.clone(),
                                base,
                                active_profile_id,
                                permissions,
                            )
                        };

                        let mut route = approval_route(
                            &cmd,
                            decision.description,
                            &permissions,
                            &allowlist,
                            &type_allowlist,
                        );
                        if matches!(route, ApprovalRoute::Smart | ApprovalRoute::Manual) {
                            let cache_key = crate::control::approval_cache::ApprovalCacheKey::new(
                                &call.name, &cmd,
                            );
                            let (_, _, approval_cache) = session.ensure_thread_controls();
                            if approval_cache.lookup(&cache_key).await.is_some() {
                                tracing::debug!(
                                    command = %cmd,
                                    "approval cache hit — skipping user prompt"
                                );
                                route = ApprovalRoute::Allowlist;
                            }
                        }
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
                                ApprovalRoute::TypeAllowlist => "command_type_allowlist",
                                ApprovalRoute::Off => "approval_disabled",
                                ApprovalRoute::Smart => "auto_review_required",
                                ApprovalRoute::Manual => "user_review_required",
                            }),
                            None,
                        );
                        let hook_request = session.permission_request_hook(
                            approval_turn_id.clone(),
                            "Bash",
                            call.id.clone(),
                            serde_json::json!({
                                "command": cmd,
                                "detail": format!("surface=terminal ask={}", decision.description),
                            }),
                        );
                        let permission_hook = session.run_permission_request_hook(hook_request);
                        if let Some(hooks::PermissionHookDecision::Deny { message }) =
                            &permission_hook.decision
                        {
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
                                format!("Command denied by PermissionRequest hook: {message}")
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
                        } else if permission_hook.decision
                            == Some(hooks::PermissionHookDecision::Allow)
                        {
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
                        } else if matches!(
                            route,
                            ApprovalRoute::Allowlist | ApprovalRoute::TypeAllowlist
                        ) {
                            let rule_source = if route == ApprovalRoute::TypeAllowlist {
                                "command_type_allowlist"
                            } else {
                                "allowlist"
                            };
                            fire_post_approval_response(
                                session,
                                &approval_session_id,
                                approval_turn_id.as_deref(),
                                &cmd,
                                rule_source,
                            )
                            .await;
                            approval_audit.record(
                                memory::PermissionAuditKind::Granted,
                                None,
                                Some(rule_source),
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
                                let canonical_action =
                                    crate::control::guardian::GuardianRetryState::canonical_action(
                                        &call.name,
                                        &call.arguments,
                                    );
                                if let Some(assessment_id) =
                                    session.guardian_retry.consume_retry(&canonical_action)
                                {
                                    let now = chrono::Utc::now().timestamp_millis();
                                    session
                                        .send_event(
                                            turn_context.sub_id(),
                                            agent_protocol::EventMsg::GuardianAssessment(
                                                agent_protocol::GuardianAssessmentEvent {
                                                    id: assessment_id,
                                                    target_item_id: call.id.clone(),
                                                    turn_id: turn_context.sub_id().to_string(),
                                                    status: agent_protocol::GuardianAssessmentStatus::Approved,
                                                    canonical_action,
                                                    risk: None,
                                                    rationale: Some(
                                                        "user authorized one exact retry".into(),
                                                    ),
                                                    decision_source: Some("user_retry".into()),
                                                    started_at_ms: now,
                                                    completed_at_ms: Some(now),
                                                },
                                            ),
                                        )
                                        .await;
                                    types::ApprovalAction::Auto
                                } else {
                                    let assessment_id = uuid::Uuid::new_v4().to_string();
                                    let started_at_ms = chrono::Utc::now().timestamp_millis();
                                    session
                                        .send_event(
                                            turn_context.sub_id(),
                                            agent_protocol::EventMsg::GuardianAssessment(
                                                agent_protocol::GuardianAssessmentEvent {
                                                    id: assessment_id.clone(),
                                                    target_item_id: call.id.clone(),
                                                    turn_id: turn_context.sub_id().to_string(),
                                                    status: agent_protocol::GuardianAssessmentStatus::InProgress,
                                                    canonical_action: canonical_action.clone(),
                                                    risk: None,
                                                    rationale: None,
                                                    decision_source: Some("guardian".into()),
                                                    started_at_ms,
                                                    completed_at_ms: None,
                                                },
                                            ),
                                        )
                                        .await;
                                    let agent = session.as_ref();
                                    let targets: Vec<_> = agent
                                        .auxiliary_targets(types::AuxiliaryTask::SmartApproval)
                                        .iter()
                                        .map(crate::control::smart_approval::ApprovalTarget::from)
                                        .collect();
                                    let smart_ctx = build_smart_approval_context(session).await;
                                    let verdict = crate::control::smart_approval::assess_guardian(
                                        &permission_request,
                                        &targets,
                                        smart_ctx.as_ref(),
                                    )
                                    .await;
                                    let (status, action, risk, rationale) = match verdict {
                                        Ok(verdict) => match verdict.decision {
                                            crate::control::smart_approval::GuardianDecision::ApproveOnce => (
                                                agent_protocol::GuardianAssessmentStatus::Approved,
                                                types::ApprovalAction::Auto,
                                                verdict.risk,
                                                verdict.reason,
                                            ),
                                            crate::control::smart_approval::GuardianDecision::Deny => (
                                                agent_protocol::GuardianAssessmentStatus::Denied,
                                                types::ApprovalAction::Deny,
                                                verdict.risk,
                                                verdict.reason,
                                            ),
                                            crate::control::smart_approval::GuardianDecision::Indeterminate => (
                                                agent_protocol::GuardianAssessmentStatus::Aborted,
                                                types::ApprovalAction::Ask,
                                                verdict.risk,
                                                verdict.reason,
                                            ),
                                        },
                                        Err(error) => (
                                            agent_protocol::GuardianAssessmentStatus::Aborted,
                                            types::ApprovalAction::Ask,
                                            None,
                                            Some(error),
                                        ),
                                    };
                                    session
                                        .send_event(
                                            turn_context.sub_id(),
                                            agent_protocol::EventMsg::GuardianAssessment(
                                                agent_protocol::GuardianAssessmentEvent {
                                                    id: assessment_id.clone(),
                                                    target_item_id: call.id.clone(),
                                                    turn_id: turn_context.sub_id().to_string(),
                                                    status,
                                                    canonical_action: canonical_action.clone(),
                                                    risk,
                                                    rationale,
                                                    decision_source: Some("guardian".into()),
                                                    started_at_ms,
                                                    completed_at_ms: Some(
                                                        chrono::Utc::now().timestamp_millis(),
                                                    ),
                                                },
                                            ),
                                        )
                                        .await;
                                    if action == types::ApprovalAction::Deny {
                                        session
                                            .guardian_retry
                                            .record_denied(assessment_id.clone(), canonical_action);
                                        out.push(format!(
                                            "Command denied by Guardian assessment {assessment_id}. The user may authorize one exact retry."
                                        ).into());
                                        continue;
                                    }
                                    action
                                }
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
                            } else if let Some(gate) = hitl_gate {
                                // Smart 模式下辅模型未放行时回退到用户手动审批，
                                // 而非直接拒绝——「不确定就问用户」比「不确定就拒绝」更合理。
                                let title = "批准危险命令";
                                let body = format!(
                                    "检测到潜在危险操作（{}）：\n\n```\n{cmd}\n```",
                                    decision.description
                                );
                                let command_type_rule =
                                    tools::command_type_rule_candidate(&cmd, decision.description);
                                let confirm = park_confirm(
                                    gate,
                                    session.as_ref(),
                                    turn_context,
                                    &call.id,
                                    ConfirmPresentation::Text { title, body: &body },
                                    true,
                                    command_type_rule
                                        .as_ref()
                                        .map(|rule| rule.command_family.as_str()),
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
                                    out.push(if confirm.status == "timeout" {
                                        "Tool error: dangerous-command approval request timed out"
                                            .into()
                                    } else {
                                        "Command denied by user (dangerous-command approval). Do not retry the same command without explicit user request.".into()
                                    });
                                    continue;
                                }
                                // 永久许可按用户选择写入精确命令或受限的命令类型规则。
                                if confirm.command_type {
                                    if let Some(rule) = command_type_rule.as_ref() {
                                        if let Err(e) =
                                            memory::config::add_command_type_to_allowlist(
                                                &memory_dir,
                                                rule,
                                            )
                                        {
                                            tracing::warn!(error = %e, "failed to persist command type allowlist");
                                        } else {
                                            tracing::info!(
                                                command_family = %rule.command_family,
                                                risk = %rule.risk,
                                                "added command type to approval allowlist"
                                            );
                                        }
                                    }
                                } else if confirm.always {
                                    if let Err(e) =
                                        memory::config::add_command_to_allowlist(&memory_dir, &cmd)
                                    {
                                        tracing::warn!(error = %e, "failed to persist command allowlist");
                                    } else {
                                        tracing::info!(command = %cmd, "added command to approval allowlist");
                                    }
                                }
                                {
                                    let cache_key =
                                        crate::control::approval_cache::ApprovalCacheKey::new(
                                            &call.name, &cmd,
                                        );
                                    let (_, _, approval_cache) = session.ensure_thread_controls();
                                    approval_cache.insert(cache_key).await;
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
            let executed = if matches!(call.name.as_str(), "exec" | "wait") {
                execute_code_mode_tool(
                    session,
                    Arc::clone(&step_context),
                    call,
                    pause,
                    turn_context,
                    hitl_gate,
                )
                .await
            } else {
                session.handle_tool_invocation_with_once_grants(
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
                        managed_network: managed_network.clone(),
                    },
                )
            };
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
                                        // 文件系统提权不应顺带收紧网络。
                                        true,
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
            "exec_command",
            &term("mkfs.ext4 /dev/sdb1")
        ));
        assert!(tool_may_require_permission(
            "exec_command",
            &term("dd if=/dev/zero of=/dev/sda")
        ));
        // Ask 也强制串行（需 HITL 卡）
        assert!(tool_may_require_permission(
            "exec_command",
            &term("rm -rf /tmp/project")
        ));
    }

    #[test]
    fn process_and_mutating_file_tools_force_serial_preflight() {
        assert!(tool_may_require_permission(
            "exec_command",
            &term("rm -rf node_modules")
        ));
        assert!(tool_may_require_permission("exec_command", &term("ls -la")));
        assert!(tool_may_require_permission(
            "code_exec",
            &serde_json::json!({})
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
        // 网络默认放开，网页工具不再占用串行审批槽位。
        assert!(!tool_may_require_permission(
            "web_search",
            &serde_json::json!({"query": "rust"})
        ));
        assert!(!tool_may_require_permission(
            "web_fetch",
            &serde_json::json!({"url": "https://example.com"})
        ));
    }

    #[tokio::test]
    async fn turn_network_override_disables_subprocess_network() {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            AgentLoop::new(crate::runtime::Config::with_defaults(
                dir.path().to_path_buf(),
            ))
            .await
            .unwrap(),
        );
        let turn = Arc::new(
            TurnContext::new(
                "review-network-off".into(),
                1,
                types::InteractionMode::Agent,
                Some(types::READ_ONLY_PROFILE.into()),
                Some(dir.path().to_path_buf()),
            )
            .with_network_access(false),
        );
        session.bind_turn_context(turn).await;
        let step = session.capture_step_context().await.unwrap();
        let call = types::ParsedToolCall::with_id(
            "call-review-network",
            "exec_command",
            json!({"command": "curl https://example.com"}),
        );

        let policy = sandbox_policy_for_call(session.as_ref(), &step, &call, false, None)
            .unwrap()
            .expect("exec_command must run in the subprocess sandbox");

        assert!(!policy.network_access);
        assert!(policy.managed_network.is_none());
    }

    #[test]
    fn permission_request_paths_include_logical_path() {
        assert_eq!(
            affected_write_paths("todo", &serde_json::json!({})),
            vec!["workspace/plans"]
        );
    }

    #[test]
    fn approval_modes_and_allowlist_route_correctly() {
        let ask = "rm -rf /tmp/project";
        let none: Vec<String> = Vec::new();
        let no_types: Vec<memory::CommandTypeRule> = Vec::new();
        assert_eq!(
            approval_route(
                ask,
                "high",
                &types::SessionPermissions::approve_for_me(),
                &none,
                &no_types,
            ),
            ApprovalRoute::Smart
        );
        assert_eq!(
            approval_route(
                ask,
                "high",
                &types::SessionPermissions::ask_for_approval(),
                &none,
                &no_types,
            ),
            ApprovalRoute::Manual
        );
        assert_eq!(
            approval_route(
                ask,
                "high",
                &types::SessionPermissions::full_access(),
                &none,
                &no_types,
            ),
            ApprovalRoute::Off
        );

        let allowlist = vec![ask.to_string()];
        assert_eq!(
            approval_route(
                ask,
                "high",
                &types::SessionPermissions::ask_for_approval(),
                &allowlist,
                &no_types,
            ),
            ApprovalRoute::Allowlist
        );

        let type_allowlist = vec![memory::CommandTypeRule {
            command_family: "curl".to_string(),
            risk: "dynamic shell expansion".to_string(),
        }];
        assert_eq!(
            approval_route(
                "curl https://example.com",
                "dynamic shell expansion",
                &types::SessionPermissions::ask_for_approval(),
                &none,
                &type_allowlist,
            ),
            ApprovalRoute::TypeAllowlist
        );
    }

    #[test]
    fn hardline_wins_over_off_and_allowlist() {
        let command = "mkfs.ext4 /dev/sdb1";
        let allowlist = vec!["mkfs*".to_string()];
        assert_eq!(
            approval_route(
                command,
                "critical",
                &types::SessionPermissions::full_access(),
                &allowlist,
                &[],
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
                tool_name: "exec_command".into(),
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
            .await
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
            ConfirmPresentation::Text {
                title: "title",
                body: "body",
            },
        )
        .await;
        assert!(matches!(allowed, Some(PermissionPreflight::Granted(_))));

        let deny_dir = tempfile::tempdir().unwrap();
        let deny_session = Arc::new(
            AgentLoop::new(crate::runtime::Config::with_defaults(
                deny_dir.path().to_path_buf(),
            ))
            .await
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
            ConfirmPresentation::Text {
                title: "title",
                body: "body",
            },
        )
        .await;
        assert!(matches!(
            denied,
            Some(PermissionPreflight::Denied(message)) if message.contains("organization policy")
        ));
    }
}
