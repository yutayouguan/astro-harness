use std::sync::Arc;

use agent_protocol::{Op, TurnInputMode, TurnInputRequest, TurnInputSubmission};
use agent_rollout::RolloutRecorder;
use tokio::sync::oneshot;

use super::session_io::SessionIo;
use super::submission_loop::submission_loop;
use super::Session;

/// Stable handle for submitting work to one session's long-lived task.
pub struct AstroThread {
    session: Arc<Session>,
    io: SessionIo,
}

impl AstroThread {
    pub fn spawn(session: Arc<Session>, rollout: RolloutRecorder) -> Arc<Self> {
        let (io, rx_sub, event_tx, status_tx, termination_tx) = SessionIo::new();
        session.bind_runtime_io(event_tx, status_tx, rollout);

        let thread = Arc::new(Self {
            session: Arc::clone(&session),
            io,
        });
        tokio::spawn(async move {
            submission_loop(session, rx_sub).await;
            let _ = termination_tx.send(true);
        });
        thread
    }

    pub async fn submit(&self, op: Op) -> anyhow::Result<String> {
        self.io.submit(op).await.map_err(anyhow::Error::from)
    }

    pub async fn submit_turn(
        &self,
        request: TurnInputRequest,
        mode: TurnInputMode,
    ) -> anyhow::Result<(String, TurnInputSubmission)> {
        let (reply, reply_rx) = oneshot::channel();
        let submission_id = self
            .submit(Op::TurnInput {
                request,
                mode,
                reply,
            })
            .await?;
        let submission = reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("turn input reply channel closed"))??;
        Ok((submission_id, submission))
    }

    pub async fn next_event(&self) -> anyhow::Result<agent_protocol::Event> {
        self.io.next_event().await.map_err(anyhow::Error::from)
    }

    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    pub async fn flush_rollout(&self) -> std::io::Result<()> {
        self.session.flush_rollout().await
    }
}
