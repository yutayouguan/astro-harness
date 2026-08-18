use std::sync::Arc;

use agent_protocol::{ErrorEvent, EventMsg, ItemEvent, Op, Submission, TurnItem};
use async_channel::Receiver;

use super::Session;
use crate::streaming::ChatOverride;

pub(crate) async fn submission_loop(
    session: Arc<Session>,
    rx_sub: Receiver<Submission>,
    chat_override: Option<ChatOverride>,
) {
    let mut shutdown_received = false;
    while let Ok(submission) = rx_sub.recv().await {
        let should_exit = match submission.op {
            Op::TurnInput {
                request,
                mode,
                reply,
            } => {
                let result = session
                    .submit_turn_input(submission.id, request, mode, chat_override.clone())
                    .await;
                let _ = reply.send(result);
                false
            }
            Op::Interrupt => {
                let _ = session
                    .abort_all_tasks(agent_protocol::TurnAbortReason::Interrupted)
                    .await;
                false
            }
            Op::EmitExtension { item } => {
                session.record_extension(submission.id, item).await;
                false
            }
            Op::Shutdown => {
                session.shutdown_runtime().await;
                true
            }
            op => {
                session.dispatch_control_op(submission.id, op).await;
                false
            }
        };
        if should_exit {
            shutdown_received = true;
            break;
        }
    }
    if !shutdown_received {
        session.shutdown_runtime().await;
    }
}

impl Session {
    async fn record_extension(&self, submission_id: String, item: agent_protocol::ExtensionItem) {
        let turn_id = self
            .active_turn
            .lock()
            .await
            .as_ref()
            .and_then(|turn| turn.task.as_ref())
            .map(|running| running.turn_context.sub_id().to_string())
            .unwrap_or_else(|| submission_id.clone());
        self.emit_runtime_event(
            submission_id,
            EventMsg::ItemCompleted(ItemEvent {
                turn_id,
                item: TurnItem::Extension(item),
            }),
        )
        .await;
    }

    async fn dispatch_control_op(&self, submission_id: String, op: Op) {
        match op {
            Op::ThreadSettings { .. } => {
                self.emit_unsupported_op(submission_id, "thread_settings")
                    .await;
            }
            Op::RefreshMcpServers => {
                if let Err(error) = self.reload_mcp().await {
                    self.emit_runtime_event(
                        submission_id,
                        EventMsg::Error(ErrorEvent {
                            message: error.to_string(),
                            error_type: "mcp_refresh".into(),
                        }),
                    )
                    .await;
                }
            }
            Op::ReloadUserConfig => {
                if let Err(error) = self.reload_tools_and_mcp().await {
                    self.emit_runtime_event(
                        submission_id,
                        EventMsg::Error(ErrorEvent {
                            message: error.to_string(),
                            error_type: "user_config_reload".into(),
                        }),
                    )
                    .await;
                }
            }
            Op::ExecApproval { .. } => {
                self.emit_unsupported_op(submission_id, "exec_approval")
                    .await;
            }
            Op::PatchApproval { .. } => {
                self.emit_unsupported_op(submission_id, "patch_approval")
                    .await;
            }
            Op::UserInputAnswer { .. } => {
                self.emit_unsupported_op(submission_id, "user_input_answer")
                    .await;
            }
            Op::RequestPermissionsResponse { .. } => {
                self.emit_unsupported_op(submission_id, "request_permissions_response")
                    .await;
            }
            Op::DynamicToolResponse { .. } => {
                self.emit_unsupported_op(submission_id, "dynamic_tool_response")
                    .await;
            }
            Op::Compact => self.emit_unsupported_op(submission_id, "compact").await,
            Op::ThreadRollback { .. } => {
                self.emit_unsupported_op(submission_id, "thread_rollback")
                    .await;
            }
            Op::Review { .. } => self.emit_unsupported_op(submission_id, "review").await,
            Op::InterAgentCommunication { .. } => {
                self.emit_unsupported_op(submission_id, "inter_agent_communication")
                    .await;
            }
            Op::TurnInput { .. } | Op::Interrupt | Op::EmitExtension { .. } | Op::Shutdown => {
                unreachable!("submission loop routes primary control operations directly")
            }
        }
    }

    async fn emit_unsupported_op(&self, submission_id: String, operation: &str) {
        self.emit_runtime_event(
            submission_id,
            EventMsg::Error(ErrorEvent {
                message: format!("operation {operation} is not supported by this runtime"),
                error_type: "unsupported_op".into(),
            }),
        )
        .await;
    }

    pub async fn shutdown_runtime(self: &Arc<Self>) {
        if !self.begin_runtime_shutdown() {
            return;
        }
        if let Err(error) = self
            .abort_all_tasks(agent_protocol::TurnAbortReason::Interrupted)
            .await
        {
            tracing::warn!(%error, session_id = %self.session_id(), "failed to abort session task during shutdown");
        }
        let turn_id = self.current_turn_id().await;
        let _ = self.fire_hook(
            ::hooks::ON_SESSION_FINALIZE,
            ::hooks::HookPayload {
                session_id: self.session_id().to_string(),
                turn_id,
                detail: format!("session={}", self.session_id()),
                ..Default::default()
            },
        );
        if let Err(error) = self
            .mcp_hub
            .lock()
            .await
            .reload_with_configs(Vec::new())
            .await
        {
            tracing::warn!(%error, session_id = %self.session_id(), "failed to shut down MCP connections");
        }
        let stopped_jobs = tools::shutdown_background_jobs_for_session(self.session_id());
        tracing::debug!(stopped_jobs, session_id = %self.session_id(), "session runtime shutdown complete");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use agent_protocol::{Op, Submission, TurnInput};
    use tokio::sync::Notify;
    use tokio_util::sync::CancellationToken;

    use crate::runtime::{Config, TurnContext};
    use crate::tasks::{SessionTask, SessionTaskResult, TaskKind};

    struct PendingTask {
        started: Arc<Notify>,
    }

    impl SessionTask for PendingTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.submission_loop_test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            self.started.notify_one();
            cancellation_token.cancelled().await;
            Ok(None)
        }
    }

    #[tokio::test]
    async fn interrupt_is_dispatched_while_a_turn_task_is_running() {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "submission-loop-test".into(),
            )
            .unwrap(),
        );
        let started = Arc::new(Notify::new());
        let context = session.create_turn_context("turn-1".into()).await;
        session
            .spawn_task(
                context,
                Vec::new(),
                PendingTask {
                    started: Arc::clone(&started),
                },
            )
            .await
            .unwrap();
        started.notified().await;

        let (tx, rx) = async_channel::bounded(4);
        let loop_task = tokio::spawn(submission_loop(Arc::clone(&session), rx, None));
        tx.send(Submission {
            id: "interrupt-1".into(),
            op: Op::Interrupt,
        })
        .await
        .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if session.active_turn.lock().await.is_none() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(tx);
        loop_task.await.unwrap();
    }
}
