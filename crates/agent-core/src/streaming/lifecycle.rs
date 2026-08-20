//! Unified turn-event helpers for the model/tool loop.

use std::sync::Arc;

use agent_protocol::{
    DeltaEvent, Event, EventMsg, ExtensionItem, ItemEvent, TextItem, TokenCountEvent, ToolItem,
    ToolStatus, TurnItem,
};
use providers::Usage;

use super::provider::ProviderStreamer;
use crate::runtime::event_identity::{event_turn_id, normalize_event_msg};
use crate::runtime::usage::{apply_llm_usage_dual_write, LlmUsageWrite};
use crate::runtime::{Session, TurnContext};

/// Match Codex's completed MCP event result cap: keep the model/history copy
/// untouched while preventing a single durable/live event from carrying
/// multi-megabyte inline payloads.
pub(crate) const TOOL_COMPLETED_EVENT_MAX_BYTES: usize = 1024 * 1024;

/// Persist an event before delivering it to live consumers.
pub(crate) async fn emit(session: &Session, turn_context: &TurnContext, msg: EventMsg) {
    session.send_event(turn_context.sub_id(), msg).await;
}

/// Send an event whose protocol-facing identities were normalized while its
/// bounded payload copy was constructed. This deliberately skips a second
/// identity projection while retaining raw-turn tap routing.
pub(crate) async fn emit_prepared(session: &Session, turn_context: &TurnContext, msg: EventMsg) {
    session
        .send_prepared_event(turn_context.sub_id(), msg)
        .await;
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
    media: Vec<types::MediaAsset>,
    status: ToolStatus,
) -> TurnItem {
    let id = id.into();
    let name = name.into();
    let item = ToolItem {
        id,
        name: name.clone(),
        arguments,
        output,
        media,
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

#[allow(clippy::too_many_arguments)]
fn tool_completed_event(
    turn_id: &str,
    id: &str,
    name: &str,
    arguments: &serde_json::Value,
    output: Option<serde_json::Value>,
    media: &[types::MediaAsset],
    status: ToolStatus,
) -> EventMsg {
    let mut event = EventMsg::ItemCompleted(ItemEvent {
        turn_id: turn_id.to_string(),
        item: tool_turn_item(id, name, arguments.clone(), output, media.to_vec(), status),
    });
    normalize_event_msg(&mut event, turn_id);
    event
}

fn serialized_event_len(turn_id: &str, event: &EventMsg) -> usize {
    let live = Event {
        id: event_turn_id(turn_id),
        msg: event.clone(),
    };
    let durable = agent_rollout::RolloutItem::EventMsg(event.clone());
    let live_len = serde_json::to_vec(&live).map_or(usize::MAX, |serialized| serialized.len());
    let durable_len = serde_json::to_vec(&durable)
        .map_or(usize::MAX, |serialized| serialized.len().saturating_add(1));
    live_len.max(durable_len)
}

#[allow(clippy::too_many_arguments)]
fn truncated_tool_completed_event(
    turn_id: &str,
    id: &str,
    name: &str,
    arguments: &serde_json::Value,
    stable_media: &[types::MediaAsset],
    status: ToolStatus,
    original_serialized_bytes: usize,
    inline_media_omitted: usize,
    stable_media_omitted: usize,
    preview: &str,
) -> EventMsg {
    tool_completed_event(
        turn_id,
        id,
        name,
        arguments,
        Some(serde_json::json!({
            "event_payload_truncated": true,
            "original_serialized_bytes": original_serialized_bytes,
            "inline_media_omitted": inline_media_omitted,
            "stable_media_omitted": stable_media_omitted,
            "preview": preview,
        })),
        stable_media,
        status,
    )
}

/// Build the completed tool event copy under the 1 MiB durable/live cap.
///
/// The original tool output has already been recorded before this helper is
/// called. Inline data URLs are never copied into an oversized event; stable
/// workspace/remote references are retained when they fit. The preview budget
/// is chosen against the fully serialized live and JSONL rollout envelopes,
/// so JSON escaping, wrapper overhead, and the record newline count toward the
/// cap.
#[allow(clippy::too_many_arguments)]
pub(crate) fn bounded_tool_completed_event(
    turn_id: &str,
    id: &str,
    name: &str,
    arguments: serde_json::Value,
    output: Option<serde_json::Value>,
    media: Vec<types::MediaAsset>,
    status: ToolStatus,
) -> EventMsg {
    let original = tool_completed_event(
        turn_id,
        id,
        name,
        &arguments,
        output.clone(),
        &media,
        status,
    );
    let original_serialized_bytes = serialized_event_len(turn_id, &original);
    if original_serialized_bytes <= TOOL_COMPLETED_EVENT_MAX_BYTES {
        return original;
    }
    drop(original);

    let preview_source = match output {
        Some(serde_json::Value::String(text)) => text,
        Some(value) => serde_json::to_string(&value).unwrap_or_default(),
        None => String::new(),
    };
    let inline_media_omitted = media
        .iter()
        .filter(|asset| matches!(asset.reference, types::MediaRef::DataUrl(_)))
        .count();
    let mut stable_media = media
        .into_iter()
        .filter(|asset| !matches!(asset.reference, types::MediaRef::DataUrl(_)))
        .collect::<Vec<_>>();
    let mut stable_media_omitted = 0;
    let mut event_arguments = arguments;

    let mut bounded = truncated_tool_completed_event(
        turn_id,
        id,
        name,
        &event_arguments,
        &stable_media,
        status,
        original_serialized_bytes,
        inline_media_omitted,
        stable_media_omitted,
        "",
    );
    if serialized_event_len(turn_id, &bounded) > TOOL_COMPLETED_EVENT_MAX_BYTES {
        stable_media_omitted = stable_media.len();
        stable_media.clear();
        bounded = truncated_tool_completed_event(
            turn_id,
            id,
            name,
            &event_arguments,
            &stable_media,
            status,
            original_serialized_bytes,
            inline_media_omitted,
            stable_media_omitted,
            "",
        );
    }
    if serialized_event_len(turn_id, &bounded) > TOOL_COMPLETED_EVENT_MAX_BYTES {
        event_arguments = serde_json::json!({
            "event_arguments_omitted": true,
        });
        bounded = truncated_tool_completed_event(
            turn_id,
            id,
            name,
            &event_arguments,
            &stable_media,
            status,
            original_serialized_bytes,
            inline_media_omitted,
            stable_media_omitted,
            "",
        );
    }

    // Final hard-stop fallback. Event-facing identities are already capped, so
    // a null-arguments/no-media marker has a small, deterministic upper bound.
    // The assertion prevents an oversized event from ever reaching dispatch if
    // that invariant is changed later.
    if serialized_event_len(turn_id, &bounded) > TOOL_COMPLETED_EVENT_MAX_BYTES {
        event_arguments = serde_json::Value::Null;
        stable_media_omitted = stable_media_omitted.saturating_add(stable_media.len());
        stable_media.clear();
        bounded = truncated_tool_completed_event(
            turn_id,
            id,
            name,
            &event_arguments,
            &stable_media,
            status,
            original_serialized_bytes,
            inline_media_omitted,
            stable_media_omitted,
            "",
        );
    }
    assert!(
        serialized_event_len(turn_id, &bounded) <= TOOL_COMPLETED_EVENT_MAX_BYTES,
        "minimal completed event exceeded the hard payload cap"
    );
    if preview_source.is_empty() {
        return bounded;
    }

    let mut best = bounded;
    let mut low = 0usize;
    let mut high = preview_source.len().min(TOOL_COMPLETED_EVENT_MAX_BYTES);
    while low <= high {
        let mid = low + (high - low) / 2;
        let preview = types::truncate_utf8(&preview_source, mid);
        let candidate = truncated_tool_completed_event(
            turn_id,
            id,
            name,
            &event_arguments,
            &stable_media,
            status,
            original_serialized_bytes,
            inline_media_omitted,
            stable_media_omitted,
            &preview,
        );
        if serialized_event_len(turn_id, &candidate) <= TOOL_COMPLETED_EVENT_MAX_BYTES {
            best = candidate;
            low = mid.saturating_add(1);
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }
    best
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
            total_tokens: u64::from(usage.input_tokens)
                .saturating_add(u64::from(usage.output_tokens))
                .saturating_add(u64::from(usage.cache_read_tokens))
                .saturating_add(u64::from(usage.cache_write_tokens)),
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

    #[test]
    fn oversized_mcp_inline_media_is_removed_from_completed_event_copy() {
        let data_url = format!(
            "data:image/png;base64,{}",
            "mcp-inline-sentinel".repeat(TOOL_COMPLETED_EVENT_MAX_BYTES / 8)
        );
        let event = bounded_tool_completed_event(
            "turn-1",
            "mcp-call-1",
            "mcp__server__image",
            serde_json::json!({}),
            Some(serde_json::Value::String("generated".into())),
            vec![types::MediaAsset::data_url(
                types::MediaKind::Image,
                data_url.clone(),
                "image/png",
            )],
            ToolStatus::Completed,
        );

        assert!(serialized_event_len("turn-1", &event) <= TOOL_COMPLETED_EVENT_MAX_BYTES);
        let EventMsg::ItemCompleted(ItemEvent {
            item: TurnItem::McpToolCall(tool),
            ..
        }) = &event
        else {
            panic!("expected MCP completed item");
        };
        assert_eq!(tool.id, "mcp-call-1");
        assert!(tool.media.is_empty());
        let serialized = serde_json::to_string(&event).unwrap();
        assert!(serialized.contains("event_payload_truncated"));
        assert!(!serialized.contains(&data_url));
    }

    fn completed_event_with_identity(turn_id: &str, call_id: &str, name: &str) -> EventMsg {
        bounded_tool_completed_event(
            turn_id,
            call_id,
            name,
            serde_json::json!({}),
            Some(serde_json::Value::String("done".into())),
            Vec::new(),
            ToolStatus::Completed,
        )
    }

    #[test]
    fn oversized_provider_call_id_cannot_break_completed_event_cap() {
        let call_id = "provider-call-id".repeat(TOOL_COMPLETED_EVENT_MAX_BYTES / 8);
        let first = completed_event_with_identity("turn-1", &call_id, "terminal");
        let second = completed_event_with_identity("turn-1", &call_id, "terminal");
        assert!(serialized_event_len("turn-1", &first) <= TOOL_COMPLETED_EVENT_MAX_BYTES);
        assert_eq!(first, second, "bounded correlation id must be stable");
    }

    #[test]
    fn oversized_provider_tool_name_cannot_break_completed_event_cap() {
        let name = "provider-tool-name".repeat(TOOL_COMPLETED_EVENT_MAX_BYTES / 8);
        let first = completed_event_with_identity("turn-1", "call-1", &name);
        let second = completed_event_with_identity("turn-1", "call-1", &name);
        assert!(serialized_event_len("turn-1", &first) <= TOOL_COMPLETED_EVENT_MAX_BYTES);
        assert_eq!(first, second, "bounded display name must be stable");
    }

    #[test]
    fn oversized_turn_id_cannot_break_completed_event_cap() {
        let turn_id = "provider-turn-id".repeat(TOOL_COMPLETED_EVENT_MAX_BYTES / 8);
        let first = completed_event_with_identity(&turn_id, "call-1", "terminal");
        let second = completed_event_with_identity(&turn_id, "call-1", "terminal");
        assert!(serialized_event_len(&turn_id, &first) <= TOOL_COMPLETED_EVENT_MAX_BYTES);
        assert_eq!(first, second, "bounded turn correlation id must be stable");
    }

    #[test]
    fn normal_provider_identity_remains_exact() {
        let event = completed_event_with_identity("turn-1", "provider-call-1", "mcp__server__tool");
        let EventMsg::ItemCompleted(ItemEvent {
            turn_id,
            item: TurnItem::McpToolCall(tool),
        }) = event
        else {
            panic!("expected MCP completed item");
        };

        assert_eq!(turn_id, "turn-1");
        assert_eq!(tool.id, "provider-call-1");
        assert_eq!(tool.name, "mcp__server__tool");
    }

    #[tokio::test]
    async fn oversized_turn_exact_tap_receives_normalized_terminal_without_hanging() {
        let (_dir, session, _context) = session().await;
        let raw_turn_id = "raw-turn-routing".repeat(TOOL_COMPLETED_EVENT_MAX_BYTES / 8);
        let rx = session.subscribe_turn_events(&raw_turn_id).await;
        session
            .send_event(
                &raw_turn_id,
                EventMsg::TurnComplete(agent_protocol::TurnCompleteEvent {
                    turn_id: raw_turn_id.clone(),
                    last_agent_message: Some("done".into()),
                    error: None,
                }),
            )
            .await;

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("exact turn tap timed out")
            .expect("exact turn tap closed");
        assert!(event.msg.is_terminal());
        assert_ne!(event.id, raw_turn_id);
        assert!(serde_json::to_vec(&event).unwrap().len() <= TOOL_COMPLETED_EVENT_MAX_BYTES);
        assert!(matches!(
            event.msg,
            EventMsg::TurnComplete(agent_protocol::TurnCompleteEvent { turn_id, .. })
                if turn_id == event.id
        ));
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
