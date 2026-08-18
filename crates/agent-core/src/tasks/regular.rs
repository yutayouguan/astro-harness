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
        let args = self.args.with_turn_context(ctx);
        args.session()
            .send_event(
                args.turn_context().sub_id(),
                EventMsg::TurnStarted(TurnStartedEvent {
                    turn_id: args.turn_context().sub_id().to_string(),
                }),
            )
            .await;
        let prepared = match args.prepared_system_prompt().map(str::to_owned) {
            Some(system_prompt) => {
                anyhow::ensure!(
                    input.is_empty(),
                    "prebuilt system prompt cannot be combined with initial input"
                );
                Ok(system_prompt)
            }
            None => match args.session().prepare_turn(&input).await {
                Err(error) => Err(error),
                Ok(TurnResult::Continue { system_prompt, .. }) => Ok(system_prompt),
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
        let system_prompt = match prepared {
            Ok(system_prompt) => system_prompt,
            Err(error) => {
                return Err(error);
            }
        };
        run_turn(args.with_system_prompt(system_prompt), cancellation_token).await
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
