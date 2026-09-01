//! 模型/工具循环的统一 turn 事件辅助函数。

use std::sync::Arc;

use agent_protocol::{
    AgentMessageItem, DeltaEvent, Event, EventMsg, ExtensionItem, ItemEvent, TextItem,
    TokenCountEvent, ToolExecutionMode, ToolItem, ToolStatus, TurnItem,
};
use providers::Usage;

use super::provider::ProviderStreamer;
use crate::runtime::event_identity::{event_turn_id, normalize_event_msg};
use crate::runtime::usage::{apply_llm_usage_dual_write, LlmUsageWrite};
use crate::runtime::{Session, TurnContext};

/// MCP 事件结果上限：保持模型/历史副本不变，同时防止单个持久/实时事件
/// 携带多兆字节的内联负载。
pub(crate) const TOOL_COMPLETED_EVENT_MAX_BYTES: usize = 1024 * 1024;

/// 在将事件投递给实时消费者之前先持久化。
pub(crate) async fn emit(session: &Session, turn_context: &TurnContext, msg: EventMsg) {
    session.send_event(turn_context.sub_id(), msg).await;
}

/// 发送一个在构建有界负载副本时已完成协议侧身份归一化的事件。
/// 刻意跳过第二次身份投影，同时保留原始 turn tap 路由。
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolExecutionMetadata {
    pub batch_id: String,
    pub mode: ToolExecutionMode,
}

pub(crate) fn tool_turn_item_with_execution(
    id: impl Into<String>,
    name: impl Into<String>,
    arguments: serde_json::Value,
    output: Option<serde_json::Value>,
    media: Vec<types::MediaAsset>,
    status: ToolStatus,
    execution: Option<&ToolExecutionMetadata>,
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
        batch_id: execution.map(|value| value.batch_id.clone()),
        execution_mode: execution.map(|value| value.mode),
    };
    if name == "exec_command" || name == "code_exec" {
        TurnItem::CommandExecution(item)
    } else if name == "image_gen" {
        TurnItem::ImageGeneration(item)
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
    execution: Option<&ToolExecutionMetadata>,
) -> EventMsg {
    let mut event = EventMsg::ItemCompleted(ItemEvent {
        turn_id: turn_id.to_string(),
        item: tool_turn_item_with_execution(
            id,
            name,
            arguments.clone(),
            output,
            media.to_vec(),
            status,
            execution,
        ),
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
    execution: Option<&ToolExecutionMetadata>,
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
        execution,
    )
}

/// 在 1 MiB 持久/实时上限内构建已完成的工具事件副本。
///
/// 在调用此辅助函数之前，原始工具输出已经被记录。内联 data URL 不会被复制到
/// 超大事件中；当空间允许时保留稳定的工作区/远程引用。预览预算基于完整序列化后的
/// 实时和 JSONL rollout 信封计算，因此 JSON 转义、包装开销和记录换行符
/// 都计入上限。
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
    bounded_tool_completed_event_inner(turn_id, id, name, arguments, output, media, status, None)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn bounded_tool_completed_event_with_execution(
    turn_id: &str,
    id: &str,
    name: &str,
    arguments: serde_json::Value,
    output: Option<serde_json::Value>,
    media: Vec<types::MediaAsset>,
    status: ToolStatus,
    execution: &ToolExecutionMetadata,
) -> EventMsg {
    bounded_tool_completed_event_inner(
        turn_id,
        id,
        name,
        arguments,
        output,
        media,
        status,
        Some(execution),
    )
}

#[allow(clippy::too_many_arguments)]
fn bounded_tool_completed_event_inner(
    turn_id: &str,
    id: &str,
    name: &str,
    arguments: serde_json::Value,
    output: Option<serde_json::Value>,
    media: Vec<types::MediaAsset>,
    status: ToolStatus,
    execution: Option<&ToolExecutionMetadata>,
) -> EventMsg {
    let original = tool_completed_event(
        turn_id,
        id,
        name,
        &arguments,
        output.clone(),
        &media,
        status,
        execution,
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
        execution,
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
            execution,
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
            execution,
        );
    }

    // 最终硬停兜底。事件侧身份已被截断，因此 null-arguments/no-media 标记
    // 具有小且确定的上界。此断言防止超大事件到达分发环节（即使将来
    // 该不变量被改变）。
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
            execution,
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
            execution,
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
            item: TurnItem::AgentMessage(AgentMessageItem {
                id: item_id,
                content,
                delivery: None,
            }),
        }),
    )
    .await;
}

pub(crate) async fn emit_async_agent_message(
    session: &Session,
    turn_context: &TurnContext,
    item_id: String,
    content: String,
) {
    let item = TurnItem::AgentMessage(AgentMessageItem {
        id: item_id,
        content,
        delivery: Some(agent_protocol::AgentMessageDelivery::Async),
    });
    emit(
        session,
        turn_context,
        EventMsg::ItemStarted(ItemEvent {
            turn_id: turn_context.sub_id().to_string(),
            item: item.clone(),
        }),
    )
    .await;
    emit(
        session,
        turn_context,
        EventMsg::ItemCompleted(ItemEvent {
            turn_id: turn_context.sub_id().to_string(),
            item,
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
    assistant_started: bool,
    assistant_item_id: String,
    assistant_content: String,
    reasoning_item_id: String,
    reasoning_content: String,
) {
    if assistant_started {
        emit_assistant_completed(session, turn_context, assistant_item_id, assistant_content).await;
    }
    if !reasoning_content.is_empty() {
        emit_reasoning_completed(session, turn_context, reasoning_item_id, reasoning_content).await;
    }
}

fn text_item(id: String, content: String, reasoning: bool) -> TurnItem {
    let item = TextItem { id, content };
    if reasoning {
        TurnItem::Reasoning(item)
    } else {
        TurnItem::AgentMessage(AgentMessageItem {
            id: item.id,
            content: item.content,
            delivery: None,
        })
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

/// 尽力双写 LLM 用量到 usage.db 和会话账单。
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
    )
    .await;
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
            input_tokens: u64::from(usage.prompt_tokens()),
            input_tokens_include_cache: true,
            uncached_input_tokens: u64::from(usage.input_tokens),
            output_tokens: u64::from(usage.output_tokens),
            total_tokens: u64::from(usage.total_tokens()),
            provider_total_tokens: usage.reported_total_tokens.map(u64::from),
            cache_read_tokens: u64::from(usage.cache_read_tokens),
            cache_write_tokens: u64::from(usage.cache_write_tokens),
            reasoning_tokens: u64::from(usage.reasoning_tokens),
            request_count: u64::from(usage.request_count),
            cache_read_reported: usage.cache_read_reported,
            cache_write_reported: usage.cache_write_reported,
            reasoning_reported: usage.reasoning_reported,
        }),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Config;

    #[test]
    fn tool_item_keeps_execution_batch_metadata() {
        let execution = ToolExecutionMetadata {
            batch_id: "batch-1".into(),
            mode: ToolExecutionMode::Parallel,
        };
        let TurnItem::DynamicToolCall(item) = tool_turn_item_with_execution(
            "call-1",
            "read_file",
            serde_json::json!({}),
            None,
            Vec::new(),
            ToolStatus::InProgress,
            Some(&execution),
        ) else {
            panic!("expected dynamic tool call");
        };
        assert_eq!(item.batch_id.as_deref(), Some("batch-1"));
        assert_eq!(item.execution_mode, Some(ToolExecutionMode::Parallel));
    }

    #[test]
    fn image_gen_is_a_first_class_business_item() {
        let TurnItem::ImageGeneration(item) = tool_turn_item_with_execution(
            "call-image-1",
            "image_gen",
            serde_json::json!({"prompt": "A polar bear"}),
            None,
            Vec::new(),
            ToolStatus::InProgress,
            None,
        ) else {
            panic!("expected image generation item");
        };

        assert_eq!(item.id, "call-image-1");
        assert_eq!(item.name, "image_gen");
        assert_eq!(item.status, ToolStatus::InProgress);
    }

    async fn session() -> (tempfile::TempDir, Arc<Session>, Arc<TurnContext>) {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "event-helper-test".into(),
            )
            .await
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
        let first = completed_event_with_identity("turn-1", &call_id, "exec_command");
        let second = completed_event_with_identity("turn-1", &call_id, "exec_command");
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
        let first = completed_event_with_identity(&turn_id, "call-1", "exec_command");
        let second = completed_event_with_identity(&turn_id, "call-1", "exec_command");
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
