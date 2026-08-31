use std::sync::Arc;

use agent_protocol::{AgentMessageItem, EventMsg, ItemEvent, TurnItem, TurnStartedEvent};
use tokio_util::sync::CancellationToken;

use crate::runtime::{Session, TurnContext};

use super::{SessionTask, SessionTaskResult, TaskKind, TurnCancelled, TurnInput};

pub(crate) struct CompactTask;

impl SessionTask for CompactTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Compact
    }

    fn span_name(&self) -> &'static str {
        "session_task.compact"
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        anyhow::ensure!(input.is_empty(), "compact task does not accept user input");
        session
            .send_event(
                ctx.sub_id(),
                EventMsg::TurnStarted(TurnStartedEvent {
                    turn_id: ctx.sub_id().to_string(),
                }),
            )
            .await;
        let pre = session.fire_hook(
            ::hooks::PRE_COMPACT,
            ::hooks::HookPayload {
                session_id: session.session_id().to_string(),
                turn_id: Some(ctx.sub_id().to_string()),
                trigger: Some("manual".into()),
                detail: "manual session compaction".into(),
                ..Default::default()
            },
        );
        if matches!(
            pre,
            ::hooks::HookOutcome::Block(_) | ::hooks::HookOutcome::Skip(_)
        ) {
            anyhow::bail!("manual compaction blocked by hook");
        }
        let summary = tokio::select! {
            _ = cancellation_token.cancelled() => return Err(TurnCancelled.into()),
            result = crate::exec::mid_run_summary::generate_manual_summary(session.as_ref()) => result?,
        };
        session
            .replace_history_with_compaction_summary(&summary)
            .await?;
        crate::streaming::lifecycle::emit_context_compacted(
            session.as_ref(),
            ctx.as_ref(),
            summary.clone(),
        )
        .await;
        session
            .send_event(
                ctx.sub_id(),
                EventMsg::ItemCompleted(ItemEvent {
                    turn_id: ctx.sub_id().to_string(),
                    item: TurnItem::AgentMessage(AgentMessageItem {
                        id: format!("compact-summary-{}", ctx.sub_id()),
                        content: summary.clone(),
                        delivery: None,
                    }),
                }),
            )
            .await;
        let _ = session.fire_hook(
            ::hooks::POST_COMPACT,
            ::hooks::HookPayload {
                session_id: session.session_id().to_string(),
                turn_id: Some(ctx.sub_id().to_string()),
                trigger: Some("manual".into()),
                detail: "manual session compaction completed".into(),
                ..Default::default()
            },
        );
        Ok(Some(summary))
    }
}
