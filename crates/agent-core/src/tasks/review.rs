use std::sync::Arc;

use agent_protocol::{
    EventMsg, ItemEvent, ReviewRequest, ReviewTarget, TextItem, TurnItem, TurnStartedEvent,
};
use tokio_util::sync::CancellationToken;

use crate::runtime::{Session, TurnContext, TurnResult};
use crate::streaming::multi_turn::{run_turn, RunTurnArgs};

use super::{SessionTask, SessionTaskResult, TaskKind, TurnCancelled, TurnInput};

pub(crate) struct ReviewTask {
    args: RunTurnArgs,
    request: ReviewRequest,
}

impl ReviewTask {
    pub(crate) fn new(args: RunTurnArgs, request: ReviewRequest) -> Self {
        Self { args, request }
    }

    fn prompt(&self) -> String {
        let target = match &self.request.target {
            ReviewTarget::UncommittedChanges => "Review the working tree changes.".to_string(),
            ReviewTarget::BaseBranch { branch } => {
                format!("Review the changes against base branch `{branch}`.")
            }
            ReviewTarget::Commit { sha, title } => match title {
                Some(title) => format!("Review commit `{sha}` ({title})."),
                None => format!("Review commit `{sha}`."),
            },
            ReviewTarget::Custom { instructions } => instructions.clone(),
        };
        match self.request.user_facing_hint.as_deref() {
            Some(hint) if !hint.trim().is_empty() => {
                format!("{target}\n\nAdditional context: {hint}")
            }
            _ => target,
        }
    }

    async fn exit_review_mode(
        session: &Session,
        ctx: &TurnContext,
        mode_item_id: String,
        content: String,
    ) {
        session
            .send_event(
                ctx.sub_id(),
                EventMsg::ItemCompleted(ItemEvent {
                    turn_id: ctx.sub_id().to_string(),
                    item: TurnItem::ExitedReviewMode(TextItem {
                        id: mode_item_id,
                        content,
                    }),
                }),
            )
            .await;
    }
}

impl SessionTask for ReviewTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Review
    }

    fn span_name(&self) -> &'static str {
        "session_task.review"
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        anyhow::ensure!(input.is_empty(), "review task owns its generated input");
        session
            .send_event(
                ctx.sub_id(),
                EventMsg::TurnStarted(TurnStartedEvent {
                    turn_id: ctx.sub_id().to_string(),
                }),
            )
            .await;
        let mode_item_id = format!("review-mode-{}", ctx.sub_id());
        session
            .send_event(
                ctx.sub_id(),
                EventMsg::ItemCompleted(ItemEvent {
                    turn_id: ctx.sub_id().to_string(),
                    item: TurnItem::EnteredReviewMode(TextItem {
                        id: mode_item_id.clone(),
                        content: self.prompt(),
                    }),
                }),
            )
            .await;
        let prompt_text = self.prompt();
        let prepared = match session
            .prepare_turn(&[TurnInput {
                content: prompt_text,
                image_data_urls: Vec::new(),
                client_message_id: None,
            }])
            .await?
        {
            TurnResult::Continue { prompt, .. } => prompt,
            TurnResult::BudgetExhausted => {
                anyhow::bail!("conversation turn budget exhausted")
            }
            TurnResult::Interrupted => return Err(TurnCancelled.into()),
            _ => anyhow::bail!("unsupported review turn preparation result"),
        };
        ctx.open_input_admission();
        let result = run_turn(
            self.args
                .with_turn_context(Arc::clone(&ctx))
                .with_prompt(prepared),
            cancellation_token.clone(),
        )
        .await;
        if !cancellation_token.is_cancelled() {
            let content = result
                .as_ref()
                .ok()
                .and_then(|message| message.clone())
                .unwrap_or_else(|| "review complete".into());
            Self::exit_review_mode(session.as_ref(), ctx.as_ref(), mode_item_id, content).await;
        }
        let error = result.as_ref().err().map(ToString::to_string);
        let turn = session.session_turn().await;
        let _ = session.fire_hook(
            ::hooks::AGENT_END,
            ::hooks::HookPayload {
                session_id: session.session_id().to_string(),
                turn_id: Some(ctx.sub_id().to_string()),
                turn: Some(turn),
                error,
                detail: format!("review_turn={turn}"),
                ..Default::default()
            },
        );
        result
    }

    async fn abort(&self, session: Arc<Session>, ctx: Arc<TurnContext>) {
        Self::exit_review_mode(
            session.as_ref(),
            ctx.as_ref(),
            format!("review-mode-{}", ctx.sub_id()),
            "review interrupted".into(),
        )
        .await;
    }
}
