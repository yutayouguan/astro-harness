//! Astro HITL 桥：解析工具结果中的 `astro_hitl` 标记，并在父/子会话间转发 park/resume。
//!
//! - 同步 `delegate` 子路径：通过 [`PARENT_HITL_CTX`] task-local 上浮到父流。
//! - 异步 `delegate_async` 子路径：通过 [`LIVE_PARENT_HITL`] 会话表按 `session_id` 查找。

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::control::hitl::{HitlGate, HitlResolution, HITL_DEFAULT_TIMEOUT_SECS};
use crate::control::interrupt::Interrupt;

use super::multi_turn::emit;
use super::types::MultiTurnStreamItem;

tokio::task_local! {
    /// 同步 `delegate` 子路径上浮 HITL 时读取；由串行工具执行注入。
    pub(crate) static PARENT_HITL_CTX: Option<ParentHitlCtx>;
}

/// 父会话 HITL 桥：子 Agent park 时复用同一 gate 与流。
#[derive(Clone)]
pub(crate) struct ParentHitlCtx {
    pub gate: Arc<HitlGate>,
    pub tx: mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    pub run_id: String,
}

/// 进行中聊天流的父 HITL 表（供 `delegate_async` 子任务上浮）。
static LIVE_PARENT_HITL: OnceLock<tokio::sync::RwLock<HashMap<String, ParentHitlCtx>>> =
    OnceLock::new();

fn live_parent_hitl_map() -> &'static tokio::sync::RwLock<HashMap<String, ParentHitlCtx>> {
    LIVE_PARENT_HITL.get_or_init(|| tokio::sync::RwLock::new(HashMap::new()))
}

pub(crate) async fn register_live_parent_hitl(session_id: &str, ctx: ParentHitlCtx) {
    live_parent_hitl_map()
        .write()
        .await
        .insert(session_id.to_string(), ctx);
}

pub(crate) async fn unregister_live_parent_hitl(session_id: &str) {
    live_parent_hitl_map().write().await.remove(session_id);
}

fn decorate_delegate_hitl(mut hitl: AstroHitlPayload, async_child: bool) -> AstroHitlPayload {
    let tag = if async_child {
        "[delegate_async]"
    } else {
        "[delegate]"
    };
    if !hitl.reason.starts_with("[delegate") {
        hitl.reason = format!("{tag} {}", hitl.reason);
    }
    if hitl.message.is_empty() {
        hitl.message = format!("{tag} Sub-agent needs your input");
    } else if !hitl.message.starts_with("[delegate") {
        hitl.message = format!("{tag} {}", hitl.message);
    }
    hitl
}

/// 子 Agent 若有父 HITL 上下文则 park 并返回 tool result；否则 `None`。
///
/// 查找顺序：task_local（同步 delegate）→ live 会话表（async，父流仍在）。
pub(crate) async fn try_park_parent_hitl(
    tool_call_id: &str,
    hitl: AstroHitlPayload,
    parent_session_id: Option<&str>,
) -> Option<String> {
    if let Some(ctx) = PARENT_HITL_CTX.try_with(|c| c.clone()).ok().flatten() {
        let hitl = decorate_delegate_hitl(hitl, false);
        return park_astro_hitl(&ctx.gate, &ctx.tx, &ctx.run_id, tool_call_id, hitl).await;
    }
    if let Some(sid) = parent_session_id {
        if let Some(ctx) = live_parent_hitl_map().read().await.get(sid).cloned() {
            let hitl = decorate_delegate_hitl(hitl, true);
            return park_astro_hitl(&ctx.gate, &ctx.tx, &ctx.run_id, tool_call_id, hitl).await;
        }
    }
    None
}

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
        return None;
    }

    let rx = gate.begin_wait(interrupt.clone()).await;
    let resolution = gate
        .finish_wait(
            &interrupt.id,
            rx,
            Duration::from_secs(HITL_DEFAULT_TIMEOUT_SECS),
        )
        .await;
    Some(resolution)
}

#[cfg(test)]
mod child_hitl_tests {
    use super::*;
    use crate::control::interrupt::ResumeItem;
    use serde_json::json;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn try_park_parent_hitl_resolves_via_gate() {
        let gate = HitlGate::new("parent-sess");
        let (tx, mut rx) = mpsc::channel::<anyhow::Result<MultiTurnStreamItem>>(8);
        let ctx = ParentHitlCtx {
            gate: gate.clone(),
            tx,
            run_id: "run-1".into(),
        };

        let gate_resolver = gate.clone();
        let resolve_task = tokio::spawn(async move {
            // 等到有 waiting
            for _ in 0..50 {
                if gate_resolver.is_waiting().await {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let pending = gate_resolver.pending_interrupts().await;
            assert!(!pending.is_empty());
            assert!(pending[0].reason.contains("[delegate]"));
            gate_resolver
                .resolve(&[ResumeItem {
                    interrupt_id: pending[0].id.clone(),
                    status: "resolved".into(),
                    payload_json: r#"{"approved":true}"#.into(),
                }])
                .await
                .unwrap();
        });

        let hitl = AstroHitlPayload {
            reason: "confirmation".into(),
            message: "ok?".into(),
            operations: json!([]),
            response_schema: json!({
                "type": "object",
                "properties": { "approved": { "type": "boolean" } },
                "required": ["approved"]
            }),
        };

        let result = PARENT_HITL_CTX
            .scope(Some(ctx), async {
                try_park_parent_hitl("tc-child-1", hitl, None).await
            })
            .await
            .expect("park should return");

        assert!(
            result.contains("approved") || result.contains("true"),
            "got {result}"
        );
        resolve_task.await.unwrap();

        // 至少收到 Activity 或 hitl_waiting
        let mut saw_waiting = false;
        while let Ok(item) = rx.try_recv() {
            if let Ok(MultiTurnStreamItem::RunFinished { outcome_type, .. }) = item {
                if outcome_type == "hitl_waiting" {
                    saw_waiting = true;
                }
            }
        }
        assert!(saw_waiting, "expected hitl_waiting on parent stream");
    }

    #[tokio::test]
    async fn try_park_without_ctx_returns_none() {
        let hitl = AstroHitlPayload {
            reason: "confirmation".into(),
            message: "x".into(),
            operations: json!([]),
            response_schema: json!({}),
        };
        assert!(try_park_parent_hitl("tc", hitl, None).await.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn try_park_via_live_session_map() {
        let gate = HitlGate::new("async-parent");
        let (tx, mut rx) = mpsc::channel::<anyhow::Result<MultiTurnStreamItem>>(8);
        register_live_parent_hitl(
            "async-parent",
            ParentHitlCtx {
                gate: gate.clone(),
                tx,
                run_id: "run-async".into(),
            },
        )
        .await;

        let gate_resolver = gate.clone();
        let resolve_task = tokio::spawn(async move {
            for _ in 0..50 {
                if gate_resolver.is_waiting().await {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let pending = gate_resolver.pending_interrupts().await;
            assert!(!pending.is_empty());
            assert!(pending[0].reason.contains("delegate_async"));
            gate_resolver
                .resolve(&[ResumeItem {
                    interrupt_id: pending[0].id.clone(),
                    status: "resolved".into(),
                    payload_json: r#"{"approved":true}"#.into(),
                }])
                .await
                .unwrap();
        });

        let hitl = AstroHitlPayload {
            reason: "confirmation".into(),
            message: "async?".into(),
            operations: json!([]),
            response_schema: json!({
                "type": "object",
                "properties": { "approved": { "type": "boolean" } },
                "required": ["approved"]
            }),
        };
        let result = try_park_parent_hitl("tc-async", hitl, Some("async-parent"))
            .await
            .expect("live park");
        assert!(
            result.contains("approved") || result.contains("true"),
            "got {result}"
        );
        resolve_task.await.unwrap();
        unregister_live_parent_hitl("async-parent").await;

        let mut saw_waiting = false;
        while let Ok(item) = rx.try_recv() {
            if let Ok(MultiTurnStreamItem::RunFinished { outcome_type, .. }) = item {
                if outcome_type == "hitl_waiting" {
                    saw_waiting = true;
                }
            }
        }
        assert!(saw_waiting);
    }
}
