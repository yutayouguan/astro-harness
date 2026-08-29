//! Astro HITL 桥：解析工具结果中的 `astro_hitl` 标记，并 park/resume 当前会话。

use std::sync::Arc;
use std::time::Duration;

use agent_protocol::{ControlRequestEvent, EventMsg};

use crate::control::hitl::{HitlGate, HitlResolution, HITL_DEFAULT_TIMEOUT_SECS};
use crate::control::interrupt::Interrupt;
use crate::runtime::{Session, TurnContext};

use super::lifecycle::emit;

pub(crate) struct AstroHitlPayload {
    pub reason: String,
    pub message: String,
    pub operations: serde_json::Value,
    pub response_schema: serde_json::Value,
}

pub(crate) fn parse_astro_hitl(result: &str) -> Option<AstroHitlPayload> {
    let value: serde_json::Value = serde_json::from_str(result).ok()?;
    if value.get("astro_hitl")?.as_bool() != Some(true) {
        return None;
    }
    let operations = value.get("operations")?.clone();
    if !operations.is_array() {
        return None;
    }
    Some(AstroHitlPayload {
        reason: value
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("confirmation")
            .to_string(),
        message: value
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        operations,
        response_schema: value
            .get("response_schema")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    })
}

/// `park_confirm` 决议：`approved` 供调用方分支；`status` 对应 [`HitlResolution::status`]
/// （`resolved` / `cancelled` / `timeout`），供 `PostApprovalResponse` 钩子填充 `choice`。
pub(crate) struct ConfirmOutcome {
    pub approved: bool,
    pub status: String,
    /// 用户点了「批准并永久放行」时为 `true`（`allow_always` 场景）。
    pub always: bool,
    /// 用户选择了低风险“同类命令”永久规则。
    pub command_type: bool,
}

pub(crate) enum ConfirmPresentation<'a> {
    Text { title: &'a str, body: &'a str },
    SandboxRetry { denial_detail: &'a str },
}

/// 弹出 confirm 型 HITL surface，等待用户批准/拒绝。
///
/// `allow_always` 为 true 时额外提供「批准并永久放行」按钮，其结果 `ConfirmOutcome::always`。
pub(crate) async fn park_confirm(
    gate: &Arc<HitlGate>,
    session: &Session,
    turn_context: &TurnContext,
    tool_call_id: &str,
    presentation: ConfirmPresentation<'_>,
    allow_always: bool,
    command_family: Option<&str>,
) -> Option<ConfirmOutcome> {
    let surface_id = format!("confirm-{}", uuid::Uuid::new_v4());
    let (message, operations) = match presentation {
        ConfirmPresentation::Text { title, body } => (
            title,
            a2ui::templates::build_confirm_surface_with_rule(
                &surface_id,
                title,
                body,
                allow_always,
                command_family,
            ),
        ),
        ConfirmPresentation::SandboxRetry { denial_detail } => (
            "sandbox_retry",
            a2ui::templates::build_sandbox_retry_surface(&surface_id, denial_detail),
        ),
    };
    let ops_value = serde_json::Value::Array(operations);
    let resolution = park_astro_hitl_resolution(
        gate,
        session,
        turn_context,
        tool_call_id,
        AstroHitlPayload {
            reason: "confirmation".into(),
            message: message.into(),
            operations: ops_value,
            response_schema: serde_json::json!({
                "type": "object",
                "properties": { "approved": { "type": "boolean" } },
                "required": ["approved"]
            }),
        },
    )
    .await?;
    let (approved, always, command_type) = if resolution.status == "resolved" {
        let v = serde_json::from_str::<serde_json::Value>(&resolution.payload_json).ok();
        let approved = v
            .as_ref()
            .and_then(|v| v.get("approved").and_then(|x| x.as_bool()))
            .unwrap_or(false);
        let always = v
            .as_ref()
            .and_then(|v| v.get("always").and_then(|x| x.as_bool()))
            .unwrap_or(false);
        let command_type = v
            .as_ref()
            .and_then(|v| v.get("scope").and_then(|x| x.as_str()))
            == Some("type");
        (approved, always, command_type)
    } else {
        (false, false, false)
    };
    Some(ConfirmOutcome {
        approved,
        status: resolution.status,
        always: always && approved,
        command_type: command_type && approved,
    })
}

/// 将 HITL payload 以 Activity + RunFinished(hitl_waiting) 形式推给 UI，并阻塞等待 resume。
///
/// `outcome_type` 由 [`super::run_state::RunState`] 派生，与 Agno requirements 语义对齐。
pub(crate) async fn park_astro_hitl(
    gate: &Arc<HitlGate>,
    session: &Session,
    turn_context: &TurnContext,
    tool_call_id: &str,
    hitl: AstroHitlPayload,
) -> Option<String> {
    park_astro_hitl_resolution(gate, session, turn_context, tool_call_id, hitl)
        .await
        .map(|r| r.to_tool_result())
}

/// [`park_astro_hitl`] 的内核：返回原始 [`HitlResolution`]（含 `status`），供
/// `park_confirm` 区分 `resolved`/`cancelled`/`timeout`；`park_astro_hitl` 仍对外只
/// 暴露转换后的 tool-result 字符串。
async fn park_astro_hitl_resolution(
    gate: &Arc<HitlGate>,
    session: &Session,
    turn_context: &TurnContext,
    tool_call_id: &str,
    hitl: AstroHitlPayload,
) -> Option<HitlResolution> {
    let interrupt = Interrupt {
        id: uuid::Uuid::new_v4().to_string(),
        reason: hitl.reason.clone(),
        message: hitl.message.clone(),
        tool_call_id: tool_call_id.to_string(),
        response_schema_json: hitl.response_schema.to_string(),
        expires_at: String::new(),
        metadata_json: String::new(),
    };
    let rx = gate.begin_wait(interrupt.clone()).await;
    emit(
        session,
        turn_context,
        EventMsg::RequestUserInput(ControlRequestEvent {
            turn_id: turn_context.sub_id().to_string(),
            item_id: tool_call_id.to_string(),
            request_id: interrupt.id.clone(),
            payload: serde_json::json!({
                "reason": hitl.reason,
                "message": hitl.message,
                "operations": hitl.operations,
                "response_schema": hitl.response_schema,
            }),
        }),
    )
    .await;

    let resolution = gate
        .finish_wait(
            &interrupt.id,
            rx,
            Duration::from_secs(HITL_DEFAULT_TIMEOUT_SECS),
        )
        .await;
    Some(resolution)
}

/// 网络主机审批请求参数。
#[allow(dead_code)] // 在 inline-managed-network-approval 的 Task 5 中接入
pub(crate) struct NetworkApprovalRequest {
    pub host: String,
    pub protocol: String,
    pub port: u16,
    pub profile_id: String,
    pub command_preview: Option<String>,
}

/// 网络审批 HITL 交互的结果。
#[allow(dead_code)] // 在 inline-managed-network-approval 的 Task 5 中接入
pub(crate) struct NetworkApprovalOutcome {
    pub decision: crate::control::network_approval::PendingApprovalDecision,
    pub status: String,
}

/// 弹出网络主机审批界面，等待用户决定。
///
/// 返回带作用域的决定（once/session/persistent/deny），若事件通道关闭则返回 `None`。
#[allow(dead_code)] // 在 inline-managed-network-approval 的 Task 5 中接入
pub(crate) async fn park_network_approval(
    gate: &Arc<HitlGate>,
    session: &Session,
    turn_context: &TurnContext,
    tool_call_id: &str,
    request: NetworkApprovalRequest,
) -> Option<NetworkApprovalOutcome> {
    use crate::control::network_approval::{ApprovalScope, PendingApprovalDecision};

    let surface_id = format!("net-approval-{}", uuid::Uuid::new_v4());
    let operations = a2ui::templates::build_network_approval_surface(
        &surface_id,
        &request.host,
        &request.protocol,
        request.port,
        &request.profile_id,
        request.command_preview.as_deref(),
    );
    let ops_value = serde_json::Value::Array(operations);
    let resolution = park_astro_hitl_resolution(
        gate,
        session,
        turn_context,
        tool_call_id,
        AstroHitlPayload {
            reason: "network_approval".into(),
            message: format!("Network access: {}", request.host),
            operations: ops_value,
            response_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "scope": {
                        "type": "string",
                        "enum": ["allow_once", "allow_session", "allow_always", "deny"]
                    }
                },
                "required": ["scope"]
            }),
        },
    )
    .await?;

    let decision = if resolution.status == "resolved" {
        let v = serde_json::from_str::<serde_json::Value>(&resolution.payload_json).ok();
        match v
            .as_ref()
            .and_then(|v| v.get("scope").and_then(|s| s.as_str()))
        {
            Some("allow_once") => PendingApprovalDecision::Allow(ApprovalScope::Once),
            Some("allow_session") => PendingApprovalDecision::Allow(ApprovalScope::Session),
            Some("allow_always") => PendingApprovalDecision::Allow(ApprovalScope::Persistent),
            _ => PendingApprovalDecision::Deny,
        }
    } else {
        PendingApprovalDecision::Deny
    };

    Some(NetworkApprovalOutcome {
        decision,
        status: resolution.status,
    })
}

#[cfg(test)]
mod event_tests {
    use super::*;
    use crate::control::interrupt::ResumeItem;
    use crate::runtime::{Config, Session};

    #[tokio::test]
    async fn hitl_request_event_uses_stable_registered_request_id() {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "hitl-event-test".into(),
            )
            .unwrap(),
        );
        let turn_context = session.create_turn_context("turn-1".into()).await;
        let gate = HitlGate::new("hitl-event-test");
        let events = session.subscribe_turn_events("turn-1").await;

        let run = tokio::spawn({
            let gate = Arc::clone(&gate);
            let session = Arc::clone(&session);
            let turn_context = Arc::clone(&turn_context);
            async move {
                park_astro_hitl(
                    &gate,
                    &session,
                    &turn_context,
                    "call-1",
                    AstroHitlPayload {
                        reason: "confirmation".into(),
                        message: "approve".into(),
                        operations: serde_json::json!([]),
                        response_schema: serde_json::json!({
                            "type": "object",
                            "properties": { "approved": { "type": "boolean" } },
                            "required": ["approved"]
                        }),
                    },
                )
                .await
            }
        });

        let event = events.recv().await.unwrap();
        let EventMsg::RequestUserInput(request) = event.msg else {
            panic!("expected request_user_input event");
        };
        assert_eq!(request.item_id, "call-1");
        assert!(!request.request_id.is_empty());
        let pending = gate.pending_interrupts().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, request.request_id);
        gate.resolve(&[ResumeItem {
            interrupt_id: request.request_id,
            status: "resolved".into(),
            payload_json: serde_json::json!({ "approved": true }).to_string(),
        }])
        .await
        .unwrap();
        assert!(run.await.unwrap().is_some());
    }

    async fn run_network_approval(
        scope: &str,
    ) -> crate::control::network_approval::PendingApprovalDecision {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                format!("net-approval-{scope}"),
            )
            .unwrap(),
        );
        let turn_context = session.create_turn_context("turn-1".into()).await;
        let gate = HitlGate::new(format!("net-approval-{scope}"));
        let events = session.subscribe_turn_events("turn-1").await;

        let scope_owned = scope.to_string();
        let run = tokio::spawn({
            let gate = Arc::clone(&gate);
            let session = Arc::clone(&session);
            let turn_context = Arc::clone(&turn_context);
            async move {
                park_network_approval(
                    &gate,
                    &session,
                    &turn_context,
                    "call-net",
                    NetworkApprovalRequest {
                        host: "api.example.com".into(),
                        protocol: "https".into(),
                        port: 443,
                        profile_id: "custom".into(),
                        command_preview: Some("curl https://api.example.com".into()),
                    },
                )
                .await
            }
        });

        let event = events.recv().await.unwrap();
        let EventMsg::RequestUserInput(request) = event.msg else {
            panic!("expected request_user_input event");
        };
        gate.resolve(&[ResumeItem {
            interrupt_id: request.request_id,
            status: "resolved".into(),
            payload_json: serde_json::json!({ "scope": scope_owned }).to_string(),
        }])
        .await
        .unwrap();
        let outcome = run.await.unwrap().unwrap();
        assert_eq!(outcome.status, "resolved");
        outcome.decision
    }

    #[tokio::test]
    async fn network_approval_allow_once_maps_to_once_scope() {
        use crate::control::network_approval::{ApprovalScope, PendingApprovalDecision};
        assert_eq!(
            run_network_approval("allow_once").await,
            PendingApprovalDecision::Allow(ApprovalScope::Once)
        );
    }

    #[tokio::test]
    async fn network_approval_allow_session_maps_to_session_scope() {
        use crate::control::network_approval::{ApprovalScope, PendingApprovalDecision};
        assert_eq!(
            run_network_approval("allow_session").await,
            PendingApprovalDecision::Allow(ApprovalScope::Session)
        );
    }

    #[tokio::test]
    async fn network_approval_allow_always_maps_to_persistent_scope() {
        use crate::control::network_approval::{ApprovalScope, PendingApprovalDecision};
        assert_eq!(
            run_network_approval("allow_always").await,
            PendingApprovalDecision::Allow(ApprovalScope::Persistent)
        );
    }

    #[tokio::test]
    async fn network_approval_deny_maps_to_deny() {
        use crate::control::network_approval::PendingApprovalDecision;
        assert_eq!(
            run_network_approval("deny").await,
            PendingApprovalDecision::Deny
        );
    }

    #[tokio::test]
    async fn network_approval_timeout_maps_to_deny() {
        use crate::control::network_approval::PendingApprovalDecision;
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "net-timeout".into(),
            )
            .unwrap(),
        );
        let turn_context = session.create_turn_context("turn-1".into()).await;
        let gate = HitlGate::new("net-timeout");

        let run = tokio::spawn({
            let gate = Arc::clone(&gate);
            let session = Arc::clone(&session);
            let turn_context = Arc::clone(&turn_context);
            async move {
                park_network_approval(
                    &gate,
                    &session,
                    &turn_context,
                    "call-net",
                    NetworkApprovalRequest {
                        host: "timeout.example.com".into(),
                        protocol: "https".into(),
                        port: 443,
                        profile_id: "custom".into(),
                        command_preview: None,
                    },
                )
                .await
            }
        });

        // 取消而非解决 — 模拟 gate 取消
        tokio::time::sleep(Duration::from_millis(50)).await;
        gate.cancel_all().await;

        let outcome = run.await.unwrap().unwrap();
        assert_eq!(outcome.status, "cancelled");
        assert_eq!(outcome.decision, PendingApprovalDecision::Deny);
    }
}
