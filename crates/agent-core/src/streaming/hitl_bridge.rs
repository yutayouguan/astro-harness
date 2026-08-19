//! Astro HITL 桥：解析工具结果中的 `astro_hitl` 标记，并 park/resume 当前会话。

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::control::hitl::{HitlGate, HitlResolution, HITL_DEFAULT_TIMEOUT_SECS};
use crate::control::interrupt::Interrupt;

use super::lifecycle::emit;
use super::types::MultiTurnStreamItem;

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
/// （`resolved` / `cancelled` / `timeout`），供 `post_approval_response` 钩子填充 `choice`。
pub(crate) struct ConfirmOutcome {
    pub approved: bool,
    pub status: String,
    /// 用户点了「批准并永久放行」时为 `true`（`allow_always` 场景）。
    pub always: bool,
}

/// 弹出 confirm 型 HITL surface，等待用户批准/拒绝。
///
/// `allow_always` 为 true 时额外提供「批准并永久放行」按钮，其结果 `ConfirmOutcome::always`。
pub(crate) async fn park_confirm(
    gate: &Arc<HitlGate>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    tool_call_id: &str,
    title: &str,
    body: &str,
    allow_always: bool,
) -> Option<ConfirmOutcome> {
    let surface_id = format!("confirm-{}", uuid::Uuid::new_v4());
    let operations =
        a2ui::templates::build_confirm_surface_ex(&surface_id, title, body, allow_always);
    let ops_value = serde_json::Value::Array(operations);
    let resolution = park_astro_hitl_resolution(
        gate,
        tx,
        run_id,
        tool_call_id,
        AstroHitlPayload {
            reason: "confirmation".into(),
            message: title.into(),
            operations: ops_value,
            response_schema: serde_json::json!({
                "type": "object",
                "properties": { "approved": { "type": "boolean" } },
                "required": ["approved"]
            }),
        },
    )
    .await?;
    let (approved, always) = if resolution.status == "resolved" {
        let v = serde_json::from_str::<serde_json::Value>(&resolution.payload_json).ok();
        let approved = v
            .as_ref()
            .and_then(|v| v.get("approved").and_then(|x| x.as_bool()))
            .unwrap_or(false);
        let always = v
            .as_ref()
            .and_then(|v| v.get("always").and_then(|x| x.as_bool()))
            .unwrap_or(false);
        (approved, always)
    } else {
        (false, false)
    };
    Some(ConfirmOutcome {
        approved,
        status: resolution.status,
        always: always && approved,
    })
}

/// 将 HITL payload 以 Activity + RunFinished(hitl_waiting) 形式推给 UI，并阻塞等待 resume。
///
/// `outcome_type` 由 [`super::run_state::RunState`] 派生，与 Agno requirements 语义对齐。
pub(crate) async fn park_astro_hitl(
    gate: &Arc<HitlGate>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    tool_call_id: &str,
    hitl: AstroHitlPayload,
) -> Option<String> {
    park_astro_hitl_resolution(gate, tx, run_id, tool_call_id, hitl)
        .await
        .map(|r| r.to_tool_result())
}

/// [`park_astro_hitl`] 的内核：返回原始 [`HitlResolution`]（含 `status`），供
/// `park_confirm` 区分 `resolved`/`cancelled`/`timeout`；`park_astro_hitl` 仍对外只
/// 暴露转换后的 tool-result 字符串。
async fn park_astro_hitl_resolution(
    gate: &Arc<HitlGate>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    tool_call_id: &str,
    hitl: AstroHitlPayload,
) -> Option<HitlResolution> {
    let message_id = format!("a2ui-surface-{tool_call_id}");
    let content_json = serde_json::json!({ "operations": hitl.operations }).to_string();
    if !emit(
        tx,
        MultiTurnStreamItem::Activity {
            message_id,
            activity_type: "a2ui-surface".into(),
            content_json,
            replace: true,
        },
    )
    .await
    {
        return None;
    }

    let interrupt = Interrupt {
        id: uuid::Uuid::new_v4().to_string(),
        reason: hitl.reason.clone(),
        message: hitl.message.clone(),
        tool_call_id: tool_call_id.to_string(),
        response_schema_json: hitl.response_schema.to_string(),
        expires_at: String::new(),
        metadata_json: String::new(),
    };
    let interrupts_json = serde_json::to_string(&vec![&interrupt]).unwrap_or_else(|_| "[]".into());
    let mut hitl_state = super::run_state::RunState::new();
    hitl_state.await_hitl(super::run_state::RunRequirements::for_hitl_reason(
        &hitl.reason,
        interrupt.id.clone(),
    ));
    let rx = gate.begin_wait(interrupt.clone()).await;
    if !emit(
        tx,
        MultiTurnStreamItem::RunFinished {
            run_id: run_id.to_string(),
            outcome_type: hitl_state.outcome_type().into(),
            interrupts_json,
        },
    )
    .await
    {
        gate.abort_wait(&interrupt.id).await;
        return None;
    }

    let resolution = gate
        .finish_wait(
            &interrupt.id,
            rx,
            Duration::from_secs(HITL_DEFAULT_TIMEOUT_SECS),
        )
        .await;
    Some(resolution)
}
