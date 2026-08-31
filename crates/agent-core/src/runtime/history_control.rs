use agent_protocol::{
    ContentItem, ErrorEvent, Event, EventMsg, ResponseItem, ThreadRolledBackEvent,
};
use agent_rollout::RolloutItem;

use super::Session;

const COMPACTION_SUMMARY_MARK: &str = "[astro:compaction-summary]";

impl Session {
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
        self.lock_state().replace_history(replacement);
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
        let Some(bindings) = self.runtime_io.get() else {
            self.emit_rollback_error(
                submission_id,
                "thread rollback requires persisted thread history",
            )
            .await;
            return;
        };
        if let Err(error) = bindings.rollout.flush().await {
            self.emit_rollback_error(
                submission_id,
                format!("failed to flush thread persistence for rollback replay: {error}"),
            )
            .await;
            return;
        }
        let items = match agent_rollout::read_rollout(bindings.rollout.path()).await {
            Ok(items) => items,
            Err(error) => {
                self.emit_rollback_error(
                    submission_id,
                    format!("failed to load thread history for rollback replay: {error}"),
                )
                .await;
                return;
            }
        };
        let mut replacement = agent_rollout::effective_response_history(&items);
        agent_rollout::drop_last_n_user_turns(&mut replacement, num_turns);
        let event = EventMsg::ThreadRolledBack(ThreadRolledBackEvent { num_turns });
        if let Err(error) = bindings
            .rollout
            .record(vec![RolloutItem::EventMsg(event.clone())])
            .await
        {
            self.emit_rollback_error(
                submission_id,
                format!("failed to persist thread rollback: {error}"),
            )
            .await;
            return;
        }
        {
            let mut state = self.lock_state();
            state.replace_history(replacement);
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
}
