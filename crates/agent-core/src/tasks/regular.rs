use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::runtime::{Session, TurnContext, TurnResult};
use crate::streaming::multi_turn::{run_turn, RunTurnArgs};

use super::{SessionTask, SessionTaskResult, TaskKind, TurnInput};

/// Standard model-and-tool turn.
pub(crate) struct RegularTask {
    args: Option<RunTurnArgs>,
}

impl RegularTask {
    pub(crate) fn new(args: RunTurnArgs) -> Self {
        Self { args: Some(args) }
    }

    pub(crate) fn submitted() -> Self {
        Self { args: None }
    }

    pub(crate) async fn run_legacy(
        self: Arc<Self>,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        self.run_with_args(ctx, input, cancellation_token).await
    }

    async fn run_with_args(
        &self,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        let stored_args = self
            .args
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("regular task has no legacy run arguments"))?;
        let args = stored_args.with_turn_context(ctx);
        let system_prompt = match args.prepared_system_prompt().map(str::to_owned) {
            Some(system_prompt) => {
                anyhow::ensure!(
                    input.is_empty(),
                    "prebuilt system prompt cannot be combined with initial input"
                );
                system_prompt
            }
            None => {
                let turn = args.session().lock().await.prepare_turn(&input).await?;
                match turn {
                    TurnResult::Continue { system_prompt, .. } => system_prompt,
                    TurnResult::BudgetExhausted => {
                        anyhow::bail!("conversation turn budget exhausted")
                    }
                    TurnResult::Interrupted => anyhow::bail!("regular turn interrupted"),
                    TurnResult::Steered { .. }
                    | TurnResult::ToolCalls(_)
                    | TurnResult::Finished(_)
                    | TurnResult::MaxDepth => {
                        anyhow::bail!("unsupported regular turn preparation result")
                    }
                }
            }
        };
        run_turn(args.with_system_prompt(system_prompt), cancellation_token).await;
        Ok(None)
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
        if self.args.is_none() {
            return match session.prepare_turn(&input).await? {
                TurnResult::Continue { .. } => Ok(None),
                TurnResult::BudgetExhausted => {
                    anyhow::bail!("conversation turn budget exhausted")
                }
                TurnResult::Interrupted => anyhow::bail!("regular turn interrupted"),
                TurnResult::Steered { .. }
                | TurnResult::ToolCalls(_)
                | TurnResult::Finished(_)
                | TurnResult::MaxDepth => {
                    anyhow::bail!("unsupported regular turn preparation result")
                }
            };
        }
        self.run_with_args(ctx, input, cancellation_token).await
    }
}
