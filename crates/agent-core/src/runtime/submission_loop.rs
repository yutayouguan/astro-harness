use std::sync::Arc;

use agent_protocol::{ErrorEvent, EventMsg, ItemEvent, Op, Submission, TurnItem};
use async_channel::Receiver;
use futures::FutureExt;
use serde_json::{Map, Value};

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
            Op::RecoverTurn { turn_id, reply } => {
                let result = session.recover_turn(turn_id, chat_override.clone()).await;
                let _ = reply.send(result);
                false
            }
            Op::SuspendTurnAndShutdown { reply } => {
                let result = session
                    .suspend_active_regular_turn()
                    .await
                    .map_err(|error| agent_protocol::TurnInputError::Invalid(error.to_string()));
                let should_exit = matches!(
                    result,
                    Ok(agent_protocol::SuspendTurnOutcome::Suspended { .. })
                );
                if should_exit {
                    session.shutdown(submission.id).await;
                }
                let _ = reply.send(result);
                should_exit
            }
            Op::Interrupt => {
                if let Err(error) = session
                    .abort_all_tasks(agent_protocol::TurnAbortReason::Interrupted)
                    .await
                {
                    session
                        .send_event(
                            &submission.id,
                            EventMsg::Error(ErrorEvent {
                                message: error.to_string(),
                                error_type: "task_abort".into(),
                            }),
                        )
                        .await;
                }
                false
            }
            Op::EmitExtension { item, turn_id } => {
                session.record_extension(submission.id, item, turn_id).await;
                false
            }
            Op::Shutdown => {
                session.shutdown(submission.id).await;
                true
            }
            op => {
                session
                    .dispatch_control_op(submission.id, op, chat_override.clone())
                    .await;
                false
            }
        };
        if should_exit {
            shutdown_received = true;
            break;
        }
    }
    if !shutdown_received {
        session
            .shutdown(format!("{}:shutdown", session.session_id()))
            .await;
    }
}

impl Session {
    pub(crate) async fn shutdown(self: &Arc<Self>, submission_id: String) {
        if let Err(error) = self
            .abort_all_tasks(agent_protocol::TurnAbortReason::Interrupted)
            .await
        {
            tracing::warn!(%error, session_id = %self.session_id(), "failed to abort session task before shutdown");
        }
        self.shutdown_runtime().await;
        if let Some(bindings) = self.runtime_io.get() {
            if let Err(error) = bindings.rollout.shutdown().await {
                self.send_event_raw_with_persistence(
                    agent_protocol::Event {
                        id: submission_id.clone(),
                        msg: EventMsg::Error(ErrorEvent {
                            message: error.to_string(),
                            error_type: "rollout_shutdown".into(),
                        }),
                    },
                    false,
                )
                .await;
            }
        }
        self.deliver_event_raw(agent_protocol::Event {
            id: submission_id,
            msg: EventMsg::ShutdownComplete,
        })
        .await;
    }

    async fn record_extension(
        &self,
        submission_id: String,
        item: agent_protocol::ExtensionItem,
        target_turn_id: Option<String>,
    ) {
        let turn_id = match target_turn_id {
            Some(turn_id) => turn_id,
            None => self
                .active_turn
                .lock()
                .await
                .as_ref()
                .and_then(|turn| turn.task.as_ref())
                .map(|running| running.turn_context.sub_id().to_string())
                .unwrap_or_else(|| submission_id.clone()),
        };
        self.send_event(
            &turn_id,
            EventMsg::ItemCompleted(ItemEvent {
                turn_id: turn_id.clone(),
                item: TurnItem::Extension(item),
            }),
        )
        .await;
    }

    async fn dispatch_control_op(
        self: &Arc<Self>,
        submission_id: String,
        op: Op,
        chat_override: Option<ChatOverride>,
    ) {
        match op {
            Op::ThreadSettings { settings } => {
                self.send_event(&submission_id, EventMsg::ThreadSettingsApplied(settings))
                    .await;
            }
            Op::RefreshMcpServers => {
                if let Err(error) = self.reload_mcp().await {
                    self.send_event(
                        &submission_id,
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
                    self.send_event(
                        &submission_id,
                        EventMsg::Error(ErrorEvent {
                            message: error.to_string(),
                            error_type: "user_config_reload".into(),
                        }),
                    )
                    .await;
                }
            }
            Op::ExecApproval { id, decision } => {
                self.resolve_approval(submission_id, id, decision).await;
            }
            Op::PatchApproval { id, decision } => {
                self.resolve_approval(submission_id, id, decision).await;
            }
            Op::UserInputAnswer { id, response } => {
                self.resolve_control_response(submission_id, id, "resolved", response)
                    .await;
            }
            Op::RequestPermissionsResponse { id, response } => {
                self.resolve_control_response(submission_id, id, "resolved", response)
                    .await;
            }
            Op::DynamicToolResponse { id, response } => {
                self.resolve_control_response(submission_id, id, "resolved", response)
                    .await;
            }
            Op::Compact => {
                let context = self.create_turn_context(submission_id.clone()).await;
                if let Err(error) = self
                    .spawn_task(context, Vec::new(), crate::tasks::CompactTask)
                    .await
                {
                    self.send_event(
                        &submission_id,
                        EventMsg::Error(ErrorEvent {
                            message: error.to_string(),
                            error_type: "compact_failed".into(),
                        }),
                    )
                    .await;
                }
            }
            Op::ThreadRollback { num_turns } => {
                self.rollback_thread(submission_id, num_turns).await;
            }
            Op::Review { request } => {
                let context = self.create_turn_context(submission_id.clone()).await;
                let args = crate::streaming::multi_turn::RunTurnArgs::submitted(
                    Arc::clone(self),
                    Arc::clone(&context),
                    chat_override,
                );
                if let Err(error) = self
                    .spawn_task(
                        context,
                        Vec::new(),
                        crate::tasks::ReviewTask::new(args, request),
                    )
                    .await
                {
                    self.send_event(
                        &submission_id,
                        EventMsg::Error(ErrorEvent {
                            message: error.to_string(),
                            error_type: "review_failed".into(),
                        }),
                    )
                    .await;
                }
            }
            Op::InterAgentCommunication { .. } => {
                self.emit_unsupported_op(submission_id, "inter_agent_communication")
                    .await;
            }
            Op::TurnInput { .. }
            | Op::RecoverTurn { .. }
            | Op::SuspendTurnAndShutdown { .. }
            | Op::Interrupt
            | Op::EmitExtension { .. }
            | Op::Shutdown => {
                unreachable!("submission loop routes primary control operations directly")
            }
        }
    }

    async fn resolve_approval(&self, submission_id: String, id: String, decision: Value) {
        let (status, payload) = approval_resolution(decision);
        self.resolve_control_response(submission_id, id, status, payload)
            .await;
    }

    async fn resolve_control_response(
        &self,
        submission_id: String,
        request_id: String,
        status: &str,
        payload: Value,
    ) {
        let (_, gate, _) = self.ensure_thread_controls();
        let result = gate
            .resolve(&[crate::ResumeItem {
                interrupt_id: request_id,
                status: status.to_string(),
                payload_json: payload.to_string(),
            }])
            .await;
        if let Err(error) = result {
            self.send_event(
                &submission_id,
                EventMsg::Error(ErrorEvent {
                    message: error,
                    error_type: "control_response".into(),
                }),
            )
            .await;
        }
    }

    async fn emit_unsupported_op(&self, submission_id: String, operation: &str) {
        self.send_event(
            &submission_id,
            EventMsg::Error(ErrorEvent {
                message: format!("operation {operation} is not supported by this runtime"),
                error_type: "unsupported_op".into(),
            }),
        )
        .await;
    }

    pub async fn shutdown_runtime(self: &Arc<Self>) {
        if self.begin_runtime_shutdown() {
            let session = Arc::clone(self);
            tokio::spawn(async move {
                let result = std::panic::AssertUnwindSafe(session.run_shutdown_worker())
                    .catch_unwind()
                    .await;
                if result.is_err() {
                    tracing::error!(session_id = %session.session_id(), "session shutdown worker panicked");
                }
                session.complete_runtime_shutdown();
            });
        }
        self.wait_runtime_shutdown_complete().await;
    }

    async fn run_shutdown_worker(self: &Arc<Self>) {
        let (task_lifecycle, abort_result) = self
            .abort_all_tasks_for_shutdown(agent_protocol::TurnAbortReason::Interrupted)
            .await;
        if let Err(error) = abort_result {
            tracing::warn!(%error, session_id = %self.session_id(), "failed to abort session task during shutdown");
        }
        let turn_id = task_lifecycle.as_ref().map(|(turn_id, _)| turn_id.clone());
        if let Some((_, task_completion)) = task_lifecycle {
            task_completion.cancelled().await;
        }
        let _ = self.fire_hook(
            ::hooks::SESSION_END,
            ::hooks::HookPayload {
                session_id: self.session_id().to_string(),
                turn_id,
                reason: Some("other".into()),
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

fn approval_resolution(decision: Value) -> (&'static str, Value) {
    match decision {
        Value::String(decision) => approval_resolution_from_name(&decision, Map::new()),
        Value::Object(mut decision) => {
            if decision.get("approved").and_then(Value::as_bool).is_some() {
                return ("resolved", Value::Object(decision));
            }
            let name = decision
                .remove("decision")
                .and_then(|value| value.as_str().map(str::to_owned))
                .or_else(|| {
                    (decision.len() == 1)
                        .then(|| decision.keys().next().cloned())
                        .flatten()
                })
                .unwrap_or_else(|| "denied".into());
            approval_resolution_from_name(&name, decision)
        }
        _ => ("resolved", serde_json::json!({ "approved": false })),
    }
}

fn approval_resolution_from_name(
    decision: &str,
    mut metadata: Map<String, Value>,
) -> (&'static str, Value) {
    let normalized = decision.trim().to_ascii_lowercase();
    let (status, approved, always, abort) = match normalized.as_str() {
        "approved" | "approve" | "allow" | "allow_once" => ("resolved", true, false, false),
        "approved_for_session"
        | "approvedforsession"
        | "allow_always"
        | "approved_execpolicy_amendment"
        | "approved_mcp_policy_amendment"
        | "network_policy_amendment" => ("resolved", true, true, false),
        "timed_out" | "timeout" => ("timeout", false, false, false),
        "abort" | "cancelled" | "canceled" => ("cancelled", false, false, true),
        _ => ("resolved", false, false, false),
    };
    metadata.insert("approved".into(), Value::Bool(approved));
    if always {
        metadata.insert("always".into(), Value::Bool(true));
    }
    if abort {
        metadata.insert("abort".into(), Value::Bool(true));
    }
    (status, Value::Object(metadata))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use agent_protocol::{Op, Submission, TurnInput};
    use serde_json::json;
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
            .await
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

    #[tokio::test]
    async fn ordered_control_ops_resolve_the_session_hitl_gate() {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "submission-control-test".into(),
            )
            .await
            .unwrap(),
        );
        let (_, gate, _) = session.ensure_thread_controls();
        let approval = gate
            .begin_wait(crate::Interrupt {
                id: "approval-1".into(),
                response_schema_json: json!({
                    "type": "object",
                    "required": ["approved"],
                    "properties": { "approved": { "type": "boolean" } }
                })
                .to_string(),
                ..Default::default()
            })
            .await;
        let answer = gate
            .begin_wait(crate::Interrupt {
                id: "question-1".into(),
                ..Default::default()
            })
            .await;

        let (tx, rx) = async_channel::bounded(4);
        let loop_task = tokio::spawn(submission_loop(Arc::clone(&session), rx, None));
        tx.send(Submission {
            id: "approval-submission".into(),
            op: Op::ExecApproval {
                id: "approval-1".into(),
                decision: Value::String("approved_for_session".into()),
            },
        })
        .await
        .unwrap();
        let approval = approval.await.unwrap();
        assert_eq!(approval.status, "resolved");
        let payload: Value = serde_json::from_str(&approval.payload_json).unwrap();
        assert_eq!(payload["approved"], true);
        assert_eq!(payload["always"], true);

        tx.send(Submission {
            id: "answer-submission".into(),
            op: Op::UserInputAnswer {
                id: "question-1".into(),
                response: json!({ "answers": { "choice": ["yes"] } }),
            },
        })
        .await
        .unwrap();
        let answer = answer.await.unwrap();
        assert_eq!(answer.status, "resolved");
        assert_eq!(
            serde_json::from_str::<Value>(&answer.payload_json).unwrap(),
            json!({ "answers": { "choice": ["yes"] } })
        );

        drop(tx);
        loop_task.await.unwrap();
    }

    #[test]
    fn approval_resolution_accepts_codex_decision_names() {
        let (status, approved) = approval_resolution(Value::String("approved".into()));
        assert_eq!(status, "resolved");
        assert_eq!(approved, json!({ "approved": true }));

        let (status, denied) = approval_resolution(json!({
            "denied": { "rejection": "unsafe" }
        }));
        assert_eq!(status, "resolved");
        assert_eq!(denied["approved"], false);

        let (status, timeout) = approval_resolution(Value::String("timed_out".into()));
        assert_eq!(status, "timeout");
        assert_eq!(timeout["approved"], false);
    }
}
