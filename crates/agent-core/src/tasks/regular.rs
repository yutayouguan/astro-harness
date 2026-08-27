use std::sync::Arc;

use agent_protocol::{EventMsg, TurnStartedEvent};
use tokio_util::sync::CancellationToken;

use crate::runtime::{Session, TurnContext, TurnResult};
use crate::streaming::multi_turn::{run_turn, RunTurnArgs};

use super::{SessionTask, SessionTaskResult, TaskKind, TurnCancelled, TurnInput};

/// Standard model-and-tool turn.
pub(crate) struct RegularTask {
    args: RunTurnArgs,
}

impl RegularTask {
    pub(crate) fn new(args: RunTurnArgs) -> Self {
        Self { args }
    }

    async fn run_with_args(
        &self,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        let args = self.args.with_turn_context(Arc::clone(&ctx));
        args.session()
            .send_event(
                args.turn_context().sub_id(),
                EventMsg::TurnStarted(TurnStartedEvent {
                    turn_id: args.turn_context().sub_id().to_string(),
                }),
            )
            .await;
        let prepared = match args.prepared_prompt() {
            Some(prompt) => {
                anyhow::ensure!(
                    input.is_empty(),
                    "prebuilt prompt cannot be combined with initial input"
                );
                Ok(prompt)
            }
            None => match args.session().prepare_turn(&input).await {
                Err(error) => Err(error),
                Ok(TurnResult::Continue { prompt, .. }) => Ok(prompt),
                Ok(TurnResult::BudgetExhausted) => {
                    Err(anyhow::anyhow!("conversation turn budget exhausted"))
                }
                Ok(TurnResult::Interrupted) => Err(TurnCancelled.into()),
                Ok(
                    TurnResult::Steered { .. }
                    | TurnResult::ToolCalls(_)
                    | TurnResult::Finished(_)
                    | TurnResult::MaxDepth,
                ) => Err(anyhow::anyhow!(
                    "unsupported regular turn preparation result"
                )),
            },
        };
        let prompt = match prepared {
            Ok(prompt) => {
                ctx.open_input_admission();
                prompt
            }
            Err(error) => {
                ctx.close_input_admission();
                return Err(error);
            }
        };
        let result = run_turn(args.with_prompt(prompt), cancellation_token).await;
        let error = result.as_ref().err().map(ToString::to_string);
        let turn = args.session().session_turn().await;
        let _ = args.session().fire_hook(
            ::hooks::AGENT_END,
            ::hooks::HookPayload {
                turn_id: Some(ctx.sub_id().to_string()),
                turn: Some(turn),
                error,
                detail: format!("turn={turn}"),
                ..Default::default()
            },
        );
        result
    }
}

impl SessionTask for RegularTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }

    fn span_name(&self) -> &'static str {
        "session_task.turn"
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        debug_assert!(Arc::ptr_eq(self.args.session(), &session));
        self.run_with_args(ctx, input, cancellation_token).await
    }
}
