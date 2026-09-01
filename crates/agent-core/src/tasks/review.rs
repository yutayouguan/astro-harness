use std::sync::Arc;

use agent_protocol::{
    Event, EventMsg, ItemEvent, ReviewRequest, ReviewTarget, TextItem, TurnItem, TurnStartedEvent,
};
use agent_rollout::RolloutRecorder;
use tokio_util::sync::CancellationToken;

use crate::runtime::{AgentStatus, Session, TurnContext};
use crate::streaming::multi_turn::{run_turn, RunTurnArgs};

use super::{SessionTask, SessionTaskResult, TaskKind, TurnInput};

pub(crate) struct ReviewTask {
    args: RunTurnArgs,
    request: ReviewRequest,
}

const REVIEW_SYSTEM_PROMPT: &str = r#"You are a code reviewer. Inspect the requested change and report only actionable correctness, security, performance, and regression findings. Prioritize findings by severity, cite precise files and lines when possible, and explain the concrete failure mode. Do not modify files, create commits, delegate work, browse the web, or ask the user questions. If there are no findings, say so plainly."#;

const REVIEW_BLOCKED_TOOLS: &[&str] = &[
    "spawn_agent",
    "list_agents",
    "read_agent",
    "send_message_to_agent",
    "send_message",
    "followup_task",
    "wait_agents",
    "wait_agent",
    "interrupt_agent",
    "close_agent",
    "web_search",
    "web_fetch",
    "browser_open",
    "browser_click",
    "browser_type",
    "browser_scroll",
    "browser_wait",
    "browser_snapshot",
    "browser_screenshot",
    "browser_close",
    "view_image",
    "ask_user",
    "send_user_message_async",
    "switch_mode",
];

impl ReviewTask {
    pub(crate) fn new(args: RunTurnArgs, request: ReviewRequest) -> Self {
        Self { args, request }
    }

    fn prompt(&self) -> String {
        let target = match &self.request.target {
            ReviewTarget::UncommittedChanges => "Review the working tree changes.".to_string(),
            ReviewTarget::BaseBranch { branch } => {
                format!("Review the changes against base branch `{branch}`.")
            }
            ReviewTarget::Commit { sha, title } => match title {
                Some(title) => format!("Review commit `{sha}` ({title})."),
                None => format!("Review commit `{sha}`."),
            },
            ReviewTarget::Custom { instructions } => instructions.clone(),
        };
        match self.request.user_facing_hint.as_deref() {
            Some(hint) if !hint.trim().is_empty() => {
                format!("{target}\n\nAdditional context: {hint}")
            }
            _ => target,
        }
    }

    async fn exit_review_mode(
        session: &Session,
        ctx: &TurnContext,
        mode_item_id: String,
        content: String,
    ) {
        session
            .send_event(
                ctx.sub_id(),
                EventMsg::ItemCompleted(ItemEvent {
                    turn_id: ctx.sub_id().to_string(),
                    item: TurnItem::ExitedReviewMode(TextItem {
                        id: mode_item_id,
                        content,
                    }),
                }),
            )
            .await;
    }

    async fn record_review_result(
        session: &Session,
        request: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let history = session.clone_history().await;
        if !history
            .last()
            .is_some_and(|message| message.role == types::message::Role::User)
        {
            session
                .record_user_message(&format!("[astro:review]\n{request}"))
                .await?;
        }
        session.record_assistant_message(content).await
    }

    fn should_forward_child_event(event: &Event) -> bool {
        match &event.msg {
            EventMsg::TurnStarted(_)
            | EventMsg::TurnComplete(_)
            | EventMsg::TurnAborted(_)
            | EventMsg::ShutdownComplete
            | EventMsg::UserInputCommitted(_)
            | EventMsg::AgentMessageContentDelta(_) => false,
            EventMsg::ItemStarted(item) | EventMsg::ItemCompleted(item) => {
                !matches!(item.item, TurnItem::AgentMessage(_))
            }
            _ => true,
        }
    }

    async fn run_isolated(
        &self,
        parent: Arc<Session>,
        parent_ctx: Arc<TurnContext>,
        prompt_text: String,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        let child_session_id = format!("{}::review::{}", parent.session_id(), parent_ctx.sub_id());
        let mut child = parent
            .isolated_review_session(child_session_id, REVIEW_SYSTEM_PROMPT.into())
            .await?;
        {
            let registry = child.tool_registry_mut();
            for name in REVIEW_BLOCKED_TOOLS {
                registry.unregister(name);
            }
        }
        let child = Arc::new(child);
        let child_turn = Arc::new(crate::runtime::TurnContext::new_with_roots(
            format!("{}::worker", parent_ctx.sub_id()),
            1,
            types::InteractionMode::Agent,
            Some(types::READ_ONLY_PROFILE.into()),
            parent_ctx.project_root().map(ToOwned::to_owned),
            parent_ctx.workspace_roots().to_vec(),
        ));
        child.bind_turn_context(Arc::clone(&child_turn)).await;

        let (event_tx, event_rx) = async_channel::unbounded();
        let (status_tx, _status_rx) = tokio::sync::watch::channel(AgentStatus::Idle);
        child
            .bind_runtime_io(event_tx, status_tx, RolloutRecorder::sink())
            .map_err(anyhow::Error::from)?;
        let forwarding_parent = Arc::clone(&parent);
        let forwarding_turn_id = parent_ctx.sub_id().to_string();
        let forwarder = tokio::spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                if Self::should_forward_child_event(&event) {
                    forwarding_parent
                        .send_event(&forwarding_turn_id, event.msg)
                        .await;
                }
            }
        });

        child.begin_user_turn().await;
        child
            .record_turn_input(TurnInput {
                content: prompt_text,
                image_data_urls: Vec::new(),
                client_message_id: None,
            })
            .await?;
        child.increment_turn().await;
        let prompt = child.build_prompt_contract().await;
        child_turn.close_input_admission();
        let args = self
            .args
            .for_isolated_review(Arc::clone(&child), Arc::clone(&child_turn))
            .with_prompt(prompt);
        let result = run_turn(args, cancellation_token).await;
        let final_message = if result.is_ok() {
            child
                .clone_history()
                .await
                .into_iter()
                .rev()
                .find(|message| message.role == types::message::Role::Assistant)
                .map(|message| message.content_text())
        } else {
            None
        };
        child.close_event_stream();
        let _ = forwarder.await;
        child.clear_current_turn_id().await;
        result.map(|_| final_message)
    }
}

impl SessionTask for ReviewTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Review
    }

    fn span_name(&self) -> &'static str {
        "session_task.review"
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        anyhow::ensure!(input.is_empty(), "review task owns its generated input");
        session
            .send_event(
                ctx.sub_id(),
                EventMsg::TurnStarted(TurnStartedEvent {
                    turn_id: ctx.sub_id().to_string(),
                }),
            )
            .await;
        let mode_item_id = format!("review-mode-{}", ctx.sub_id());
        session
            .send_event(
                ctx.sub_id(),
                EventMsg::ItemCompleted(ItemEvent {
                    turn_id: ctx.sub_id().to_string(),
                    item: TurnItem::EnteredReviewMode(TextItem {
                        id: mode_item_id.clone(),
                        content: self.prompt(),
                    }),
                }),
            )
            .await;
        let prompt_text = self.prompt();
        let result = self
            .run_isolated(
                Arc::clone(&session),
                Arc::clone(&ctx),
                prompt_text.clone(),
                cancellation_token.clone(),
            )
            .await;
        if !cancellation_token.is_cancelled() {
            let content = result
                .as_ref()
                .ok()
                .and_then(|message| message.clone())
                .unwrap_or_else(|| "Review failed before producing a result.".into());
            Self::exit_review_mode(
                session.as_ref(),
                ctx.as_ref(),
                mode_item_id,
                content.clone(),
            )
            .await;
            if result.is_ok() {
                Self::record_review_result(session.as_ref(), &prompt_text, &content).await?;
            }
        }
        let error = result.as_ref().err().map(ToString::to_string);
        let turn = session.session_turn().await;
        let _ = session.fire_hook(
            ::hooks::AGENT_END,
            ::hooks::HookPayload {
                session_id: session.session_id().to_string(),
                turn_id: Some(ctx.sub_id().to_string()),
                turn: Some(turn),
                error,
                detail: format!("review_turn={turn}"),
                ..Default::default()
            },
        );
        result
    }

    async fn abort(&self, session: Arc<Session>, ctx: Arc<TurnContext>) {
        let content = "Review was interrupted. Please run the review again.";
        Self::exit_review_mode(
            session.as_ref(),
            ctx.as_ref(),
            format!("review-mode-{}", ctx.sub_id()),
            content.into(),
        )
        .await;
        if let Err(error) =
            Self::record_review_result(session.as_ref(), &self.prompt(), content).await
        {
            tracing::warn!(%error, "failed to record interrupted review result");
        }
    }
}
