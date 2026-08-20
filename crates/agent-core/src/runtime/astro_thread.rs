use std::sync::Arc;

use agent_protocol::{Op, TurnInputMode, TurnInputRequest, TurnInputSubmission};
use agent_rollout::RolloutRecorder;
use tokio::sync::oneshot;

use super::session_io::{AgentStatus, SessionIo};
use super::submission_loop::submission_loop;
use super::{RuntimeIoBindError, Session};
use crate::streaming::ChatOverride;

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
        Self::spawn_inner(session, rollout, None)
    }

    fn spawn_inner(
        session: Arc<Session>,
        rollout: RolloutRecorder,
        chat_override: Option<ChatOverride>,
    ) -> Result<Arc<Self>, RuntimeIoBindError> {
        let (io, rx_sub, event_tx, status_tx, termination_tx) = SessionIo::new();
        session.bind_runtime_io(event_tx, status_tx, rollout)?;

        let thread = Arc::new(Self {
            session: Arc::clone(&session),
            io,
        });
        tokio::spawn(async move {
            submission_loop(Arc::clone(&session), rx_sub, chat_override).await;
            session.close_event_stream();
            session.set_status(AgentStatus::Shutdown);
            let _ = termination_tx.send(true);
        });
        Ok(thread)
    }

    #[cfg(test)]
    fn spawn_with_chat_override(
        session: Arc<Session>,
        rollout: RolloutRecorder,
        chat_override: ChatOverride,
    ) -> Result<Arc<Self>, RuntimeIoBindError> {
        Self::spawn_inner(session, rollout, Some(chat_override))
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use providers::types::stream::StreamChunk;
    use providers::CompletionStream;
    use tempfile::TempDir;
    use tokio::time::timeout;

    use super::*;
    use crate::runtime::{AgentStatus, Config, RuntimeIoBindError};

    async fn recorder(dir: &TempDir, name: &str) -> RolloutRecorder {
        RolloutRecorder::open(dir.path().join(name)).await.unwrap()
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
        assert!(matches!(
            timeout(Duration::from_secs(1), thread.next_event())
                .await
                .expect("shutdown event should be delivered before stream close")
                .unwrap()
                .msg,
            agent_protocol::EventMsg::ShutdownComplete
        ));
        assert!(thread.next_event().await.is_err());
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

    #[tokio::test]
    async fn actor_submit_turn_executes_the_model_loop() {
        let dir = TempDir::new().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "actor-model-loop".into(),
            )
            .unwrap(),
        );
        session.set_chat_targets(vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: "test".into(),
            api_key: String::new(),
            base_url: String::new(),
        }]);
        let calls = Arc::new(AtomicUsize::new(0));
        let called = Arc::new(tokio::sync::Notify::new());
        let chat_override: crate::streaming::ChatOverride = {
            let calls = Arc::clone(&calls);
            let called = Arc::clone(&called);
            Arc::new(move |_messages, _tools, _config| {
                let calls = Arc::clone(&calls);
                let called = Arc::clone(&called);
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    called.notify_one();
                    Ok(Box::pin(futures::stream::iter(vec![
                        Ok(StreamChunk::Text("actor reply".into())),
                        Ok(StreamChunk::Done {
                            finish_reason: "stop".into(),
                        }),
                    ])) as CompletionStream)
                })
            })
        };
        let thread = AstroThread::spawn_with_chat_override(
            Arc::clone(&session),
            recorder(&dir, "actor.jsonl").await,
            chat_override,
        )
        .unwrap();

        let (_, submitted) = thread
            .submit_turn(
                TurnInputRequest {
                    input: vec![agent_protocol::TurnInput {
                        content: "call the model".into(),
                        image_data_urls: Vec::new(),
                        client_message_id: None,
                    }],
                },
                TurnInputMode::StartIfIdle,
            )
            .await
            .unwrap();
        assert!(matches!(submitted, TurnInputSubmission::Started { .. }));
        timeout(Duration::from_secs(1), called.notified())
            .await
            .expect("actor submission should enter the model loop");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        thread.submit(Op::Shutdown).await.unwrap();
        timeout(Duration::from_secs(1), thread.wait_terminated())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn actor_submit_turn_keeps_hitl_control_event_and_resume_reachable() {
        let dir = TempDir::new().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "actor-hitl-loop".into(),
            )
            .unwrap(),
        );
        session.set_chat_targets(vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: "test".into(),
            api_key: String::new(),
            base_url: String::new(),
        }]);
        let calls = Arc::new(AtomicUsize::new(0));
        let chat_override: crate::streaming::ChatOverride = {
            let calls = Arc::clone(&calls);
            Arc::new(move |_messages, _tools, _config| {
                let call = calls.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    let chunks = if call == 0 {
                        vec![
                            StreamChunk::ToolCallStart {
                                index: 0,
                                id: "ask-1".into(),
                                name: "ask_user".into(),
                            },
                            StreamChunk::ToolCallDelta {
                                index: 0,
                                arguments:
                                    r#"{"mode":"confirm","title":"Continue?","body":"Proceed?"}"#
                                        .into(),
                            },
                            StreamChunk::Done {
                                finish_reason: "tool_calls".into(),
                            },
                        ]
                    } else {
                        vec![
                            StreamChunk::Text("resumed".into()),
                            StreamChunk::Done {
                                finish_reason: "stop".into(),
                            },
                        ]
                    };
                    Ok(Box::pin(futures::stream::iter(chunks.into_iter().map(Ok)))
                        as CompletionStream)
                })
            })
        };
        let thread = AstroThread::spawn_with_chat_override(
            Arc::clone(&session),
            recorder(&dir, "actor-hitl.jsonl").await,
            chat_override,
        )
        .unwrap();
        let (_, submitted) = thread
            .submit_turn(
                TurnInputRequest {
                    input: vec![agent_protocol::TurnInput {
                        content: "ask before acting".into(),
                        image_data_urls: Vec::new(),
                        client_message_id: None,
                    }],
                },
                TurnInputMode::StartIfIdle,
            )
            .await
            .unwrap();
        let turn_id = submitted.turn_id().unwrap().to_string();

        let request_id = timeout(Duration::from_secs(2), async {
            loop {
                let event = thread.next_event().await.unwrap();
                if let agent_protocol::EventMsg::RequestUserInput(request) = event.msg {
                    break request.request_id;
                }
            }
        })
        .await
        .expect("actor thread should emit a HITL control event");
        assert!(!request_id.is_empty());

        let (_, gate) = session.ensure_thread_controls();
        let pending = gate.pending_interrupts().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, request_id);
        gate.resolve(&[crate::ResumeItem {
            interrupt_id: pending[0].id.clone(),
            status: "resolved".into(),
            payload_json: r#"{"approved":true}"#.into(),
        }])
        .await
        .unwrap();

        timeout(Duration::from_secs(2), async {
            loop {
                let event = thread.next_event().await.unwrap();
                if matches!(
                    event.msg,
                    agent_protocol::EventMsg::TurnComplete(ref complete)
                        if complete.turn_id == turn_id && complete.error.is_none()
                ) {
                    break;
                }
            }
        })
        .await
        .expect("resumed HITL turn should complete");
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        thread.submit(Op::Shutdown).await.unwrap();
        timeout(Duration::from_secs(1), thread.wait_terminated())
            .await
            .unwrap();
    }
}
