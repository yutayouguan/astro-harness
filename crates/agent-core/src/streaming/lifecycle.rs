//! 多轮流式循环的生命周期辅助：事件发送、usage 记录、终态收尾。

use std::sync::Arc;

use providers::Usage;
use tokio::sync::mpsc;

use super::provider::ProviderStreamer;
use super::run_state::{RunPhase, RunState};
use super::types::{MultiTurnStreamItem, StreamedAssistantContent};
use crate::runtime::usage::{apply_llm_usage_dual_write, LlmUsageWrite};
use crate::runtime::AgentLoop;

/// 向 mpsc 发送单个成功事件；接收方关闭时返回 `false`。
pub(crate) async fn emit(
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    item: MultiTurnStreamItem,
) -> bool {
    tx.send(Ok(item)).await.is_ok()
}

/// 尽力双写 `kind=llm` 事件与会话账单；失败忽略。
/// `meta` 优先使用本轮实际命中目标；缺省时回退到 AgentLoop 上的会话凭据（不应在 failover 时写入）。
pub(super) async fn record_llm_usage(
    session: &Arc<AgentLoop>,
    streamer: &ProviderStreamer,
    usage: &Usage,
) {
    if usage.is_empty() {
        return;
    }
    let agent = session.as_ref();
    let agent_id = agent.agent_id().to_string();
    let session_id = agent.session_id().to_string();
    let turn_id = agent.current_turn_id().await;
    let fallback_provider = agent.chat_provider().to_string();
    let fallback_base_url = agent.chat_base_url().to_string();
    let fallback_api_key = agent.chat_api_key().to_string();
    let fallback_model = agent.chat_model().to_string();

    let (model, provider, base_url, api_key) = if let Some(meta) = streamer.last_hit_meta() {
        let api_key = streamer.api_key_for(&meta);
        (meta.model, meta.backend_id, meta.base_url, api_key)
    } else {
        (
            if fallback_model.is_empty() {
                streamer.primary_model()
            } else {
                fallback_model
            },
            fallback_provider,
            fallback_base_url,
            fallback_api_key,
        )
    };

    apply_llm_usage_dual_write(
        &LlmUsageWrite {
            agent_id: &agent_id,
            session_id: Some(&session_id),
            turn_id: turn_id.as_deref(),
            model: &model,
            usage,
            provider: &provider,
            base_url: &base_url,
            api_key: &api_key,
        },
        None,
        Some(agent.sessions()),
    );
}

/// 发送 Error、RunFinished(error)、Done；若有已累计 usage 则先写入 `usage.db`。
pub(super) async fn finish_error(
    session: &Arc<AgentLoop>,
    streamer: &ProviderStreamer,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    msg: impl Into<String>,
    usage: Option<Usage>,
    run_id: &str,
) {
    let _ = emit(tx, MultiTurnStreamItem::Error(msg.into())).await;
    finish_run(session, streamer, tx, usage, run_id, RunPhase::Error).await;
}

/// 仅发送 Done，表示正常结束。
async fn finish_done(tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>) {
    let _ = emit(tx, MultiTurnStreamItem::Done).await;
}

/// 发送唯一 RunFinished 终态后 Done。
async fn finish_run(
    session: &Arc<AgentLoop>,
    streamer: &ProviderStreamer,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    usage: Option<Usage>,
    run_id: &str,
    phase: RunPhase,
) {
    debug_assert!(matches!(
        phase,
        RunPhase::Finished | RunPhase::Cancelled | RunPhase::Error
    ));
    if let Some(u) = usage {
        record_llm_usage(session, streamer, &u).await;
        let _ = emit(
            tx,
            MultiTurnStreamItem::Assistant(StreamedAssistantContent::FinalUsage(u)),
        )
        .await;
    }
    let mut state = RunState::new();
    state.set_phase(phase);
    let _ = emit(
        tx,
        MultiTurnStreamItem::RunFinished {
            run_id: run_id.to_string(),
            outcome_type: state.outcome_type().into(),
            interrupts_json: "[]".into(),
        },
    )
    .await;
    finish_done(tx).await;
}

/// 正常完成：可选发送累计 usage，再发送 RunFinished(success) 与 Done。
pub(crate) async fn finish_success(
    session: &Arc<AgentLoop>,
    streamer: &ProviderStreamer,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    usage: Option<Usage>,
    run_id: &str,
) {
    finish_run(session, streamer, tx, usage, run_id, RunPhase::Finished).await;
}

/// 用户取消或流控制中断：发送 RunFinished(interrupt) 与 Done。
pub(crate) async fn finish_interrupted(
    session: &Arc<AgentLoop>,
    streamer: &ProviderStreamer,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    usage: Option<Usage>,
    run_id: &str,
) {
    finish_run(session, streamer, tx, usage, run_id, RunPhase::Cancelled).await;
}
