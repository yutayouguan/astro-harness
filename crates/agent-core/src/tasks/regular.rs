use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::runtime::{Session, TurnContext, TurnResult};
use crate::streaming::multi_turn::{run_turn, RunTurnArgs};

use super::{SessionTask, SessionTaskResult, TaskKind, TurnInput};

/// Standard model-and-tool turn.
pub(crate) struct RegularTask {
    args: RunTurnArgs,
}

impl RegularTask {
    pub(crate) fn new(args: RunTurnArgs) -> Self {
        Self { args }
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
        sess: Arc<Mutex<Session>>,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        let args = self.args.with_session_and_turn(Arc::clone(&sess), ctx);
        let system_prompt = match args.prepared_system_prompt().map(str::to_owned) {
            Some(system_prompt) => {
                anyhow::ensure!(
                    input.is_empty(),
                    "prebuilt system prompt cannot be combined with initial input"
                );
                system_prompt
            }
            None => {
                let turn = sess.lock().await.prepare_turn(&input).await?;
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
