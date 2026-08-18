use std::sync::Arc;

use agent_protocol::{Op, TurnInputMode, TurnInputRequest, TurnInputSubmission};
use agent_rollout::RolloutRecorder;
use tokio::sync::oneshot;

use super::session_io::{AgentStatus, SessionIo};
use super::submission_loop::submission_loop;
use super::{RuntimeIoBindError, Session};

/// Stable handle for submitting work to one session's long-lived task.
pub struct AstroThread {
    session: Arc<Session>,
    io: SessionIo,
}

impl AstroThread {
    pub fn spawn(
        session: Arc<Session>,
        rollout: RolloutRecorder,
    ) -> Result<Arc<Self>, RuntimeIoBindError> {
        let (io, rx_sub, event_tx, status_tx, termination_tx) = SessionIo::new();
        session.bind_runtime_io(event_tx, status_tx, rollout)?;

        let thread = Arc::new(Self {
            session: Arc::clone(&session),
            io,
        });
        tokio::spawn(async move {
            submission_loop(Arc::clone(&session), rx_sub).await;
            session.close_event_stream();
            session.set_status(AgentStatus::Shutdown);
            let _ = termination_tx.send(true);
        });
        Ok(thread)
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

    pub fn status(&self) -> AgentStatus {
        self.io.status()
    }

    pub fn subscribe_status(&self) -> tokio::sync::watch::Receiver<AgentStatus> {
        self.io.subscribe_status()
    }

    pub async fn wait_terminated(&self) {
        self.io.wait_terminated().await;
    }

    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    pub async fn flush_rollout(&self) -> std::io::Result<()> {
        self.session.flush_rollout().await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use agent_rollout::ThreadHistoryMode;
    use tempfile::TempDir;
    use tokio::time::timeout;

    use super::*;
    use crate::runtime::{AgentStatus, Config, RuntimeIoBindError};

    async fn recorder(dir: &TempDir, name: &str) -> RolloutRecorder {
        RolloutRecorder::open(dir.path().join(name), ThreadHistoryMode::Paginated)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn public_handle_observes_shutdown_and_closed_event_stream() {
        let dir = TempDir::new().unwrap();
        let session =
            Arc::new(Session::new(Config::with_defaults(dir.path().to_path_buf())).unwrap());
        let thread =
            AstroThread::spawn(Arc::clone(&session), recorder(&dir, "first.jsonl").await).unwrap();
        let mut statuses = thread.subscribe_status();

        assert_eq!(thread.status(), AgentStatus::Idle);
        thread.submit(Op::Shutdown).await.unwrap();
        timeout(Duration::from_secs(1), thread.wait_terminated())
            .await
            .expect("thread should terminate after shutdown");

        assert_eq!(thread.status(), AgentStatus::Shutdown);
        statuses.changed().await.unwrap();
        assert_eq!(*statuses.borrow(), AgentStatus::Shutdown);
        assert!(timeout(Duration::from_secs(1), thread.next_event())
            .await
            .expect("event stream should close after shutdown")
            .is_err());
    }

    #[tokio::test]
    async fn second_spawn_returns_already_bound_and_preserves_first_thread() {
        let dir = TempDir::new().unwrap();
        let session =
            Arc::new(Session::new(Config::with_defaults(dir.path().to_path_buf())).unwrap());
        let first =
            AstroThread::spawn(Arc::clone(&session), recorder(&dir, "first.jsonl").await).unwrap();
        let rejected_recorder = recorder(&dir, "rejected.jsonl").await;

        let second = AstroThread::spawn(Arc::clone(&session), rejected_recorder.clone());
        assert!(matches!(second, Err(RuntimeIoBindError::AlreadyBound)));
        rejected_recorder.shutdown().await.unwrap();

        first.submit(Op::Shutdown).await.unwrap();
        timeout(Duration::from_secs(1), first.wait_terminated())
            .await
            .expect("first thread should remain usable");
        assert_eq!(first.status(), AgentStatus::Shutdown);
    }
}
