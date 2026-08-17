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
use tokio::sync::{mpsc, Mutex};
use types::message::{MessageContent, Role};
use types::ChatTarget;

use crate::runtime::Session;
use crate::streaming::{
    run_multi_turn_stream, ChatOverride, MultiTurnStreamArgs, MultiTurnStreamItem,
    StreamedAssistantContent,
};

/// 使用统一多轮引擎执行后台任务。
pub async fn run_background_multi_turn(
    session: Arc<Mutex<Session>>,
    targets: Vec<ChatTarget>,
    system_prompt: String,
) -> anyhow::Result<(String, Usage)> {
    run_background_multi_turn_controlled(session, targets, system_prompt, None).await
}

/// 带 Agent Thread interrupt / close 控制的后台执行入口。
pub async fn run_background_multi_turn_controlled(
    session: Arc<Mutex<Session>>,
    targets: Vec<ChatTarget>,
    system_prompt: String,
    control: Option<Arc<subagents::AgentThreadControl>>,
) -> anyhow::Result<(String, Usage)> {
    run_background_multi_turn_controlled_with_chat(session, targets, system_prompt, control, None)
        .await
}

pub(crate) async fn run_background_multi_turn_controlled_with_chat(
    session: Arc<Mutex<Session>>,
    targets: Vec<ChatTarget>,
    system_prompt: String,
    control: Option<Arc<subagents::AgentThreadControl>>,
    chat_override: Option<ChatOverride>,
) -> anyhow::Result<(String, Usage)> {
    let (base_config, message_start) = {
        let agent = session.lock().await;
        (
            ProviderConfig {
                temperature: agent.temperature(),
                additional_params: agent.additional_params().clone(),
                ..ProviderConfig::default()
            },
            agent.session_messages.len(),
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
    let (tx, rx) = mpsc::channel(32);

    let engine = run_multi_turn_stream(MultiTurnStreamArgs {
        session: Arc::clone(&session),
        targets,
        base_config,
        system_prompt,
        pause,
        hitl_gate: None,
        tx,
        chat_override,
    });
    let collector = collect_background_events(rx);
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
    let usage = collected?;
    let output = latest_assistant_text(&session, message_start)
        .await
        .filter(|text| !text.is_empty())
        .ok_or_else(|| anyhow::anyhow!("模型未返回有效回复"))?;
    Ok((output, usage))
}

async fn collect_background_events(
    mut rx: mpsc::Receiver<anyhow::Result<MultiTurnStreamItem>>,
) -> anyhow::Result<Usage> {
    let mut usage = Usage::default();
    let mut stream_error = None;
    while let Some(item) = rx.recv().await {
        match item? {
            MultiTurnStreamItem::Assistant(StreamedAssistantContent::FinalUsage(value)) => {
                usage = value;
            }
            MultiTurnStreamItem::Error(message) => stream_error = Some(message),
            MultiTurnStreamItem::Done => break,
            MultiTurnStreamItem::Assistant(_)
            | MultiTurnStreamItem::ToolStarted { .. }
            | MultiTurnStreamItem::ToolResult { .. }
            | MultiTurnStreamItem::MemoryUpdate { .. }
            | MultiTurnStreamItem::ContextUsage(_)
            | MultiTurnStreamItem::RunStarted { .. }
            | MultiTurnStreamItem::Activity { .. }
            | MultiTurnStreamItem::RunFinished { .. } => {}
        }
    }
    if let Some(error) = stream_error {
        anyhow::bail!(error);
    }
    Ok(usage)
}

async fn latest_assistant_text(
    session: &Arc<Mutex<Session>>,
    message_start: usize,
) -> Option<String> {
    let agent = session.lock().await;
    agent.session_messages[message_start..]
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

    fn pending_chat() -> ChatOverride {
        Arc::new(move |_messages, _tools, _config| {
            Box::pin(async move {
                Ok(Box::pin(futures::stream::pending()) as providers::CompletionStream)
            })
        })
    }

    #[tokio::test]
    async fn collector_returns_error_even_when_done_follows() {
        let (tx, rx) = mpsc::channel(4);
        tx.send(Ok(MultiTurnStreamItem::Error("blocked".into())))
            .await
            .unwrap();
        tx.send(Ok(MultiTurnStreamItem::Done)).await.unwrap();
        drop(tx);

        assert_eq!(
            collect_background_events(rx).await.unwrap_err().to_string(),
            "blocked"
        );
    }

    #[tokio::test]
    async fn collector_uses_final_aggregate_usage() {
        let (tx, rx) = mpsc::channel(4);
        tx.send(Ok(MultiTurnStreamItem::Assistant(
            StreamedAssistantContent::FinalUsage(Usage {
                input_tokens: 12,
                output_tokens: 3,
                ..Usage::default()
            }),
        )))
        .await
        .unwrap();
        tx.send(Ok(MultiTurnStreamItem::Done)).await.unwrap();
        drop(tx);

        let usage = collect_background_events(rx).await.unwrap();
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 3);
    }

    #[tokio::test]
    async fn agent_thread_interrupt_cancels_unified_engine() {
        let temp = tempfile::tempdir().unwrap();
        let config = crate::runtime::Config::with_defaults(temp.path().to_path_buf());
        let mut agent = Session::with_session_id(config, "background-cancel".into()).unwrap();
        agent
            .session_messages
            .push(types::message::Message::user("wait"));
        let session = Arc::new(Mutex::new(agent));
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
            "test".into(),
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
}
