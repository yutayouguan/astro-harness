//! Unified turn-event helpers for the model/tool loop.

use std::sync::Arc;

use agent_protocol::{
    DeltaEvent, EventMsg, ExtensionItem, ItemEvent, TextItem, TokenCountEvent, ToolItem,
    ToolStatus, TurnItem,
};
use providers::Usage;

use super::provider::ProviderStreamer;
use crate::runtime::usage::{apply_llm_usage_dual_write, LlmUsageWrite};
use crate::runtime::{Session, TurnContext};

/// Persist an event before delivering it to live consumers.
pub(crate) async fn emit(session: &Session, turn_context: &TurnContext, msg: EventMsg) {
    session.send_event(turn_context.sub_id(), msg).await;
}

pub(crate) async fn emit_delta(
    session: &Session,
    turn_context: &TurnContext,
    item_id: &str,
    delta: String,
    reasoning: bool,
) {
    let event = DeltaEvent {
        turn_id: turn_context.sub_id().to_string(),
        item_id: item_id.to_string(),
        delta,
    };
    emit(
        session,
        turn_context,
        if reasoning {
            EventMsg::ReasoningContentDelta(event)
        } else {
            EventMsg::AgentMessageContentDelta(event)
        },
    )
    .await;
}

pub(crate) fn tool_turn_item(
    id: impl Into<String>,
    name: impl Into<String>,
    arguments: serde_json::Value,
    output: Option<serde_json::Value>,
    status: ToolStatus,
) -> TurnItem {
    let id = id.into();
    let name = name.into();
    let item = ToolItem {
        id,
        name: name.clone(),
        arguments,
        output,
        status,
    };
    if name == "terminal" || name == "code_exec" {
        TurnItem::CommandExecution(item)
    } else if name.starts_with("mcp__") {
        TurnItem::McpToolCall(item)
    } else if matches!(
        name.as_str(),
        "spawn_agent"
            | "list_agents"
            | "read_agent"
            | "send_message_to_agent"
            | "send_message"
            | "followup_task"
            | "wait_agents"
            | "wait_agent"
            | "interrupt_agent"
            | "close_agent"
    ) {
        TurnItem::CollabAgentToolCall(item)
    } else {
        TurnItem::DynamicToolCall(item)
    }
}

pub(crate) async fn emit_assistant_completed(
    session: &Session,
    turn_context: &TurnContext,
    item_id: String,
    content: String,
) {
    emit(
        session,
        turn_context,
        EventMsg::ItemCompleted(ItemEvent {
            turn_id: turn_context.sub_id().to_string(),
            item: TurnItem::AgentMessage(TextItem {
                id: item_id,
                content,
            }),
        }),
    )
    .await;
}

pub(crate) async fn emit_text_item_started(
    session: &Session,
    turn_context: &TurnContext,
    item_id: String,
    reasoning: bool,
) {
    emit(
        session,
        turn_context,
        EventMsg::ItemStarted(ItemEvent {
            turn_id: turn_context.sub_id().to_string(),
            item: text_item(item_id, String::new(), reasoning),
        }),
    )
    .await;
}

pub(crate) async fn emit_reasoning_completed(
    session: &Session,
    turn_context: &TurnContext,
    item_id: String,
    content: String,
) {
    emit(
        session,
        turn_context,
        EventMsg::ItemCompleted(ItemEvent {
            turn_id: turn_context.sub_id().to_string(),
            item: text_item(item_id, content, true),
        }),
    )
    .await;
}

pub(crate) async fn emit_response_items_completed(
    session: &Session,
    turn_context: &TurnContext,
    assistant_item_id: String,
    assistant_content: String,
    reasoning_item_id: String,
    reasoning_content: String,
) {
    emit_assistant_completed(session, turn_context, assistant_item_id, assistant_content).await;
    if !reasoning_content.is_empty() {
        emit_reasoning_completed(session, turn_context, reasoning_item_id, reasoning_content).await;
    }
}

fn text_item(id: String, content: String, reasoning: bool) -> TurnItem {
    let item = TextItem { id, content };
    if reasoning {
        TurnItem::Reasoning(item)
    } else {
        TurnItem::AgentMessage(item)
    }
}

pub(crate) async fn emit_extension_completed(
    session: &Session,
    turn_context: &TurnContext,
    id: String,
    namespace: &str,
    payload: serde_json::Value,
) {
    emit(
        session,
        turn_context,
        EventMsg::ItemCompleted(ItemEvent {
            turn_id: turn_context.sub_id().to_string(),
            item: TurnItem::Extension(ExtensionItem {
                id,
                namespace: namespace.to_string(),
                payload,
            }),
        }),
    )
    .await;
}

pub(crate) async fn emit_hook_started(
    session: &Session,
    turn_context: &TurnContext,
    hook_name: &str,
) -> String {
    let item_id = format!("hook-{}", uuid::Uuid::new_v4());
    emit(
        session,
        turn_context,
        EventMsg::HookStarted(ItemEvent {
            turn_id: turn_context.sub_id().to_string(),
            item: TurnItem::HookPrompt(TextItem {
                id: item_id.clone(),
                content: hook_name.to_string(),
            }),
        }),
    )
    .await;
    item_id
}

pub(crate) async fn emit_hook_completed(
    session: &Session,
    turn_context: &TurnContext,
    item_id: String,
    hook_name: &str,
) {
    emit(
        session,
        turn_context,
        EventMsg::HookCompleted(ItemEvent {
            turn_id: turn_context.sub_id().to_string(),
            item: TurnItem::HookPrompt(TextItem {
                id: item_id,
                content: hook_name.to_string(),
            }),
        }),
    )
    .await;
}

pub(crate) async fn emit_context_compacted(
    session: &Session,
    turn_context: &TurnContext,
    content: String,
) {
    emit(
        session,
        turn_context,
        EventMsg::ContextCompacted(ItemEvent {
            turn_id: turn_context.sub_id().to_string(),
            item: TurnItem::ContextCompaction(TextItem {
                id: format!("compaction-{}", uuid::Uuid::new_v4()),
                content,
            }),
        }),
    )
    .await;
}

pub(crate) fn is_subagent_tool(name: &str) -> bool {
    matches!(
        name,
        "spawn_agent"
            | "list_agents"
            | "read_agent"
            | "send_message_to_agent"
            | "send_message"
            | "followup_task"
            | "wait_agents"
            | "wait_agent"
            | "interrupt_agent"
            | "close_agent"
    )
}

pub(crate) async fn emit_subagent_activity(
    session: &Session,
    turn_context: &TurnContext,
    item_id: String,
    content: String,
) {
    emit(
        session,
        turn_context,
        EventMsg::SubAgentActivity(ItemEvent {
            turn_id: turn_context.sub_id().to_string(),
            item: TurnItem::SubAgentActivity(TextItem {
                id: item_id,
                content,
            }),
        }),
    )
    .await;
}

/// Best-effort dual write of LLM usage to usage.db and the session bill.
pub(super) async fn record_llm_usage(
    session: &Arc<Session>,
    streamer: &ProviderStreamer,
    usage: &Usage,
) {
    if usage.is_empty() {
        return;
    }
    let agent = session.as_ref();
    let agent_id = agent.agent_id().to_string();
    let session_id = agent.session_id().to_string();
    let turn_id = agent.current_turn_id().await;
    let fallback_provider = agent.chat_provider().to_string();
    let fallback_base_url = agent.chat_base_url().to_string();
    let fallback_api_key = agent.chat_api_key().to_string();
    let fallback_model = agent.chat_model().to_string();

    let (model, provider, base_url, api_key) = if let Some(meta) = streamer.last_hit_meta() {
        let api_key = streamer.api_key_for(&meta);
        (meta.model, meta.backend_id, meta.base_url, api_key)
    } else {
        (
            if fallback_model.is_empty() {
                streamer.primary_model()
            } else {
                fallback_model
            },
            fallback_provider,
            fallback_base_url,
            fallback_api_key,
        )
    };

    apply_llm_usage_dual_write(
        &LlmUsageWrite {
            agent_id: &agent_id,
            session_id: Some(&session_id),
            turn_id: turn_id.as_deref(),
            model: &model,
            usage,
            provider: &provider,
            base_url: &base_url,
            api_key: &api_key,
        },
        None,
        Some(agent.sessions()),
    );
}

pub(crate) async fn emit_usage(
    session: &Arc<Session>,
    turn_context: &TurnContext,
    streamer: &ProviderStreamer,
    usage: Option<Usage>,
) {
    let Some(usage) = usage else {
        return;
    };
    record_llm_usage(session, streamer, &usage).await;
    emit(
        session,
        turn_context,
        EventMsg::TokenCount(TokenCountEvent {
            turn_id: Some(turn_context.sub_id().to_string()),
            input_tokens: u64::from(usage.input_tokens),
            output_tokens: u64::from(usage.output_tokens),
            total_tokens: u64::from(usage.input_tokens) + u64::from(usage.output_tokens),
            cache_read_tokens: u64::from(usage.cache_read_tokens),
            cache_write_tokens: u64::from(usage.cache_write_tokens),
            reasoning_tokens: u64::from(usage.reasoning_tokens),
            request_count: u64::from(usage.request_count),
        }),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Config;

    async fn session() -> (tempfile::TempDir, Arc<Session>, Arc<TurnContext>) {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "event-helper-test".into(),
            )
            .unwrap(),
        );
        let context = session.create_turn_context("turn-1".into()).await;
        (dir, session, context)
    }

    #[tokio::test]
    async fn hook_started_and_completed_share_stable_item_id() {
        let (_dir, session, context) = session().await;
        let rx = session.subscribe_turn_events("turn-1").await;
        let item_id = emit_hook_started(&session, &context, "pre_api_request").await;
        emit_hook_completed(&session, &context, item_id.clone(), "pre_api_request").await;

        let started = rx.recv().await.unwrap();
        let completed = rx.recv().await.unwrap();
        assert!(matches!(
            started.msg,
            EventMsg::HookStarted(ItemEvent { item, .. }) if item.id() == item_id
        ));
        assert!(matches!(
            completed.msg,
            EventMsg::HookCompleted(ItemEvent { item, .. }) if item.id() == item_id
        ));
    }

    #[tokio::test]
    async fn compaction_helper_emits_context_compacted() {
        let (_dir, session, context) = session().await;
        let rx = session.subscribe_turn_events("turn-1").await;
        emit_context_compacted(&session, &context, "pruned=1 compressed=1".into()).await;

        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event.msg,
            EventMsg::ContextCompacted(ItemEvent { item, .. })
                if matches!(item, TurnItem::ContextCompaction(_))
        ));
    }

    #[tokio::test]
    async fn subagent_helper_emits_subagent_activity() {
        let (_dir, session, context) = session().await;
        let rx = session.subscribe_turn_events("turn-1").await;
        emit_subagent_activity(
            &session,
            &context,
            "subagent-call-1".into(),
            "running".into(),
        )
        .await;

        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event.msg,
            EventMsg::SubAgentActivity(ItemEvent { item, .. })
                if item.id() == "subagent-call-1"
        ));
    }
}
