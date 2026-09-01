use std::sync::Arc;

use agent_protocol::{Op, TurnInputMode, TurnInputRequest, TurnInputSubmission};
use agent_rollout::RolloutRecorder;
use tokio::sync::oneshot;

use super::session_io::{AgentStatus, SessionIo};
use super::submission_loop::submission_loop;
use super::{RuntimeIoBindError, Session};
use crate::streaming::ResponsesOverride;

/// 向单个会话长生命周期任务提交工作的稳定句柄。
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
        responses_override: Option<ResponsesOverride>,
    ) -> Result<Arc<Self>, RuntimeIoBindError> {
        let (io, rx_sub, event_tx, status_tx, termination_tx) = SessionIo::new();
        session.bind_runtime_io(event_tx, status_tx, rollout)?;
        session.bind_hook_run_events();

        let thread = Arc::new(Self {
            session: Arc::clone(&session),
            io,
        });
        tokio::spawn(async move {
            submission_loop(Arc::clone(&session), rx_sub, responses_override).await;
            session.close_event_stream();
            session.set_status(AgentStatus::Shutdown);
            let _ = termination_tx.send(true);
        });
        Ok(thread)
    }

    #[cfg(test)]
    fn spawn_with_responses_override(
        session: Arc<Session>,
        rollout: RolloutRecorder,
        responses_override: ResponsesOverride,
    ) -> Result<Arc<Self>, RuntimeIoBindError> {
        Self::spawn_inner(session, rollout, Some(responses_override))
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

    pub async fn recover_turn(
        &self,
        turn_id: impl Into<String>,
    ) -> Result<(String, TurnInputSubmission), agent_protocol::TurnInputError> {
        let (reply, reply_rx) = oneshot::channel();
        let submission_id = self
            .io
            .submit(Op::RecoverTurn {
                turn_id: turn_id.into(),
                reply,
            })
            .await
            .map_err(|_| agent_protocol::TurnInputError::QueueClosed)?;
        let submission = reply_rx
            .await
            .map_err(|_| agent_protocol::TurnInputError::ReplyClosed)??;
        Ok((submission_id, submission))
    }

    pub async fn suspend_turn_and_shutdown(
        &self,
    ) -> Result<agent_protocol::SuspendTurnOutcome, agent_protocol::TurnInputError> {
        let (reply, reply_rx) = oneshot::channel();
        self.io
            .submit(Op::SuspendTurnAndShutdown { reply })
            .await
            .map_err(|_| agent_protocol::TurnInputError::QueueClosed)?;
        reply_rx
            .await
            .map_err(|_| agent_protocol::TurnInputError::ReplyClosed)?
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

    use agent_rollout::{read_rollout, RolloutItem};
    use providers::types::message::Message as ProviderMessage;
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
    async fn native_tool_search_history_is_persisted_without_chat_projection() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("native-tool-search.jsonl");
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "native-tool-search".into(),
            )
            .await
            .unwrap(),
        );
        let thread = AstroThread::spawn(
            Arc::clone(&session),
            RolloutRecorder::open(path.clone()).await.unwrap(),
        )
        .unwrap();

        session
            .record_assistant_message_with_tools(
                "",
                Some(vec![types::message::ToolCall {
                    id: "call-search".into(),
                    name: "tool_search".into(),
                    arguments: serde_json::json!({"query": "image"}),
                    signature: None,
                }]),
                None,
                None,
            )
            .await
            .unwrap();
        session
            .record_tool_result_with_id(
                Some("call-search"),
                Some("tool_search"),
                "[{\"name\":\"image_gen\"}]",
            )
            .await
            .unwrap();
        thread.flush_rollout().await.unwrap();

        let items = read_rollout(&path).await.unwrap();
        assert!(matches!(
            items.first(),
            Some(RolloutItem::ResponseItem(
                agent_protocol::ResponseItem::ToolSearchCall { .. }
            ))
        ));
        assert!(matches!(
            items.get(1),
            Some(RolloutItem::ResponseItem(
                agent_protocol::ResponseItem::ToolSearchOutput { tools, .. }
            )) if tools.first().and_then(|tool| tool.get("name")).and_then(serde_json::Value::as_str)
                == Some("image_gen")
        ));

        thread.submit(Op::Shutdown).await.unwrap();
        thread.wait_terminated().await;
    }

    fn prompt_context(content: &str) -> crate::prompt::PromptContract {
        crate::prompt::PromptContract {
            base_instructions: "stable base".into(),
            context: vec![ProviderMessage::developer(content)],
            context_sections: vec![crate::prompt::contract::PromptContextSection {
                id: "mode".into(),
                role: crate::prompt::contract::PromptContextRole::Developer,
                content: content.into(),
            }],
            usage: Default::default(),
        }
    }

    #[tokio::test]
    async fn prompt_context_world_state_resumes_and_deduplicates() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("prompt-context.jsonl");
        let first_session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "prompt-context-first".into(),
            )
            .await
            .unwrap(),
        );
        let first_thread = AstroThread::spawn(
            Arc::clone(&first_session),
            RolloutRecorder::open(path.clone()).await.unwrap(),
        )
        .unwrap();

        first_session
            .record_items(vec![types::message::Message::user("first")])
            .await;
        first_session
            .persist_prompt_context_if_changed(&prompt_context("first"))
            .await;
        first_session
            .persist_prompt_context_if_changed(&prompt_context("first"))
            .await;
        first_thread.flush_rollout().await.unwrap();
        let initial_items = read_rollout(&path).await.unwrap();
        assert_eq!(
            initial_items
                .iter()
                .filter(|item| matches!(item, RolloutItem::WorldState(_)))
                .count(),
            1
        );

        first_thread.submit(Op::Shutdown).await.unwrap();
        first_thread.wait_terminated().await;
        drop(first_thread);
        drop(first_session);

        let resumed_session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "prompt-context-resumed".into(),
            )
            .await
            .unwrap(),
        );
        resumed_session.restore_prompt_context_from_rollout(&initial_items);
        assert_eq!(resumed_session.prompt_context_history().len(), 1);
        let resumed_thread = AstroThread::spawn(
            Arc::clone(&resumed_session),
            RolloutRecorder::open(path.clone()).await.unwrap(),
        )
        .unwrap();

        resumed_session
            .persist_prompt_context_if_changed(&prompt_context("first"))
            .await;
        resumed_session
            .record_items(vec![
                types::message::Message::user("first"),
                types::message::Message::assistant("answer"),
                types::message::Message::user("second"),
            ])
            .await;
        resumed_session
            .persist_prompt_context_if_changed(&prompt_context("second"))
            .await;
        let context_history = resumed_session.prompt_context_history();
        assert_eq!(context_history.len(), 2);
        assert_eq!(context_history[0].before_user, 1);
        assert_eq!(context_history[1].before_user, 2);
        assert!(context_history[1].messages[0]
            .text_content()
            .contains("second"));
        let step_context = resumed_session.capture_step_context().await.unwrap();
        assert_eq!(step_context.prompt_context.len(), 2);
        resumed_thread.flush_rollout().await.unwrap();

        let resumed_items = read_rollout(&path).await.unwrap();
        let world_states = resumed_items
            .iter()
            .filter_map(|item| match item {
                RolloutItem::WorldState(value) => Some(value),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(world_states.len(), 2);
        assert_eq!(world_states[0]["full"], true);
        assert_eq!(world_states[1]["full"], false);

        resumed_session
            .rebase_prompt_context_after_compaction("summary")
            .await;
        resumed_thread.flush_rollout().await.unwrap();
        assert_eq!(resumed_session.prompt_context_history().len(), 1);
        let rebased_items = read_rollout(&path).await.unwrap();
        assert!(matches!(
            &rebased_items[rebased_items.len() - 2],
            RolloutItem::Compacted(_)
        ));
        assert!(matches!(
            &rebased_items[rebased_items.len() - 1],
            RolloutItem::WorldState(value) if value["full"] == true
        ));
        let after_compaction = Session::with_session_id(
            Config::with_defaults(dir.path().to_path_buf()),
            "prompt-context-after-compaction".into(),
        )
        .await
        .unwrap();
        after_compaction.restore_prompt_context_from_rollout(&rebased_items);
        let compacted_history = after_compaction.prompt_context_history();
        assert_eq!(compacted_history.len(), 1);
        assert_eq!(compacted_history[0].before_user, 0);
        assert_eq!(compacted_history[0].messages[0].text_content(), "second");

        resumed_thread.submit(Op::Shutdown).await.unwrap();
        resumed_thread.wait_terminated().await;
    }

    #[tokio::test]
    async fn public_handle_observes_shutdown_and_closed_event_stream() {
        let dir = TempDir::new().unwrap();
        let session = Arc::new(
            Session::new(Config::with_defaults(dir.path().to_path_buf()))
                .await
                .unwrap(),
        );
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
        let session = Arc::new(
            Session::new(Config::with_defaults(dir.path().to_path_buf()))
                .await
                .unwrap(),
        );
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
    async fn thread_settings_are_applied_by_the_submission_actor() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path().join("project");
        let session = Arc::new(
            Session::new(Config::with_defaults(dir.path().to_path_buf()))
                .await
                .unwrap(),
        );
        let thread =
            AstroThread::spawn(Arc::clone(&session), recorder(&dir, "settings.jsonl").await)
                .unwrap();
        let submission_id = thread
            .submit(Op::ThreadSettings {
                thread_settings: agent_protocol::ThreadSettingsOverrides {
                    chat_targets: Some(vec![types::ChatTarget {
                        provider_id: "provider-1".into(),
                        backend_id: "openai".into(),
                        model: "gpt-test".into(),
                        api_key: "must-not-leak".into(),
                        base_url: "https://example.test".into(),
                    }]),
                    context_window: Some(64_000),
                    interaction_mode: Some(types::InteractionMode::Plan),
                    project_root: Some(Some(project_root.clone())),
                    workspace_roots: Some(vec![project_root.clone(), dir.path().to_path_buf()]),
                    temperature: Some(0.25),
                    thinking_enabled: Some(true),
                    reasoning_effort: Some("medium".into()),
                    max_tokens: Some(4096),
                    ..Default::default()
                },
            })
            .await
            .unwrap();

        let applied = loop {
            let event = thread.next_event().await.unwrap();
            if event.id == submission_id {
                if let agent_protocol::EventMsg::ThreadSettingsApplied(event) = event.msg {
                    break event.thread_settings;
                }
            }
        };
        assert_eq!(session.chat_model(), "gpt-test");
        assert_eq!(
            session.interaction_mode().await,
            types::InteractionMode::Plan
        );
        assert_eq!(
            session.project_root().as_deref(),
            Some(project_root.as_path())
        );
        assert_eq!(session.context_window(), 64_000);
        assert_eq!(session.temperature(), 0.25);
        assert_eq!(session.thread_provider_options().max_tokens, 4096);
        assert_eq!(applied.model, "gpt-test");
        assert_eq!(applied.interaction_mode, types::InteractionMode::Plan);
        assert!(!serde_json::to_string(&applied)
            .unwrap()
            .contains("must-not-leak"));

        thread.submit(Op::Shutdown).await.unwrap();
        thread.wait_terminated().await;
    }

    #[tokio::test]
    async fn actor_submit_turn_executes_the_model_loop() {
        let dir = TempDir::new().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "actor-model-loop".into(),
            )
            .await
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
        let responses_override: crate::streaming::ResponsesOverride = {
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
        let thread = AstroThread::spawn_with_responses_override(
            Arc::clone(&session),
            recorder(&dir, "actor.jsonl").await,
            responses_override,
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
                    thread_settings: Default::default(),
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
    async fn accepted_turn_applies_settings_before_turn_started() {
        let dir = TempDir::new().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "settings-accepted-turn".into(),
            )
            .await
            .unwrap(),
        );
        session.set_chat_targets(vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: "old-model".into(),
            api_key: String::new(),
            base_url: String::new(),
        }]);
        let observed_config = Arc::new(std::sync::Mutex::new(None));
        let responses_override: crate::streaming::ResponsesOverride = {
            let observed_config = Arc::clone(&observed_config);
            Arc::new(move |_messages, _tools, config| {
                *observed_config.lock().unwrap() = Some(config);
                Box::pin(async {
                    Ok(Box::pin(futures::stream::iter(vec![
                        Ok(StreamChunk::Text("configured".into())),
                        Ok(StreamChunk::Done {
                            finish_reason: "stop".into(),
                        }),
                    ])) as CompletionStream)
                })
            })
        };
        let thread = AstroThread::spawn_with_responses_override(
            Arc::clone(&session),
            recorder(&dir, "settings-accepted-turn.jsonl").await,
            responses_override,
        )
        .unwrap();

        let (_, submitted) = thread
            .submit_turn(
                TurnInputRequest {
                    input: vec![agent_protocol::TurnInput {
                        content: "use the new settings".into(),
                        image_data_urls: Vec::new(),
                        client_message_id: None,
                    }],
                    thread_settings: agent_protocol::ThreadSettingsOverrides {
                        chat_targets: Some(vec![types::ChatTarget {
                            provider_id: "scripted".into(),
                            backend_id: "scripted".into(),
                            model: "new-model".into(),
                            api_key: "secret".into(),
                            base_url: String::new(),
                        }]),
                        temperature: Some(0.2),
                        max_tokens: Some(1234),
                        ..Default::default()
                    },
                },
                TurnInputMode::StartIfIdle,
            )
            .await
            .unwrap();
        let turn_id = submitted.turn_id().unwrap().to_string();
        let mut settings_index = None;
        let mut started_index = None;
        let mut event_index = 0;
        timeout(Duration::from_secs(2), async {
            loop {
                let event = thread.next_event().await.unwrap();
                match event.msg {
                    agent_protocol::EventMsg::ThreadSettingsApplied(applied) => {
                        assert_eq!(applied.thread_settings.model, "new-model");
                        settings_index = Some(event_index);
                    }
                    agent_protocol::EventMsg::TurnStarted(started)
                        if started.turn_id == turn_id =>
                    {
                        started_index = Some(event_index);
                    }
                    agent_protocol::EventMsg::TurnComplete(complete)
                        if complete.turn_id == turn_id =>
                    {
                        break;
                    }
                    _ => {}
                }
                event_index += 1;
            }
        })
        .await
        .expect("configured turn should complete");

        assert!(settings_index.is_some_and(|index| index < started_index.unwrap()));
        let config = observed_config.lock().unwrap();
        let config = config.as_ref().expect("model call should observe settings");
        assert_eq!(config.model, "new-model");
        assert_eq!(config.temperature, 0.2);
        assert_eq!(config.max_tokens, 1234);

        thread.submit(Op::Shutdown).await.unwrap();
        timeout(Duration::from_secs(1), thread.wait_terminated())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rejected_turn_input_does_not_apply_its_thread_settings() {
        let dir = TempDir::new().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "settings-rejected-turn".into(),
            )
            .await
            .unwrap(),
        );
        session.set_chat_targets(vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: "test".into(),
            api_key: String::new(),
            base_url: String::new(),
        }]);
        let entered = Arc::new(tokio::sync::Notify::new());
        let responses_override: crate::streaming::ResponsesOverride = {
            let entered = Arc::clone(&entered);
            Arc::new(move |_messages, _tools, _config| {
                entered.notify_one();
                Box::pin(async {
                    Ok(
                        Box::pin(futures::stream::pending::<anyhow::Result<StreamChunk>>())
                            as CompletionStream,
                    )
                })
            })
        };
        let thread = AstroThread::spawn_with_responses_override(
            Arc::clone(&session),
            recorder(&dir, "settings-rejected-turn.jsonl").await,
            responses_override,
        )
        .unwrap();
        let (_, started) = thread
            .submit_turn(
                TurnInputRequest {
                    input: vec![agent_protocol::TurnInput {
                        content: "keep running".into(),
                        image_data_urls: Vec::new(),
                        client_message_id: None,
                    }],
                    thread_settings: Default::default(),
                },
                TurnInputMode::StartIfIdle,
            )
            .await
            .unwrap();
        assert!(matches!(started, TurnInputSubmission::Started { .. }));
        timeout(Duration::from_secs(1), entered.notified())
            .await
            .unwrap();

        let (_, rejected) = thread
            .submit_turn(
                TurnInputRequest {
                    input: vec![agent_protocol::TurnInput {
                        content: "do not accept".into(),
                        image_data_urls: Vec::new(),
                        client_message_id: None,
                    }],
                    thread_settings: agent_protocol::ThreadSettingsOverrides {
                        interaction_mode: Some(types::InteractionMode::Plan),
                        temperature: Some(1.5),
                        ..Default::default()
                    },
                },
                TurnInputMode::StartIfIdle,
            )
            .await
            .unwrap();
        assert!(matches!(
            rejected,
            TurnInputSubmission::NotSubmitted { ref reason } if reason == "not_idle"
        ));
        assert_eq!(
            session.interaction_mode().await,
            types::InteractionMode::Agent
        );
        assert_eq!(session.temperature(), 0.7);

        thread.submit(Op::Interrupt).await.unwrap();
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
            .await
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
        let responses_override: crate::streaming::ResponsesOverride = {
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
                                signature: None,
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
        let thread = AstroThread::spawn_with_responses_override(
            Arc::clone(&session),
            recorder(&dir, "actor-hitl.jsonl").await,
            responses_override,
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
                    thread_settings: Default::default(),
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

        let (_, gate, _) = session.ensure_thread_controls();
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

    #[tokio::test]
    async fn suspended_turn_recovers_under_the_same_turn_id_after_runtime_restart() {
        let dir = TempDir::new().unwrap();
        let session_id = "actor-recovery-loop";
        let rollout_root = dir.path().join("sessions").join("rollouts");
        let rollout_path =
            agent_rollout::new_rollout_path(&rollout_root, session_id, chrono::Utc::now());
        let first_session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                session_id.into(),
            )
            .await
            .unwrap(),
        );
        first_session.set_chat_targets(vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: "test".into(),
            api_key: String::new(),
            base_url: String::new(),
        }]);
        let entered = Arc::new(tokio::sync::Notify::new());
        let first_chat: crate::streaming::ResponsesOverride = {
            let entered = Arc::clone(&entered);
            Arc::new(move |_messages, _tools, _config| {
                entered.notify_one();
                Box::pin(async {
                    Ok(
                        Box::pin(futures::stream::pending::<anyhow::Result<StreamChunk>>())
                            as CompletionStream,
                    )
                })
            })
        };
        let first_thread = AstroThread::spawn_with_responses_override(
            first_session,
            RolloutRecorder::open(rollout_path.clone()).await.unwrap(),
            first_chat,
        )
        .unwrap();
        let (_, submitted) = first_thread
            .submit_turn(
                TurnInputRequest {
                    input: vec![agent_protocol::TurnInput {
                        content: "persist this once".into(),
                        image_data_urls: Vec::new(),
                        client_message_id: None,
                    }],
                    thread_settings: Default::default(),
                },
                TurnInputMode::StartIfIdle,
            )
            .await
            .unwrap();
        let turn_id = submitted.turn_id().unwrap().to_string();
        timeout(Duration::from_secs(2), entered.notified())
            .await
            .unwrap();

        assert_eq!(
            first_thread.suspend_turn_and_shutdown().await.unwrap(),
            agent_protocol::SuspendTurnOutcome::Suspended {
                turn_id: turn_id.clone()
            }
        );
        timeout(Duration::from_secs(2), first_thread.wait_terminated())
            .await
            .unwrap();

        let second_session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                session_id.into(),
            )
            .await
            .unwrap(),
        );
        second_session.set_chat_targets(vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: "test".into(),
            api_key: String::new(),
            base_url: String::new(),
        }]);
        let second_chat: crate::streaming::ResponsesOverride =
            Arc::new(move |_messages, _tools, _config| {
                Box::pin(async {
                    Ok(Box::pin(futures::stream::iter(
                        vec![
                            StreamChunk::Text("recovered".into()),
                            StreamChunk::Done {
                                finish_reason: "stop".into(),
                            },
                        ]
                        .into_iter()
                        .map(Ok),
                    )) as CompletionStream)
                })
            });
        let second_thread = AstroThread::spawn_with_responses_override(
            Arc::clone(&second_session),
            RolloutRecorder::open(rollout_path.clone()).await.unwrap(),
            second_chat,
        )
        .unwrap();
        let (_, recovered) = second_thread.recover_turn(turn_id.clone()).await.unwrap();
        assert_eq!(
            recovered,
            TurnInputSubmission::Started {
                turn_id: turn_id.clone()
            }
        );
        timeout(Duration::from_secs(2), async {
            loop {
                let event = second_thread.next_event().await.unwrap();
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
        .unwrap();
        assert_eq!(
            second_session
                .clone_history()
                .await
                .iter()
                .filter(|message| matches!(message.role, types::message::Role::User))
                .count(),
            1
        );

        second_thread.submit(Op::Shutdown).await.unwrap();
        timeout(Duration::from_secs(1), second_thread.wait_terminated())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn review_op_runs_as_a_review_task_with_mode_events() {
        let dir = TempDir::new().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "actor-review-loop".into(),
            )
            .await
            .unwrap(),
        );
        session.set_chat_targets(vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: "test".into(),
            api_key: String::new(),
            base_url: String::new(),
        }]);
        session.record_user_message("old user").await.unwrap();
        session
            .record_assistant_message("old assistant")
            .await
            .unwrap();
        let chat: crate::streaming::ResponsesOverride =
            Arc::new(move |messages, tools, _config| {
                let request = format!("{messages:?}");
                assert!(!request.contains("old user"));
                assert!(!request.contains("old assistant"));
                assert!(request.contains("review the current diff"));
                assert!(request.contains("You are a code reviewer"));
                let tools = serde_json::to_string(&tools).unwrap();
                assert!(!tools.contains("spawn_agent"));
                assert!(!tools.contains("web_search"));
                assert!(!tools.contains("image_gen"));
                assert!(!tools.contains("apply_patch"));
                Box::pin(async {
                    Ok(Box::pin(futures::stream::iter(
                        vec![
                            StreamChunk::Text("no findings".into()),
                            StreamChunk::Done {
                                finish_reason: "stop".into(),
                            },
                        ]
                        .into_iter()
                        .map(Ok),
                    )) as CompletionStream)
                })
            });
        let rollout_path = dir.path().join("actor-review.jsonl");
        let thread = AstroThread::spawn_with_responses_override(
            Arc::clone(&session),
            RolloutRecorder::open(rollout_path.clone()).await.unwrap(),
            chat,
        )
        .unwrap();
        let turn_id = thread
            .submit(Op::Review {
                review_request: agent_protocol::ReviewRequest {
                    target: agent_protocol::ReviewTarget::Custom {
                        instructions: "review the current diff".into(),
                    },
                    user_facing_hint: None,
                },
            })
            .await
            .unwrap();
        let mut entered = false;
        let mut exited = false;
        timeout(Duration::from_secs(2), async {
            loop {
                let event = thread.next_event().await.unwrap();
                match event.msg {
                    agent_protocol::EventMsg::ItemCompleted(ref event)
                        if matches!(event.item, agent_protocol::TurnItem::EnteredReviewMode(_)) =>
                    {
                        entered = true;
                    }
                    agent_protocol::EventMsg::ItemCompleted(ref event)
                        if matches!(event.item, agent_protocol::TurnItem::ExitedReviewMode(_)) =>
                    {
                        exited = true;
                    }
                    agent_protocol::EventMsg::TurnComplete(ref complete)
                        if complete.turn_id == turn_id =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        assert!(entered && exited);
        let history = session.clone_response_history().await;
        assert_eq!(history.len(), 4);
        let serialized = serde_json::to_string(&history).unwrap();
        assert_eq!(serialized.matches("old user").count(), 1);
        assert_eq!(serialized.matches("old assistant").count(), 1);
        assert_eq!(serialized.matches("review the current diff").count(), 1);
        assert_eq!(
            serialized.matches("no findings").count(),
            1,
            "history={serialized}"
        );
        let stored = session
            .sessions()
            .get_messages(session.session_id())
            .await
            .unwrap();
        assert_eq!(stored.len(), 4);
        assert!(session
            .sessions()
            .get_messages(&format!("{}::review::{turn_id}", session.session_id()))
            .await
            .unwrap()
            .is_empty());
        session.flush_rollout().await.unwrap();
        let rollout = read_rollout(&rollout_path).await.unwrap();
        let response_indices = rollout
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                matches!(item, RolloutItem::ResponseItem(_)).then_some(index)
            })
            .collect::<Vec<_>>();
        assert_eq!(response_indices.len(), 2);
        let exit_index = rollout
            .iter()
            .position(|item| {
                matches!(
                    item,
                    RolloutItem::EventMsg(agent_protocol::EventMsg::ItemCompleted(event))
                        if matches!(event.item, agent_protocol::TurnItem::ExitedReviewMode(_))
                )
            })
            .expect("review exit event persisted");
        assert!(
            response_indices[1] < exit_index,
            "review result must be durable before the exit event"
        );

        thread.submit(Op::Shutdown).await.unwrap();
        timeout(Duration::from_secs(1), thread.wait_terminated())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn interrupting_review_records_only_the_parent_exit_pair() {
        let dir = TempDir::new().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "actor-review-interrupt".into(),
            )
            .await
            .unwrap(),
        );
        session.set_chat_targets(vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: "test".into(),
            api_key: String::new(),
            base_url: String::new(),
        }]);
        let chat: crate::streaming::ResponsesOverride =
            Arc::new(move |_messages, _tools, _config| {
                Box::pin(async {
                    Ok(
                        Box::pin(futures::stream::pending::<anyhow::Result<StreamChunk>>())
                            as CompletionStream,
                    )
                })
            });
        let thread = AstroThread::spawn_with_responses_override(
            Arc::clone(&session),
            recorder(&dir, "actor-review-interrupt.jsonl").await,
            chat,
        )
        .unwrap();
        let turn_id = thread
            .submit(Op::Review {
                review_request: agent_protocol::ReviewRequest {
                    target: agent_protocol::ReviewTarget::UncommittedChanges,
                    user_facing_hint: None,
                },
            })
            .await
            .unwrap();
        timeout(Duration::from_secs(2), async {
            loop {
                let event = thread.next_event().await.unwrap();
                if matches!(
                    event.msg,
                    agent_protocol::EventMsg::ItemCompleted(ref event)
                        if matches!(event.item, agent_protocol::TurnItem::EnteredReviewMode(_))
                ) {
                    break;
                }
            }
        })
        .await
        .unwrap();

        thread.submit(Op::Interrupt).await.unwrap();
        let mut exited = false;
        timeout(Duration::from_secs(2), async {
            loop {
                let event = thread.next_event().await.unwrap();
                match event.msg {
                    agent_protocol::EventMsg::ItemCompleted(ref event)
                        if matches!(event.item, agent_protocol::TurnItem::ExitedReviewMode(_)) =>
                    {
                        exited = true;
                    }
                    agent_protocol::EventMsg::TurnAborted(ref aborted)
                        if aborted.turn_id.as_deref() == Some(&turn_id) =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        assert!(exited);
        let history = serde_json::to_string(&session.clone_response_history().await).unwrap();
        assert_eq!(
            history.matches("Review the working tree changes.").count(),
            1
        );
        assert_eq!(history.matches("Review was interrupted").count(), 1);

        thread.submit(Op::Shutdown).await.unwrap();
        timeout(Duration::from_secs(1), thread.wait_terminated())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn non_triggering_inter_agent_message_is_durable_and_model_visible() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("actor-inter-agent.jsonl");
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "actor-inter-agent".into(),
            )
            .await
            .unwrap(),
        );
        let thread = AstroThread::spawn(
            Arc::clone(&session),
            RolloutRecorder::open(path.clone()).await.unwrap(),
        )
        .unwrap();

        thread
            .submit(Op::InterAgentCommunication {
                communication: agent_protocol::InterAgentCommunication {
                    id: Some(agent_protocol::ResponseItemId::with_suffix("mail", "1")),
                    author: "/root/reviewer".into(),
                    recipient: "/root".into(),
                    other_recipients: Vec::new(),
                    content: "check the rollback boundary".into(),
                    encrypted_content: None,
                    internal_chat_message_metadata_passthrough: None,
                    trigger_turn: false,
                },
            })
            .await
            .unwrap();
        let barrier_id = thread
            .submit(Op::ThreadSettings {
                thread_settings: agent_protocol::ThreadSettingsOverrides::default(),
            })
            .await
            .unwrap();
        loop {
            let event = thread.next_event().await.unwrap();
            if event.id == barrier_id
                && matches!(
                    event.msg,
                    agent_protocol::EventMsg::ThreadSettingsApplied(_)
                )
            {
                break;
            }
        }
        let history = session.clone_response_history().await;
        assert!(matches!(
            history.last(),
            Some(agent_protocol::ResponseItem::AgentMessage { author, content, .. })
                if author == "/root/reviewer"
                    && matches!(content.first(), Some(agent_protocol::AgentMessageInputContent::InputText { text }) if text.contains("check the rollback boundary"))
        ));
        thread.flush_rollout().await.unwrap();
        assert!(read_rollout(&path).await.unwrap().iter().any(|item| {
            matches!(item, RolloutItem::InterAgentCommunication(payload) if payload["author"] == "/root/reviewer")
        }));

        thread.submit(Op::Shutdown).await.unwrap();
        thread.wait_terminated().await;
    }

    #[tokio::test]
    async fn triggering_inter_agent_message_starts_a_regular_turn() {
        let dir = TempDir::new().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "actor-inter-agent-trigger".into(),
            )
            .await
            .unwrap(),
        );
        session.set_chat_targets(vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: "test".into(),
            api_key: String::new(),
            base_url: String::new(),
        }]);
        let chat: crate::streaming::ResponsesOverride =
            Arc::new(move |_messages, _tools, _config| {
                Box::pin(async {
                    Ok(Box::pin(futures::stream::iter(
                        vec![
                            StreamChunk::Text("mail received".into()),
                            StreamChunk::Done {
                                finish_reason: "stop".into(),
                            },
                        ]
                        .into_iter()
                        .map(Ok),
                    )) as CompletionStream)
                })
            });
        let thread = AstroThread::spawn_with_responses_override(
            Arc::clone(&session),
            recorder(&dir, "actor-inter-agent-trigger.jsonl").await,
            chat,
        )
        .unwrap();

        let turn_id = thread
            .submit(Op::InterAgentCommunication {
                communication: agent_protocol::InterAgentCommunication {
                    id: Some(agent_protocol::ResponseItemId::with_suffix(
                        "mail", "trigger",
                    )),
                    author: "/root/reviewer".into(),
                    recipient: "/root".into(),
                    other_recipients: Vec::new(),
                    content: "continue from this result".into(),
                    encrypted_content: None,
                    internal_chat_message_metadata_passthrough: None,
                    trigger_turn: true,
                },
            })
            .await
            .unwrap();
        timeout(Duration::from_secs(2), async {
            loop {
                let event = thread.next_event().await.unwrap();
                if matches!(
                    event.msg,
                    agent_protocol::EventMsg::TurnComplete(ref complete)
                        if complete.turn_id == turn_id
                ) {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let history = session.clone_response_history().await;
        assert!(history.iter().any(|item| {
            matches!(item, agent_protocol::ResponseItem::Message { role, content, .. }
                if role == "user"
                    && matches!(content.first(), Some(agent_protocol::ContentItem::InputText { text }) if text.contains("continue from this result")))
        }));

        thread.submit(Op::Shutdown).await.unwrap();
        thread.wait_terminated().await;
    }
}
