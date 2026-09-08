use agent_protocol::{
    ContentItem, ErrorEvent, Event, EventMsg, ResponseItem, ThreadRolledBackEvent,
};
use agent_rollout::RolloutItem;
use session::ConversationStore;

use super::Session;

const COMPACTION_SUMMARY_MARK: &str = "[astro:compaction-summary]";

impl Session {
    pub(crate) async fn referenced_history(
        &self,
        history: &[ResponseItem],
    ) -> anyhow::Result<Vec<Option<String>>> {
        let Some(bindings) = self.runtime_io.get() else {
            return Ok(Vec::new());
        };
        bindings.rollout.flush().await?;
        if bindings.rollout.path().as_os_str().is_empty() {
            return Ok(Vec::new());
        }
        Ok(agent_rollout::response_history_references(bindings.rollout.path(), history).await?)
    }

    /// Only called at a sampling boundary after every tool output has been persisted.
    pub(crate) async fn apply_requested_context_compaction(
        &self,
        turn_id: &str,
    ) -> anyhow::Result<bool> {
        if !self
            .services
            .sessions
            .take_context_compaction(&self.session_id, turn_id)
            .await?
        {
            return Ok(false);
        }
        let result: anyhow::Result<()> = async {
            anyhow::ensure!(
                self.runtime_io.get().is_some(),
                "compaction requires bound rollout persistence"
            );
            let pre = self.run_pre_compact_hook(Some(turn_id.to_owned()), "tool");
            anyhow::ensure!(
                !pre.should_stop,
                "compaction blocked by hook: {}",
                pre.stop_reason.unwrap_or_default()
            );
            let summary = tokio::select! {
                _ = async {
                    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(100));
                    while !self.cancel.is_cancelled() { ticker.tick().await; }
                } => anyhow::bail!("compaction cancelled"),
                summary = crate::exec::mid_run_summary::generate_manual_summary(self) => summary?,
            };
            anyhow::ensure!(!self.cancel.is_cancelled(), "compaction cancelled");
            self.replace_history_with_compaction_summary(&summary)
                .await?;
            let _ = self.run_post_compact_hook(Some(turn_id.to_owned()), "tool");
            Ok(())
        }
        .await;
        let status = match &result {
            Ok(()) => "completed".to_string(),
            Err(error) => format!(
                "failed: {}",
                error.to_string().chars().take(1_000).collect::<String>()
            ),
        };
        self.services
            .sessions
            .finish_context_compaction(&self.session_id, turn_id, &status)
            .await?;
        result.map(|()| true)
    }

    pub(crate) async fn replace_history_with_compaction_summary(
        &self,
        summary: &str,
    ) -> anyhow::Result<()> {
        let replacement = vec![ResponseItem::Message {
            id: None,
            role: "developer".into(),
            content: vec![ContentItem::InputText {
                text: format!("{COMPACTION_SUMMARY_MARK}\n{}", summary.trim()),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }];
        let bindings = self
            .runtime_io
            .get()
            .ok_or_else(|| anyhow::anyhow!("compaction requires bound rollout persistence"))?;
        bindings
            .rollout
            .record(vec![RolloutItem::Compacted(serde_json::json!({
                "reason": "manual",
                "replacement_history": &replacement,
            }))])
            .await?;
        {
            let mut state = self.lock_state();
            state.replace_history(replacement);
            state.compression.mid_run_handoff = None;
            state.sampled_context_tokens = None;
        }
        self.rebase_prompt_context_after_compaction(summary).await;
        Ok(())
    }

    pub(crate) async fn rollback_thread(&self, submission_id: String, num_turns: u32) {
        if num_turns == 0 {
            self.emit_rollback_error(submission_id, "num_turns must be >= 1")
                .await;
            return;
        }
        if self.active_turn.lock().await.is_some() {
            self.emit_rollback_error(submission_id, "cannot rollback while a turn is in progress")
                .await;
            return;
        }
        if let Err(error) = self
            .rollback_history(submission_id.clone(), num_turns, None)
            .await
        {
            self.emit_rollback_error(submission_id, error.to_string())
                .await;
        }
    }

    /// Idempotently replace the active history with an absolute chat-bubble prefix.
    /// Used immediately before an edited user input is resubmitted.
    pub(crate) async fn rollback_thread_to_bubbles(
        &self,
        submission_id: String,
        keep_chat_bubbles: u32,
    ) -> anyhow::Result<()> {
        self.rollback_history(submission_id, 0, Some(keep_chat_bubbles))
            .await
    }

    async fn rollback_history(
        &self,
        submission_id: String,
        num_turns: u32,
        keep_chat_bubbles: Option<u32>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.active_turn.lock().await.is_none(),
            "cannot rollback while a turn is in progress"
        );
        let _write_guard = self.conversation_write_lock.lock().await;
        let bindings = self
            .runtime_io
            .get()
            .ok_or_else(|| anyhow::anyhow!("thread rollback requires persisted thread history"))?;
        bindings.rollout.flush().await.map_err(|error| {
            anyhow::anyhow!("failed to flush thread persistence for rollback replay: {error}")
        })?;
        let items = agent_rollout::read_rollout(bindings.rollout.path())
            .await
            .map_err(|error| {
                anyhow::anyhow!("failed to load thread history for rollback replay: {error}")
            })?;
        let mut replacement = agent_rollout::effective_response_history(&items);
        let previous_turns = agent_rollout::user_turn_count(&replacement);
        if let Some(keep) = keep_chat_bubbles {
            agent_rollout::truncate_to_chat_bubbles(&mut replacement, keep as usize);
        } else {
            agent_rollout::drop_last_n_user_turns(&mut replacement, num_turns);
        }
        let removed_turns = previous_turns
            .saturating_sub(agent_rollout::user_turn_count(&replacement))
            .try_into()
            .unwrap_or(u32::MAX);
        let event = EventMsg::ThreadRolledBack(ThreadRolledBackEvent {
            num_turns: removed_turns,
            keep_chat_bubbles,
        });
        self.services
            .sessions
            .invalidate_thread_notes(&self.session_id)
            .await?;
        bindings
            .rollout
            .record(vec![RolloutItem::EventMsg(event.clone())])
            .await
            .map_err(|error| anyhow::anyhow!("failed to persist thread rollback: {error}"))?;
        {
            let mut state = self.lock_state();
            state.replace_history(replacement.clone());
            state.prompt_context_snapshot = None;
            state.prompt_context_history.clear();
        }
        self.send_event_raw_with_persistence(
            Event {
                id: submission_id,
                msg: event,
            },
            false,
        )
        .await;
        self.services
            .sessions
            .replace_response_items(&self.session_id, &replacement)
            .await
            .map_err(|error| anyhow::anyhow!("failed to project thread rollback: {error}"))?;
        Ok(())
    }

    async fn emit_rollback_error(&self, submission_id: String, message: impl Into<String>) {
        self.send_event(
            &submission_id,
            EventMsg::Error(ErrorEvent {
                message: message.into(),
                error_type: "thread_rollback_failed".into(),
            }),
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use agent_protocol::{ContentItem, EventMsg, Op, ResponseItem};
    use agent_rollout::RolloutRecorder;
    use tempfile::TempDir;

    use crate::runtime::{AstroThread, Config, Session};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn queued_compaction_persists_batch_then_restores_summary_and_notes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = TempDir::new().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let n = socket.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                request.extend_from_slice(&chunk[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:").map(str::trim))
                        .unwrap()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.contains("BATCH_DONE"));
            assert!(request.contains("CHECKPOINT_KEEP"));
            assert!(request.contains("history_ref"));
            let body = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Goal: continue; BATCH_DONE verified.\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"message\"}]}}\n\n";
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        let (session, thread, path) = thread(&dir, "queued-compaction").await;
        session.set_model_targets(vec![types::ModelTarget {
            provider_id: "local-test".into(),
            backend_id: "openai".into(),
            model: "test".into(),
            api_key: "test-key".into(),
            base_url: format!("http://{addr}/v1"),
        }]);
        session.set_current_turn_id("turn-test").await;
        session
            .sessions()
            .ensure_session(session.session_id(), "test")
            .await
            .unwrap();
        session
            .sessions()
            .write_thread_notes(session.session_id(), "CHECKPOINT_KEEP", 0)
            .await
            .unwrap();
        session
            .record_response_items(vec![
                message("user", "Continue the task"),
                ResponseItem::FunctionCall {
                    id: None,
                    name: "new_context_window".into(),
                    namespace: None,
                    arguments: "{}".into(),
                    encrypted_function_args: None,
                    call_id: "compact-call".into(),
                    internal_chat_message_metadata_passthrough: None,
                },
            ])
            .await
            .unwrap();
        let queued = session
            .handle_tool_call_async("new_context_window", &serde_json::json!({}))
            .await
            .unwrap();
        assert!(queued.text().contains("not yet completed"));
        assert_eq!(session.clone_history().await.len(), 2);
        session
            .record_response_items(vec![ResponseItem::FunctionCallOutput {
                id: None,
                call_id: Some("compact-call".into()),
                name: Some("new_context_window".into()),
                namespace: None,
                output: agent_protocol::FunctionCallOutputPayload::from_text(format!(
                    "{} BATCH_DONE",
                    queued.text()
                )),
                internal_chat_message_metadata_passthrough: None,
            }])
            .await
            .unwrap();
        let applied = tokio::time::timeout(
            // Exercise a real local HTTP stream without putting both peers on one worker.
            // Match the provider's bounded connection deadline under loaded CI/build hosts.
            std::time::Duration::from_secs(30),
            session.apply_requested_context_compaction("turn-test"),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(applied);
        server.await.unwrap();
        assert_eq!(session.clone_history().await.len(), 1);
        assert_eq!(
            session
                .sessions()
                .thread_context(session.session_id())
                .await
                .unwrap()
                .compaction_status,
            "completed"
        );
        thread.submit(Op::Shutdown).await.unwrap();
        thread.wait_terminated().await;
        let records = agent_rollout::read_rollout(&path).await.unwrap();
        let output_position = records
            .iter()
            .position(
                |r| matches!(r, agent_rollout::RolloutItem::ResponseItem(i) if i.is_tool_output()),
            )
            .unwrap();
        let compact_position = records
            .iter()
            .position(|r| matches!(r, agent_rollout::RolloutItem::Compacted(_)))
            .unwrap();
        assert!(output_position < compact_position);
        let restored = Session::with_session_id(
            Config::with_defaults(dir.path().to_path_buf()),
            "queued-compaction".into(),
        )
        .await
        .unwrap();
        assert!(restored.clone_history().await[0]
            .text()
            .contains("BATCH_DONE"));
        assert!(restored
            .build_system_prompt()
            .await
            .contains("CHECKPOINT_KEEP"));
    }

    #[tokio::test]
    async fn requested_compaction_failure_preserves_history_and_reports_status() {
        let dir = TempDir::new().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        let (session, _thread, _) = thread(&dir, "compaction-failure").await;
        session
            .sessions()
            .ensure_session(session.session_id(), "test")
            .await
            .unwrap();
        session
            .lock_state()
            .record_items([message("user", "Keep this request")]);
        session
            .sessions()
            .request_context_compaction(session.session_id(), "turn-1", "test")
            .await
            .unwrap();
        session.hook_bus().register(::hooks::PRE_COMPACT, |_| {
            ::hooks::HookOutcome::Block("test denied".into())
        });
        let before = session.clone_history().await;
        assert!(session
            .apply_requested_context_compaction("turn-1")
            .await
            .is_err());
        assert_eq!(session.clone_history().await, before);
        let status = session
            .sessions()
            .thread_context(session.session_id())
            .await
            .unwrap();
        assert!(status.compaction_status.starts_with("failed:"));
        assert!(!session
            .apply_requested_context_compaction("turn-1")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn history_tool_cannot_read_a_different_thread_reference() {
        let dir = TempDir::new().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        let (one, _thread_one, path_one) = thread(&dir, "history-one").await;
        let (two, _thread_two, path_two) = thread(&dir, "history-two").await;
        one.runtime_io
            .get()
            .unwrap()
            .rollout
            .record(vec![agent_rollout::RolloutItem::ResponseItem(message(
                "user",
                "first evidence",
            ))])
            .await
            .unwrap();
        two.runtime_io
            .get()
            .unwrap()
            .rollout
            .record(vec![agent_rollout::RolloutItem::ResponseItem(message(
                "user",
                "private second evidence",
            ))])
            .await
            .unwrap();
        let references = one
            .referenced_history(&[message("user", "first evidence")])
            .await
            .unwrap();
        let own_ref = references[0].as_ref().unwrap();
        let result = one
            .handle_tool_call_async(
                "history",
                &serde_json::json!({"action":"read_item","item_ref":own_ref}),
            )
            .await
            .unwrap();
        assert!(result.text().contains("first evidence"));
        let foreign = agent_rollout::history_reference(&path_two, 1);
        assert!(one
            .handle_tool_call_async(
                "history",
                &serde_json::json!({"action":"read_item","item_ref":foreign})
            )
            .await
            .is_err());
        assert_ne!(path_one, path_two);
    }

    fn message(role: &str, text: &str) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: role.into(),
            content: vec![ContentItem::InputText { text: text.into() }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
    }

    async fn thread(
        dir: &TempDir,
        session_id: &str,
    ) -> (Arc<Session>, Arc<AstroThread>, std::path::PathBuf) {
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                session_id.into(),
            )
            .await
            .unwrap(),
        );
        let path = agent_rollout::new_rollout_path(
            &dir.path().join("sessions").join("rollouts"),
            session_id,
            chrono::Utc::now(),
        );
        let thread = AstroThread::spawn(
            Arc::clone(&session),
            RolloutRecorder::open(path.clone()).await.unwrap(),
        )
        .unwrap();
        (session, thread, path)
    }

    #[tokio::test]
    async fn compaction_replacement_is_restored_as_canonical_history() {
        let dir = TempDir::new().unwrap();
        let (session, thread, _) = thread(&dir, "compact-history").await;
        session
            .record_response_items(vec![message("user", "old"), message("assistant", "answer")])
            .await
            .unwrap();
        session
            .replace_history_with_compaction_summary("durable summary")
            .await
            .unwrap();
        assert_eq!(session.clone_response_history().await.len(), 1);
        thread.submit(Op::Shutdown).await.unwrap();
        thread.wait_terminated().await;

        let restored = Session::with_session_id(
            Config::with_defaults(dir.path().to_path_buf()),
            "compact-history".into(),
        )
        .await
        .unwrap();
        let history = restored.clone_response_history().await;
        assert_eq!(history.len(), 1);
        assert!(matches!(
            &history[0],
            ResponseItem::Message { role, content, .. }
                if role == "developer"
                    && matches!(&content[0], ContentItem::InputText { text } if text.contains("durable summary"))
        ));
    }

    #[tokio::test]
    async fn rollback_is_cumulative_and_replayed_after_restart() {
        let dir = TempDir::new().unwrap();
        let (session, thread, _) = thread(&dir, "rollback-history").await;
        session
            .record_response_items(vec![
                message("user", "one"),
                message("assistant", "answer one"),
                message("user", "two"),
                message("assistant", "answer two"),
            ])
            .await
            .unwrap();

        thread
            .submit(Op::ThreadRollback { num_turns: 1 })
            .await
            .unwrap();
        let event = thread.next_event().await.unwrap();
        assert!(matches!(
            event.msg,
            EventMsg::ThreadRolledBack(ref rollback) if rollback.num_turns == 1
        ));
        assert_eq!(session.clone_response_history().await.len(), 2);

        thread
            .submit(Op::ThreadRollback { num_turns: 1 })
            .await
            .unwrap();
        let _ = thread.next_event().await.unwrap();
        assert!(session.clone_response_history().await.is_empty());
        thread.submit(Op::Shutdown).await.unwrap();
        thread.wait_terminated().await;

        let restored = Session::with_session_id(
            Config::with_defaults(dir.path().to_path_buf()),
            "rollback-history".into(),
        )
        .await
        .unwrap();
        assert!(restored.clone_response_history().await.is_empty());
    }

    #[tokio::test]
    async fn edited_input_rollback_replaces_memory_sqlite_and_restart_history() {
        let dir = TempDir::new().unwrap();
        let (session, thread, _) = thread(&dir, "edited-history").await;
        session
            .record_response_items(vec![
                message("user", "one"),
                message("assistant", "answer one"),
                message("user", "old two"),
                message("assistant", "old answer two"),
            ])
            .await
            .unwrap();

        session
            .rollback_thread_to_bubbles("edit".into(), 2)
            .await
            .unwrap();
        let event = thread.next_event().await.unwrap();
        assert!(matches!(
            event.msg,
            EventMsg::ThreadRolledBack(ref rollback)
                if rollback.num_turns == 1 && rollback.keep_chat_bubbles == Some(2)
        ));
        assert_eq!(session.clone_response_history().await.len(), 2);
        assert_eq!(
            session
                .sessions()
                .get_response_items(session.session_id())
                .await
                .unwrap()
                .len(),
            2
        );

        session
            .record_response_items(vec![
                message("user", "new two"),
                message("assistant", "new answer two"),
            ])
            .await
            .unwrap();
        thread.submit(Op::Shutdown).await.unwrap();
        thread.wait_terminated().await;

        let restored = Session::with_session_id(
            Config::with_defaults(dir.path().to_path_buf()),
            "edited-history".into(),
        )
        .await
        .unwrap();
        let serialized = serde_json::to_string(&restored.clone_response_history().await).unwrap();
        assert!(!serialized.contains("old two"));
        assert!(!serialized.contains("old answer two"));
        assert!(serialized.contains("new two"));
        assert!(serialized.contains("new answer two"));
    }
}
