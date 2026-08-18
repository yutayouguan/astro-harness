//! 后台（非 UI）运行适配器，供 Cron 和 Agent Thread 使用。
//!
//! 本模块不实现第二套 Agent 循环。foreground 与 background 都由
//! [`crate::streaming::run_multi_turn_stream`] 驱动；这里只负责：
//! - 丢弃 UI 专属事件并收集最终 assistant 文本与 usage；
//! - 将 Agent Thread 的 interrupt / close 信号桥接到 [`providers::PauseControl`]；
//! - 把统一引擎的流式错误转换为后台调用方可处理的 `Result`。
//!
//! 权限与执行表面解耦：后台运行经过与前台完全相同的 sandbox、approval、hooks、
//! tool execution、iteration budget 和 max-iteration summary，仅不向 UI 转发事件。

use std::sync::Arc;

use providers::{PauseControl, ProviderConfig, Usage};
use types::message::{MessageContent, Role};
use types::ChatTarget;

use crate::runtime::Session;
use crate::streaming::multi_turn::{
    install_multi_turn_task, InstalledMultiTurn, MultiTurnTaskArgs,
};
use crate::streaming::ChatOverride;
use agent_protocol::{Event, EventMsg, TurnInput};

#[derive(Debug)]
struct BackgroundCollected {
    usage: Usage,
    event_kinds: Vec<&'static str>,
    terminal_kind: &'static str,
}

/// 使用统一多轮引擎执行后台任务。
pub async fn run_background_multi_turn(
    session: Arc<Session>,
    targets: Vec<ChatTarget>,
    input: Vec<TurnInput>,
) -> anyhow::Result<(String, Usage)> {
    run_background_multi_turn_controlled(session, targets, input, None).await
}

/// 带 Agent Thread interrupt / close 控制的后台执行入口。
pub async fn run_background_multi_turn_controlled(
    session: Arc<Session>,
    targets: Vec<ChatTarget>,
    input: Vec<TurnInput>,
    control: Option<Arc<subagents::AgentThreadControl>>,
) -> anyhow::Result<(String, Usage)> {
    run_background_multi_turn_controlled_with_chat(session, targets, input, control, None).await
}

pub(crate) async fn run_background_multi_turn_controlled_with_chat(
    session: Arc<Session>,
    targets: Vec<ChatTarget>,
    input: Vec<TurnInput>,
    control: Option<Arc<subagents::AgentThreadControl>>,
    chat_override: Option<ChatOverride>,
) -> anyhow::Result<(String, Usage)> {
    let (base_config, message_start) = {
        let agent = session.as_ref();
        let history = agent.clone_history().await;
        (
            ProviderConfig {
                temperature: agent.temperature(),
                additional_params: agent.additional_params(),
                ..ProviderConfig::default()
            },
            history.len(),
        )
    };
    let pause = PauseControl::new();
    let cancellation_bridge = control.as_ref().map(|control| {
        let control = Arc::clone(control);
        let pause = Arc::clone(&pause);
        tokio::spawn(async move {
            control.cancelled().await;
            pause.cancel();
        })
    });
    let installed = install_multi_turn_task(MultiTurnTaskArgs {
        session: Arc::clone(&session),
        targets,
        base_config,
        input,
        system_prompt: None,
        pause,
        hitl_gate: None,
        chat_override,
    })
    .await;
    let InstalledMultiTurn {
        session: installed_session,
        turn_id,
        events,
        ..
    } = match installed {
        Ok(installed) => installed,
        Err(error) => {
            if let Some(bridge) = cancellation_bridge {
                bridge.abort();
            }
            anyhow::bail!(
                "background turn {} install failed: {}",
                error.turn_id,
                error.message
            );
        }
    };
    let engine = installed_session.wait_for_task(&turn_id);
    let collector = collect_background_events(events, &turn_id);
    let ((), collected) = tokio::join!(engine, collector);

    if let Some(bridge) = cancellation_bridge {
        bridge.abort();
    }
    if control
        .as_ref()
        .is_some_and(|value| value.is_interrupted() || value.is_closed())
    {
        anyhow::bail!("agent thread interrupted");
    }
    let collected = collected?;
    tracing::debug!(
        event_kinds = ?collected.event_kinds,
        terminal_kind = collected.terminal_kind,
        "background turn collected unified events"
    );
    let usage = collected.usage;
    let output = latest_assistant_text(&session, message_start)
        .await
        .filter(|text| !text.is_empty())
        .ok_or_else(|| anyhow::anyhow!("模型未返回有效回复"))?;
    Ok((output, usage))
}

async fn collect_background_events(
    rx: async_channel::Receiver<Event>,
    turn_id: &str,
) -> anyhow::Result<BackgroundCollected> {
    let mut usage = Usage::default();
    let mut selected_turn =
        (!turn_id.is_empty()).then(|| crate::runtime::event_identity::event_turn_id(turn_id));
    let mut event_kinds = Vec::new();
    let mut stream_error = None;
    while let Ok(event) = rx.recv().await {
        if selected_turn.is_none() && matches!(event.msg, EventMsg::TurnStarted(_)) {
            selected_turn = Some(event.id.clone());
        }
        if selected_turn.as_deref() != Some(event.id.as_str()) {
            continue;
        }
        match event.msg {
            EventMsg::ItemStarted(_) => event_kinds.push("item_started"),
            EventMsg::ItemCompleted(_) => event_kinds.push("item_completed"),
            EventMsg::TokenCount(tokens) => {
                usage.input_tokens = u32::try_from(tokens.input_tokens).unwrap_or(u32::MAX);
                usage.output_tokens = u32::try_from(tokens.output_tokens).unwrap_or(u32::MAX);
                usage.cache_read_tokens =
                    u32::try_from(tokens.cache_read_tokens).unwrap_or(u32::MAX);
                usage.cache_write_tokens =
                    u32::try_from(tokens.cache_write_tokens).unwrap_or(u32::MAX);
                usage.reasoning_tokens = u32::try_from(tokens.reasoning_tokens).unwrap_or(u32::MAX);
                usage.request_count = u32::try_from(tokens.request_count).unwrap_or(u32::MAX);
            }
            EventMsg::Error(error) | EventMsg::StreamError(error) => {
                stream_error = Some(error.message)
            }
            EventMsg::TurnComplete(complete) => {
                if let Some(error) = complete.error {
                    anyhow::bail!(error.message);
                }
                if let Some(error) = stream_error {
                    anyhow::bail!(error);
                }
                return Ok(BackgroundCollected {
                    usage,
                    event_kinds,
                    terminal_kind: "turn_complete",
                });
            }
            EventMsg::TurnAborted(event) => {
                anyhow::bail!("background turn aborted: {:?}", event.reason);
            }
            EventMsg::Warning(_)
            | EventMsg::TurnStarted(_)
            | EventMsg::AgentMessageContentDelta(_)
            | EventMsg::PlanDelta(_)
            | EventMsg::ReasoningContentDelta(_)
            | EventMsg::ExecCommandOutputDelta(_)
            | EventMsg::PatchApplyUpdated(_)
            | EventMsg::ExecApprovalRequest(_)
            | EventMsg::ApplyPatchApprovalRequest(_)
            | EventMsg::RequestPermissions(_)
            | EventMsg::RequestUserInput(_)
            | EventMsg::ElicitationRequest(_)
            | EventMsg::DynamicToolCallRequest(_)
            | EventMsg::DynamicToolCallResponse(_)
            | EventMsg::McpToolCallBegin(_)
            | EventMsg::McpToolCallEnd(_)
            | EventMsg::HookStarted(_)
            | EventMsg::HookCompleted(_)
            | EventMsg::SubAgentActivity(_)
            | EventMsg::ContextCompacted(_)
            | EventMsg::ContextUsage(_)
            | EventMsg::LegacyUserMessage(_)
            | EventMsg::LegacyAgentMessage(_)
            | EventMsg::LegacyReasoning(_)
            | EventMsg::LegacyMcpToolCallEnd(_)
            | EventMsg::LegacyPatchApplyEnd(_)
            | EventMsg::LegacyContextCompacted(_)
            | EventMsg::LegacySubAgentActivity(_)
            | EventMsg::ThreadSettingsApplied(_)
            | EventMsg::ThreadRolledBack(_)
            | EventMsg::ShutdownComplete => {}
        }
    }
    anyhow::bail!("background event stream closed before terminal event")
}

async fn latest_assistant_text(session: &Arc<Session>, message_start: usize) -> Option<String> {
    let history = session.clone_history().await;
    history[message_start..]
        .iter()
        .rev()
        .find(|message| message.role == Role::Assistant)
        .map(|message| match &message.content {
            MessageContent::Text(text) => text.clone(),
            MessageContent::Parts(parts) => parts
                .iter()
                .filter_map(|part| part.text.as_deref())
                .collect::<Vec<_>>()
                .join("\n"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::{run_multi_turn_stream_with_chat_fn_legacy, MultiTurnStreamItem};
    use agent_protocol::{
        ErrorEvent, Event, EventMsg, ItemEvent, TokenCountEvent, TurnCompleteEvent,
    };
    use agent_protocol::{ToolItem, ToolStatus, TurnItem};

    fn pending_chat() -> ChatOverride {
        Arc::new(move |_messages, _tools, _config| {
            Box::pin(async move {
                Ok(Box::pin(futures::stream::pending()) as providers::CompletionStream)
            })
        })
    }

    fn completed_chat() -> ChatOverride {
        Arc::new(move |_messages, _tools, _config| {
            Box::pin(async move {
                Ok(Box::pin(futures::stream::iter(vec![
                    Ok(providers::types::stream::StreamChunk::Text("done".into())),
                    Ok(providers::types::stream::StreamChunk::Done {
                        finish_reason: "stop".into(),
                    }),
                ])) as providers::CompletionStream)
            })
        })
    }

    fn test_target() -> ChatTarget {
        ChatTarget {
            provider_id: "test".into(),
            backend_id: "openai".into(),
            model: "test".into(),
            api_key: "test".into(),
            base_url: "http://127.0.0.1.invalid".into(),
        }
    }

    #[tokio::test]
    async fn background_collector_observes_tool_and_terminal_events() {
        let (tx, rx) = async_channel::unbounded();
        let item = TurnItem::CommandExecution(ToolItem {
            id: "call-1".into(),
            name: "terminal".into(),
            arguments: serde_json::json!({"command": "pwd"}),
            output: None,
            media: Vec::new(),
            status: ToolStatus::InProgress,
        });
        for msg in [
            EventMsg::ItemStarted(ItemEvent {
                turn_id: "turn-1".into(),
                item: item.clone(),
            }),
            EventMsg::ItemCompleted(ItemEvent {
                turn_id: "turn-1".into(),
                item,
            }),
            EventMsg::TurnComplete(TurnCompleteEvent {
                turn_id: "turn-1".into(),
                last_agent_message: Some("done".into()),
                error: None,
            }),
        ] {
            tx.send(Event {
                id: "turn-1".into(),
                msg,
            })
            .await
            .unwrap();
        }
        drop(tx);
        let result = collect_background_events(rx, "turn-1").await.unwrap();
        assert!(result.event_kinds.contains(&"item_started"));
        assert!(result.event_kinds.contains(&"item_completed"));
        assert_eq!(result.terminal_kind, "turn_complete");
    }

    #[tokio::test]
    async fn collector_returns_error_even_when_done_follows() {
        let (tx, rx) = async_channel::unbounded();
        let error = ErrorEvent {
            message: "blocked".into(),
            error_type: "internal".into(),
        };
        tx.send(Event {
            id: "turn-1".into(),
            msg: EventMsg::Error(error.clone()),
        })
        .await
        .unwrap();
        tx.send(Event {
            id: "turn-1".into(),
            msg: EventMsg::TurnComplete(TurnCompleteEvent {
                turn_id: "turn-1".into(),
                last_agent_message: None,
                error: Some(error),
            }),
        })
        .await
        .unwrap();
        drop(tx);

        assert_eq!(
            collect_background_events(rx, "turn-1")
                .await
                .unwrap_err()
                .to_string(),
            "blocked"
        );
    }

    #[tokio::test]
    async fn collector_uses_final_aggregate_usage() {
        let (tx, rx) = async_channel::unbounded();
        tx.send(Event {
            id: "turn-1".into(),
            msg: EventMsg::TokenCount(TokenCountEvent {
                turn_id: Some("turn-1".into()),
                input_tokens: 12,
                output_tokens: 3,
                total_tokens: 15,
                cache_read_tokens: 4,
                cache_write_tokens: 2,
                reasoning_tokens: 1,
                request_count: 2,
            }),
        })
        .await
        .unwrap();
        tx.send(Event {
            id: "turn-1".into(),
            msg: EventMsg::TurnComplete(TurnCompleteEvent {
                turn_id: "turn-1".into(),
                last_agent_message: Some("done".into()),
                error: None,
            }),
        })
        .await
        .unwrap();
        drop(tx);

        let usage = collect_background_events(rx, "turn-1").await.unwrap().usage;
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 3);
        assert_eq!(usage.cache_read_tokens, 4);
    }

    #[tokio::test]
    async fn agent_thread_interrupt_cancels_unified_engine() {
        let temp = tempfile::tempdir().unwrap();
        let config = crate::runtime::Config::with_defaults(temp.path().to_path_buf());
        let agent = Session::with_session_id(config, "background-cancel".into()).unwrap();
        let session = Arc::new(agent);
        let control = Arc::new(subagents::AgentThreadControl::default());
        let target = ChatTarget {
            provider_id: "test".into(),
            backend_id: "openai".into(),
            model: "test".into(),
            api_key: "test".into(),
            base_url: "http://127.0.0.1.invalid".into(),
        };

        let run = run_background_multi_turn_controlled_with_chat(
            session,
            vec![target],
            vec![TurnInput {
                content: "wait".into(),
                image_data_urls: Vec::new(),
            }],
            Some(Arc::clone(&control)),
            Some(pending_chat()),
        );
        let interrupt = async {
            tokio::task::yield_now().await;
            control.interrupt();
        };
        let (result, ()) = tokio::join!(run, interrupt);

        assert_eq!(result.unwrap_err().to_string(), "agent thread interrupted");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn replacement_collects_only_the_new_background_turn() {
        let temp = tempfile::tempdir().unwrap();
        let config = crate::runtime::Config::with_defaults(temp.path().to_path_buf());
        let session =
            Arc::new(Session::with_session_id(config, "background-replace".into()).unwrap());
        let (legacy_tx, mut legacy_rx) = tokio::sync::mpsc::channel(8);
        let old_run = tokio::spawn(run_multi_turn_stream_with_chat_fn_legacy(
            Arc::clone(&session),
            pending_chat(),
            ProviderConfig::default(),
            "system".into(),
            PauseControl::new(),
            None,
            legacy_tx,
        ));
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if matches!(
                    legacy_rx.recv().await,
                    Some(Ok(MultiTurnStreamItem::RunStarted { .. }))
                ) {
                    break;
                }
            }
        })
        .await
        .expect("old turn must start");

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_background_multi_turn_controlled_with_chat(
                Arc::clone(&session),
                vec![test_target()],
                vec![TurnInput {
                    content: "replace old turn".into(),
                    image_data_urls: Vec::new(),
                }],
                None,
                Some(completed_chat()),
            ),
        )
        .await
        .expect("background replacement must not hang")
        .expect("new background turn must complete");
        assert_eq!(result.0, "done");
        tokio::time::timeout(std::time::Duration::from_secs(1), old_run)
            .await
            .expect("old run must observe replacement")
            .unwrap();
    }

    #[tokio::test]
    async fn runtime_shutdown_install_failure_returns_without_hanging() {
        let temp = tempfile::tempdir().unwrap();
        let config = crate::runtime::Config::with_defaults(temp.path().to_path_buf());
        let session =
            Arc::new(Session::with_session_id(config, "background-shutdown".into()).unwrap());
        session.begin_runtime_shutdown();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            run_background_multi_turn_controlled_with_chat(
                session,
                vec![test_target()],
                vec![TurnInput {
                    content: "must reject".into(),
                    image_data_urls: Vec::new(),
                }],
                None,
                Some(completed_chat()),
            ),
        )
        .await
        .expect("install failure must return without hanging")
        .expect_err("runtime shutdown must reject the turn");
        assert!(result.to_string().contains("shutting down"));
    }

    #[tokio::test]
    async fn install_failure_is_structured_without_legacy_channel() {
        let temp = tempfile::tempdir().unwrap();
        let config = crate::runtime::Config::with_defaults(temp.path().to_path_buf());
        let session =
            Arc::new(Session::with_session_id(config, "structured-install-error".into()).unwrap());
        session.begin_runtime_shutdown();

        let error = match install_multi_turn_task(MultiTurnTaskArgs {
            session,
            targets: vec![test_target()],
            base_config: ProviderConfig::default(),
            input: vec![TurnInput {
                content: "must reject".into(),
                image_data_urls: Vec::new(),
            }],
            system_prompt: None,
            pause: PauseControl::new(),
            hitl_gate: None,
            chat_override: Some(completed_chat()),
        })
        .await
        {
            Ok(_) => panic!("runtime shutdown must reject the turn"),
            Err(error) => error,
        };

        assert!(!error.turn_id.is_empty());
        assert!(error.message.contains("shutting down"));
    }
}
