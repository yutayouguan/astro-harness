//! 多轮流式与 `PauseControl` 集成测试。

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use providers::types::stream::StreamChunk;
use providers::{CompletionStream, ProviderConfig};
use providers::{PauseControl, Usage};
use tokio::sync::Notify;

use agent::runtime::{AgentConfig, AgentLoop};
use agent::streaming::{
    run_multi_turn_stream_with_chat_fn, run_thread_turn_events, ChatOverride,
    StreamedAssistantContent, ThreadTurnEventArgs,
};
use agent::TurnInput;
use agent_protocol::{Event, EventMsg, TurnItem};

#[derive(Debug, Clone)]
enum ProjectedStreamItem {
    Assistant(StreamedAssistantContent),
    ToolStarted {
        name: String,
    },
    ToolResult {
        id: String,
        name: String,
        result: String,
        media: Vec<types::MediaAsset>,
    },
    MemoryUpdate {
        op: String,
    },
    ContextUsage(agent_protocol::ContextUsageEvent),
    UserInputCommitted(String),
    RunStarted {},
    RunFinished {
        outcome_type: String,
        interrupts_json: String,
    },
    Error(String),
    Done,
}

struct ProjectedStreamArgs {
    session: Arc<agent::Session>,
    targets: Vec<types::ChatTarget>,
    base_config: ProviderConfig,
    input: Vec<TurnInput>,
    system_prompt: Option<String>,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<agent::HitlGate>>,
    tx: tokio::sync::mpsc::Sender<anyhow::Result<ProjectedStreamItem>>,
    chat_override: Option<ChatOverride>,
}

fn project_event(event: Event) -> Vec<ProjectedStreamItem> {
    match event.msg {
        EventMsg::TurnStarted(_) => vec![ProjectedStreamItem::RunStarted {}],
        EventMsg::AgentMessageContentDelta(delta) => vec![ProjectedStreamItem::Assistant(
            StreamedAssistantContent::Text(delta.delta),
        )],
        EventMsg::ReasoningContentDelta(delta) => vec![ProjectedStreamItem::Assistant(
            StreamedAssistantContent::Reasoning(delta.delta),
        )],
        EventMsg::DynamicToolCallRequest(request) => vec![ProjectedStreamItem::Assistant(
            StreamedAssistantContent::ToolCallDelta(types::ToolCallDelta {
                index: request
                    .payload
                    .get("index")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default() as u32,
                id: Some(request.item_id),
                name: request
                    .payload
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                arguments: request
                    .payload
                    .get("delta")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                signature: None,
            }),
        )],
        EventMsg::RequestUserInput(request)
        | EventMsg::RequestPermissions(request)
        | EventMsg::ExecApprovalRequest(request)
        | EventMsg::ApplyPatchApprovalRequest(request) => {
            vec![ProjectedStreamItem::RunFinished {
                outcome_type: "hitl_waiting".into(),
                interrupts_json: serde_json::json!([{
                    "id": request.request_id,
                    "reason": request.payload.get("reason").cloned().unwrap_or_default(),
                    "message": request.payload.get("message").cloned().unwrap_or_default(),
                    "tool_call_id": request.item_id,
                    "response_schema_json": request
                        .payload
                        .get("response_schema")
                        .cloned()
                        .unwrap_or_default()
                        .to_string(),
                }])
                .to_string(),
            }]
        }
        EventMsg::ItemStarted(item) => match item.item {
            TurnItem::CommandExecution(tool)
            | TurnItem::DynamicToolCall(tool)
            | TurnItem::McpToolCall(tool)
            | TurnItem::CollabAgentToolCall(tool) => {
                vec![ProjectedStreamItem::ToolStarted { name: tool.name }]
            }
            _ => Vec::new(),
        },
        EventMsg::ItemCompleted(item) => match item.item {
            TurnItem::CommandExecution(tool)
            | TurnItem::DynamicToolCall(tool)
            | TurnItem::McpToolCall(tool)
            | TurnItem::CollabAgentToolCall(tool) => vec![ProjectedStreamItem::ToolResult {
                id: tool.id,
                name: tool.name,
                result: tool
                    .output
                    .map(|value| match value {
                        serde_json::Value::String(text) => text,
                        other => other.to_string(),
                    })
                    .unwrap_or_default(),
                media: tool.media,
            }],
            TurnItem::Extension(extension) if extension.namespace == "astro.memory" => {
                vec![ProjectedStreamItem::MemoryUpdate {
                    op: extension
                        .payload
                        .get("op")
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| {
                            extension
                                .payload
                                .get("source")
                                .and_then(serde_json::Value::as_str)
                                .map(|source| if source == "tool" { "memory" } else { source })
                        })
                        .unwrap_or("memory")
                        .to_string(),
                }]
            }
            _ => Vec::new(),
        },
        EventMsg::ContextUsage(context) => vec![ProjectedStreamItem::ContextUsage(context)],
        EventMsg::UserInputCommitted(committed) => vec![ProjectedStreamItem::UserInputCommitted(
            committed.client_message_id,
        )],
        EventMsg::TokenCount(tokens) => vec![ProjectedStreamItem::Assistant(
            StreamedAssistantContent::FinalUsage(Usage {
                input_tokens: u32::try_from(tokens.input_tokens).unwrap_or(u32::MAX),
                output_tokens: u32::try_from(tokens.output_tokens).unwrap_or(u32::MAX),
                cache_read_tokens: u32::try_from(tokens.cache_read_tokens).unwrap_or(u32::MAX),
                cache_write_tokens: u32::try_from(tokens.cache_write_tokens).unwrap_or(u32::MAX),
                reasoning_tokens: u32::try_from(tokens.reasoning_tokens).unwrap_or(u32::MAX),
                request_count: u32::try_from(tokens.request_count).unwrap_or(u32::MAX),
            }),
        )],
        EventMsg::Error(error) => vec![ProjectedStreamItem::Error(error.message)],
        EventMsg::TurnComplete(complete) => vec![
            ProjectedStreamItem::RunFinished {
                outcome_type: if complete.error.is_some() {
                    "error".into()
                } else {
                    "success".into()
                },
                interrupts_json: "[]".into(),
            },
            ProjectedStreamItem::Done,
        ],
        EventMsg::TurnAborted(_) => vec![
            ProjectedStreamItem::RunFinished {
                outcome_type: "interrupt".into(),
                interrupts_json: "[]".into(),
            },
            ProjectedStreamItem::Done,
        ],
        _ => Vec::new(),
    }
}

async fn forward_projected_events(
    mut rx: tokio::sync::mpsc::Receiver<anyhow::Result<Event>>,
    tx: tokio::sync::mpsc::Sender<anyhow::Result<ProjectedStreamItem>>,
) {
    let mut saw_started = false;
    let mut saw_terminal = false;
    while let Some(event) = rx.recv().await {
        let event = event.unwrap();
        if matches!(event.msg, EventMsg::TurnStarted(_)) {
            saw_started = true;
        } else if matches!(event.msg, EventMsg::Error(_)) && !saw_started {
            saw_started = true;
            let _ = tx.send(Ok(ProjectedStreamItem::RunStarted {})).await;
        }
        for item in project_event(event) {
            saw_terminal |= matches!(item, ProjectedStreamItem::Done);
            if tx.send(Ok(item)).await.is_err() {
                return;
            }
        }
    }
    if saw_started && !saw_terminal {
        let _ = tx
            .send(Ok(ProjectedStreamItem::RunFinished {
                outcome_type: "error".into(),
                interrupts_json: "[]".into(),
            }))
            .await;
        let _ = tx.send(Ok(ProjectedStreamItem::Done)).await;
    }
}

async fn run_projected_stream(args: ProjectedStreamArgs) {
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(64);
    let projected = forward_projected_events(event_rx, args.tx);
    let run = run_thread_turn_events(ThreadTurnEventArgs {
        session: args.session,
        targets: args.targets,
        base_config: args.base_config,
        input: args.input,
        system_prompt: args.system_prompt,
        pause: args.pause,
        hitl_gate: args.hitl_gate,
        tx: event_tx,
        chat_override: args.chat_override,
    });
    tokio::join!(run, projected);
}

async fn run_projected_stream_with_chat_fn(
    session: Arc<agent::Session>,
    chat_fn: ChatOverride,
    config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<agent::HitlGate>>,
    tx: tokio::sync::mpsc::Sender<anyhow::Result<ProjectedStreamItem>>,
) {
    run_projected_stream(ProjectedStreamArgs {
        session,
        targets: vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            base_url: config.base_url.clone().unwrap_or_default(),
        }],
        base_config: config,
        input: Vec::new(),
        system_prompt: Some(system_prompt),
        pause,
        hitl_gate,
        tx,
        chat_override: Some(chat_fn),
    })
    .await;
}

#[tokio::test]
async fn scripted_tool_turn_emits_item_lifecycle_and_one_terminal() {
    let (_dir, session, thread, _recorder, _path) = common::new_thread().await;
    let turn_id = "scripted-tool-turn";
    let turn_context = session.create_turn_context(turn_id.into()).await;
    let chat = scripted_chat(vec![
        vec![
            StreamChunk::ToolCallStart {
                index: 0,
                id: "call-1".into(),
                name: "terminal".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: r#"{"command":"pwd"}"#.into(),
            },
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        vec![
            StreamChunk::Thinking("because".into()),
            StreamChunk::Text("done".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);
    let run = tokio::spawn(run_multi_turn_stream_with_chat_fn(
        Arc::clone(&session),
        turn_context,
        vec![TurnInput {
            content: "run pwd".into(),
            image_data_urls: Vec::new(),
            client_message_id: None,
        }],
        chat,
    ));
    let events = common::collect_through_terminal(&thread, turn_id).await;
    run.await.unwrap().unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.msg,
        EventMsg::ItemStarted(item) if item.item.id() == "call-1"
    )));
    assert!(events.iter().any(|event| matches!(
        &event.msg,
        EventMsg::ItemCompleted(item) if item.item.id() == "call-1"
    )));
    assert!(events.iter().any(|event| matches!(
        &event.msg,
        EventMsg::AgentMessageContentDelta(delta) if delta.delta == "done"
    )));
    assert_text_item_lifecycle(&events, false, "done");
    assert_text_item_lifecycle(&events, true, "because");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.msg.is_terminal())
            .count(),
        1
    );
}

fn assert_text_item_lifecycle(
    events: &[agent_protocol::Event],
    reasoning: bool,
    expected_delta: &str,
) {
    let delta = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match &event.msg {
            EventMsg::AgentMessageContentDelta(delta)
                if !reasoning && delta.delta == expected_delta =>
            {
                Some((index, delta.item_id.clone()))
            }
            EventMsg::ReasoningContentDelta(delta)
                if reasoning && delta.delta == expected_delta =>
            {
                Some((index, delta.item_id.clone()))
            }
            _ => None,
        })
        .expect("text item must emit its delta");
    let started = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match &event.msg {
            EventMsg::ItemStarted(item)
                if item.item.id() == delta.1
                    && matches!(
                        (&item.item, reasoning),
                        (agent_protocol::TurnItem::AgentMessage(_), false)
                            | (agent_protocol::TurnItem::Reasoning(_), true)
                    ) =>
            {
                Some((index, item.item.id().to_string()))
            }
            _ => None,
        })
        .expect("text item must start");
    let completed = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match &event.msg {
            EventMsg::ItemCompleted(item)
                if item.item.id() == started.1
                    && matches!(
                        (&item.item, reasoning),
                        (agent_protocol::TurnItem::AgentMessage(_), false)
                            | (agent_protocol::TurnItem::Reasoning(_), true)
                    ) =>
            {
                Some((index, item.item.id().to_string()))
            }
            _ => None,
        })
        .expect("text item must complete");

    assert_eq!(started.1, delta.1);
    assert_eq!(delta.1, completed.1);
    assert!(started.0 < delta.0, "ItemStarted must precede delta");
    assert!(delta.0 < completed.0, "delta must precede ItemCompleted");
}

#[tokio::test]
async fn tool_argument_events_keep_stable_ids_across_late_start_and_rounds() {
    let (_dir, session, thread, _recorder, _path) = common::new_thread().await;
    let turn_id = "stable-tool-ids";
    let turn_context = session.create_turn_context(turn_id.into()).await;
    let chat = scripted_chat(vec![
        vec![
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: r#"{"command":"pwd"}"#.into(),
            },
            StreamChunk::ToolCallStart {
                index: 0,
                id: "provider-call-1".into(),
                name: "terminal".into(),
            },
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        vec![
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: r#"{"command":"pwd"}"#.into(),
            },
            StreamChunk::ToolCallStart {
                index: 0,
                id: "provider-call-2".into(),
                name: "terminal".into(),
            },
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        vec![
            StreamChunk::Text("done".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);
    let run = tokio::spawn(run_multi_turn_stream_with_chat_fn(
        Arc::clone(&session),
        turn_context,
        vec![TurnInput {
            content: "run twice".into(),
            image_data_urls: Vec::new(),
            client_message_id: None,
        }],
        chat,
    ));
    let events = common::collect_through_terminal(&thread, turn_id).await;
    run.await.unwrap().unwrap();

    let argument_events = events
        .iter()
        .filter_map(|event| match &event.msg {
            EventMsg::DynamicToolCallRequest(request) => Some(request),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(argument_events.len(), 4);
    let expected_ids = [
        "provider-call-1",
        "provider-call-1",
        "provider-call-2",
        "provider-call-2",
    ];
    assert_eq!(
        argument_events
            .iter()
            .map(|request| request.item_id.as_str())
            .collect::<Vec<_>>(),
        expected_ids
    );
    for request in argument_events {
        assert!(request.request_id.contains(turn_id));
        assert!(request.request_id.contains(&request.item_id));
    }

    let started_tool_ids = events
        .iter()
        .filter_map(|event| match &event.msg {
            EventMsg::ItemStarted(item)
                if matches!(
                    item.item,
                    agent_protocol::TurnItem::CommandExecution(_)
                        | agent_protocol::TurnItem::DynamicToolCall(_)
                        | agent_protocol::TurnItem::McpToolCall(_)
                        | agent_protocol::TurnItem::CollabAgentToolCall(_)
                ) =>
            {
                Some(item.item.id())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(started_tool_ids, vec!["provider-call-1", "provider-call-2"]);
    let completed_tool_ids = events
        .iter()
        .filter_map(|event| match &event.msg {
            EventMsg::ItemCompleted(item)
                if matches!(
                    item.item,
                    agent_protocol::TurnItem::CommandExecution(_)
                        | agent_protocol::TurnItem::DynamicToolCall(_)
                        | agent_protocol::TurnItem::McpToolCall(_)
                        | agent_protocol::TurnItem::CollabAgentToolCall(_)
                ) =>
            {
                Some(item.item.id())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        completed_tool_ids,
        vec!["provider-call-1", "provider-call-2"]
    );

    let history = session.clone_history().await;
    let recorded_call_ids = history
        .iter()
        .flat_map(|message| message.tool_calls.iter().flatten())
        .map(|call| call.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        recorded_call_ids,
        vec!["provider-call-1", "provider-call-2"]
    );
    let recorded_result_ids = history
        .iter()
        .filter_map(|message| message.tool_call_id.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        recorded_result_ids,
        vec!["provider-call-1", "provider-call-2"]
    );
}

#[tokio::test]
async fn argument_only_malformed_tool_delta_emits_no_orphan_request() {
    let (_dir, session, thread, _recorder, _path) = common::new_thread().await;
    let turn_id = "malformed-tool-delta";
    let turn_context = session.create_turn_context(turn_id.into()).await;
    let chat = scripted_chat(vec![vec![
        StreamChunk::ToolCallDelta {
            index: 0,
            arguments: r#"{"command":"pwd"}"#.into(),
        },
        StreamChunk::Done {
            finish_reason: "tool_calls".into(),
        },
    ]]);
    let run = tokio::spawn(run_multi_turn_stream_with_chat_fn(
        Arc::clone(&session),
        turn_context,
        vec![TurnInput {
            content: "malformed tool".into(),
            image_data_urls: Vec::new(),
            client_message_id: None,
        }],
        chat,
    ));
    let events = common::collect_through_terminal(&thread, turn_id).await;
    run.await.unwrap().unwrap();

    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.msg, EventMsg::DynamicToolCallRequest(_)))
            .count(),
        0,
        "argument-only malformed tool calls must not emit orphan requests"
    );
}

#[tokio::test]
async fn billing_token_count_total_includes_cached_tokens() {
    let (_dir, session, thread, _recorder, _path) = common::new_thread().await;
    let turn_id = "billing-token-total";
    let turn_context = session.create_turn_context(turn_id.into()).await;
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 5,
        cache_read_tokens: 4,
        cache_write_tokens: 2,
        reasoning_tokens: 0,
        request_count: 1,
    };
    let chat = scripted_chat(vec![vec![
        StreamChunk::Text("done".into()),
        StreamChunk::Usage(usage),
        StreamChunk::Done {
            finish_reason: "stop".into(),
        },
    ]]);
    let run = tokio::spawn(run_multi_turn_stream_with_chat_fn(
        Arc::clone(&session),
        turn_context,
        vec![TurnInput {
            content: "count tokens".into(),
            image_data_urls: Vec::new(),
            client_message_id: None,
        }],
        chat,
    ));
    let events = common::collect_through_terminal(&thread, turn_id).await;
    run.await.unwrap().unwrap();

    let billing = events
        .iter()
        .find_map(|event| match &event.msg {
            EventMsg::TokenCount(tokens) if tokens.request_count > 0 => Some(tokens),
            _ => None,
        })
        .expect("billing TokenCount event");
    assert_eq!(billing.total_tokens, 21);
}

#[tokio::test]
async fn media_tool_result_survives_rollout_and_legacy_adapter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("media-rollout.jsonl");
    let mut agent = AgentLoop::with_session_id(
        AgentConfig::with_defaults(dir.path().to_path_buf()),
        "media-tool-session".into(),
    )
    .unwrap();
    agent.tool_registry_mut().register_dynamic(
        types::ToolEntry {
            name: "test_media".into(),
            toolset: "test_media".into(),
            description: "returns structured media".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            ..types::ToolEntry::lifecycle_defaults()
        },
        Arc::new(|_name, _args| {
            Box::pin(async {
                Ok(types::ToolOutput::Media {
                    text: "generated".into(),
                    assets: vec![types::MediaAsset {
                        kind: types::MediaKind::Image,
                        mime_type: "image/png".into(),
                        reference: types::MediaRef::DataUrl(
                            "data:image/png;base64,c21hbGw=".into(),
                        ),
                        label: Some("generated image".into()),
                        id: Some("asset-1".into()),
                    }],
                })
            })
        }),
    );
    let session = Arc::new(agent);
    let recorder = agent_rollout::RolloutRecorder::open(
        path.clone(),
        agent_rollout::ThreadHistoryMode::Paginated,
    )
    .await
    .unwrap();
    let thread = agent::AstroThread::spawn(Arc::clone(&session), recorder).unwrap();
    session
        .record_items(vec![types::message::Message::user("generate media")])
        .await;
    let chat = scripted_chat(vec![
        vec![
            StreamChunk::ToolCallStart {
                index: 0,
                id: "media-call".into(),
                name: "test_media".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: "{}".into(),
            },
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        vec![
            StreamChunk::Text("done".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let run = tokio::spawn(run_projected_stream_with_chat_fn(
        Arc::clone(&session),
        chat,
        ProviderConfig::default(),
        "system".into(),
        PauseControl::new(),
        None,
        tx,
    ));
    let mut legacy = Vec::new();
    while let Some(item) = rx.recv().await {
        legacy.push(item.unwrap());
    }
    run.await.unwrap();
    let mut unified = Vec::new();
    loop {
        let event = thread.next_event().await.unwrap();
        let terminal = event.msg.is_terminal();
        unified.push(event);
        if terminal {
            break;
        }
    }

    let unified_tool = unified
        .iter()
        .find_map(|event| match &event.msg {
            EventMsg::ItemCompleted(item) if item.item.id() == "media-call" => match &item.item {
                agent_protocol::TurnItem::DynamicToolCall(tool) => Some(tool),
                _ => None,
            },
            _ => None,
        })
        .expect("unified completed media tool item");
    assert_eq!(
        serde_json::to_value(unified_tool).unwrap()["media"][0]["id"],
        "asset-1"
    );
    assert!(matches!(
        &unified_tool.media[0].reference,
        types::MediaRef::DataUrl(value) if value == "data:image/png;base64,c21hbGw="
    ));
    assert!(legacy.iter().any(|item| matches!(
        item,
        ProjectedStreamItem::ToolResult { id, media, .. }
            if id == "media-call" && media.first().is_some_and(|asset| {
                asset.id.as_deref() == Some("asset-1")
                    && matches!(
                        &asset.reference,
                        types::MediaRef::DataUrl(value)
                            if value == "data:image/png;base64,c21hbGw="
                    )
            })
    )));

    let rollout = agent_rollout::read_rollout(&path).await.unwrap();
    assert!(rollout.iter().any(|item| match item {
        agent_rollout::RolloutItem::EventMsg(EventMsg::ItemCompleted(item))
            if item.item.id() == "media-call" =>
        {
            match &item.item {
                agent_protocol::TurnItem::DynamicToolCall(tool) => {
                    tool.media.first().is_some_and(|asset| {
                        matches!(
                            &asset.reference,
                            types::MediaRef::DataUrl(value)
                                if value == "data:image/png;base64,c21hbGw="
                        )
                    })
                }
                _ => false,
            }
        }
        _ => false,
    }));
}

#[tokio::test]
async fn oversized_inline_media_is_bounded_only_in_completed_event_copy() {
    const COMPLETED_EVENT_MAX_BYTES: usize = 1024 * 1024;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large-media-rollout.jsonl");
    let large_data_url = format!(
        "data:image/png;base64,{}",
        "large-inline-media-sentinel-".repeat(COMPLETED_EVENT_MAX_BYTES / 16)
    );
    let tool_data_url = large_data_url.clone();
    let escape_heavy_text = "\"\\\n".repeat(COMPLETED_EVENT_MAX_BYTES / 8);
    let tool_text = escape_heavy_text.clone();
    let mut agent = AgentLoop::with_session_id(
        AgentConfig::with_defaults(dir.path().to_path_buf()),
        "large-media-tool-session".into(),
    )
    .unwrap();
    agent.tool_registry_mut().register_dynamic(
        types::ToolEntry {
            name: "test_large_media".into(),
            toolset: "test_large_media".into(),
            description: "returns oversized inline media".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            ..types::ToolEntry::lifecycle_defaults()
        },
        Arc::new(move |_name, _args| {
            let data_url = tool_data_url.clone();
            let text = tool_text.clone();
            Box::pin(async move {
                Ok(types::ToolOutput::Media {
                    text,
                    assets: vec![
                        types::MediaAsset {
                            kind: types::MediaKind::Image,
                            mime_type: "image/png".into(),
                            reference: types::MediaRef::DataUrl(data_url),
                            label: Some("inline image".into()),
                            id: Some("inline-asset".into()),
                        },
                        types::MediaAsset {
                            kind: types::MediaKind::Image,
                            mime_type: "image/png".into(),
                            reference: types::MediaRef::RemoteUri(
                                "https://example.test/stable.png".into(),
                            ),
                            label: Some("stable image".into()),
                            id: Some("stable-asset".into()),
                        },
                    ],
                })
            })
        }),
    );
    let session = Arc::new(agent);
    let recorder = agent_rollout::RolloutRecorder::open(
        path.clone(),
        agent_rollout::ThreadHistoryMode::Paginated,
    )
    .await
    .unwrap();
    let thread = agent::AstroThread::spawn(Arc::clone(&session), recorder).unwrap();
    let chat = scripted_chat(vec![
        vec![
            StreamChunk::ToolCallStart {
                index: 0,
                id: "large-media-call".into(),
                name: "test_large_media".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: "{}".into(),
            },
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        vec![
            StreamChunk::Text("done".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let run = tokio::spawn(run_projected_stream_with_chat_fn(
        Arc::clone(&session),
        chat,
        ProviderConfig::default(),
        "system".into(),
        PauseControl::new(),
        None,
        tx,
    ));
    while rx.recv().await.is_some() {}
    run.await.unwrap();

    let mut events = Vec::new();
    loop {
        let event = thread.next_event().await.unwrap();
        let terminal = event.msg.is_terminal();
        events.push(event);
        if terminal {
            break;
        }
    }
    let completed = events
        .iter()
        .find(|event| {
            matches!(
                &event.msg,
                EventMsg::ItemCompleted(item) if item.item.id() == "large-media-call"
            )
        })
        .expect("live completed large-media event");
    assert_bounded_large_media_payload(
        serde_json::to_vec(completed).unwrap(),
        &large_data_url,
        COMPLETED_EVENT_MAX_BYTES,
    );

    let history = session.clone_history().await;
    let recorded_tool = history
        .iter()
        .find(|message| message.role == types::message::Role::Tool)
        .expect("original tool history");
    assert_eq!(recorded_tool.content_str(), escape_heavy_text);
    assert!(recorded_tool.media.iter().any(|asset| matches!(
        &asset.reference,
        types::MediaRef::DataUrl(value) if value == &large_data_url
    )));

    let rollout = agent_rollout::read_rollout(&path).await.unwrap();
    let persisted = rollout
        .iter()
        .find(|item| {
            matches!(
                item,
                agent_rollout::RolloutItem::EventMsg(EventMsg::ItemCompleted(completed))
                    if completed.item.id() == "large-media-call"
            )
        })
        .expect("persisted completed large-media event");
    assert_bounded_large_media_payload(
        serde_json::to_vec(persisted).unwrap(),
        &large_data_url,
        COMPLETED_EVENT_MAX_BYTES,
    );
}

fn assert_bounded_large_media_payload(serialized: Vec<u8>, sentinel: &str, max_bytes: usize) {
    assert!(
        serialized.len() <= max_bytes,
        "completed event was {} bytes",
        serialized.len()
    );
    let serialized = String::from_utf8(serialized).unwrap();
    assert!(serialized.contains("event_payload_truncated"));
    assert!(!serialized.contains(sentinel));
    assert!(serialized.contains("https://example.test/stable.png"));
    assert!(!serialized.contains("data:image/png;base64"));
}

/// 从脚本化轮次列表构造 [`ChatOverride`]。
///
/// 每次调用消费一轮 chunks；轮次用尽后返回默认 "done" 回复。
fn scripted_chat(rounds: Vec<Vec<StreamChunk>>) -> ChatOverride {
    let rounds = Arc::new(tokio::sync::Mutex::new(rounds));
    Arc::new(move |_msgs, _tools, _cfg| {
        let rounds = rounds.clone();
        Box::pin(async move {
            let mut r = rounds.lock().await;
            let chunks = if r.is_empty() {
                vec![
                    StreamChunk::Text("done".into()),
                    StreamChunk::Done {
                        finish_reason: "stop".into(),
                    },
                ]
            } else {
                r.remove(0)
            };
            Ok(Box::pin(futures::stream::iter(
                chunks.into_iter().map(Ok::<_, anyhow::Error>),
            )) as CompletionStream)
        })
    })
}

/// Boom 型 chat override：每次调用均返回错误。
fn boom_chat() -> ChatOverride {
    Arc::new(move |_msgs, _tools, _cfg| Box::pin(async move { Err(anyhow::anyhow!("boom")) }))
}

fn pending_chat() -> ChatOverride {
    Arc::new(move |_msgs, _tools, _cfg| {
        Box::pin(async move { Ok(Box::pin(futures::stream::pending()) as CompletionStream) })
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn regular_task_owns_initial_input_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let session =
        Arc::new(AgentLoop::with_session_id(config, "regular-task-input".into()).unwrap());
    let saw_initial_input = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let chat_fn: ChatOverride = {
        let saw_initial_input = Arc::clone(&saw_initial_input);
        Arc::new(move |messages, _tools, _config| {
            let saw_initial_input = Arc::clone(&saw_initial_input);
            Box::pin(async move {
                saw_initial_input.store(
                    messages
                        .iter()
                        .any(|message| message.text_content() == "owned by regular task"),
                    Ordering::SeqCst,
                );
                Ok(Box::pin(futures::stream::iter(vec![
                    Ok(StreamChunk::Text("done".into())),
                    Ok(StreamChunk::Done {
                        finish_reason: "stop".into(),
                    }),
                ])) as CompletionStream)
            })
        })
    };
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);

    run_projected_stream(ProjectedStreamArgs {
        session: Arc::clone(&session),
        targets: vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: "test".into(),
            api_key: String::new(),
            base_url: String::new(),
        }],
        base_config: ProviderConfig {
            model: "test".into(),
            ..Default::default()
        },
        input: vec![TurnInput {
            content: "owned by regular task".into(),
            image_data_urls: Vec::new(),
            client_message_id: None,
        }],
        system_prompt: None,
        pause: PauseControl::new(),
        hitl_gate: None,
        tx,
        chat_override: Some(chat_fn),
    })
    .await;
    while rx.recv().await.is_some() {}

    assert!(saw_initial_input.load(Ordering::SeqCst));
    let history = session.clone_history().await;
    assert_eq!(history[0].content_str(), "owned by regular task");
}

#[tokio::test]
async fn regular_task_prepare_failure_emits_error_then_done() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = AgentConfig::with_defaults(dir.path().to_path_buf());
    config.max_turns = 0;
    let session =
        Arc::new(AgentLoop::with_session_id(config, "regular-task-prepare-error".into()).unwrap());
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);

    run_projected_stream(ProjectedStreamArgs {
        session,
        targets: vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: "test".into(),
            api_key: String::new(),
            base_url: String::new(),
        }],
        base_config: ProviderConfig::default(),
        input: vec![TurnInput {
            content: "over budget".into(),
            image_data_urls: Vec::new(),
            client_message_id: None,
        }],
        system_prompt: None,
        pause: PauseControl::new(),
        hitl_gate: None,
        tx,
        chat_override: Some(pending_chat()),
    })
    .await;

    let mut items = Vec::new();
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }
    assert!(matches!(
        items.as_slice(),
        [
            ProjectedStreamItem::RunStarted { .. },
            ProjectedStreamItem::Error(message),
            ProjectedStreamItem::RunFinished { outcome_type, .. },
            ProjectedStreamItem::Done,
        ]
            if message.contains("budget exhausted")
                && outcome_type == "error"
    ));
}

#[tokio::test]
async fn regular_task_prepare_error_emits_error_then_done() {
    let dir = tempfile::tempdir().unwrap();
    let session = Arc::new(
        AgentLoop::with_session_id(
            AgentConfig::with_defaults(dir.path().to_path_buf()),
            "regular-task-prepare-error".into(),
        )
        .unwrap(),
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);

    run_projected_stream(ProjectedStreamArgs {
        session,
        targets: vec![types::ChatTarget {
            provider_id: "scripted".into(),
            backend_id: "scripted".into(),
            model: "test".into(),
            api_key: String::new(),
            base_url: String::new(),
        }],
        base_config: ProviderConfig::default(),
        input: Vec::new(),
        system_prompt: None,
        pause: PauseControl::new(),
        hitl_gate: None,
        tx,
        chat_override: Some(pending_chat()),
    })
    .await;

    let mut items = Vec::new();
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }
    assert!(matches!(
        items.as_slice(),
        [
            ProjectedStreamItem::RunStarted { .. },
            ProjectedStreamItem::Error(message),
            ProjectedStreamItem::RunFinished { outcome_type, .. },
            ProjectedStreamItem::Done,
        ]
            if message.contains("requires initial input")
                && outcome_type == "error"
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steered_input_is_consumed_by_the_active_regular_task() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "steer-session".into()).unwrap();
    let session = Arc::new(agent);
    session
        .record_items(vec![types::message::Message::user("initial")])
        .await;

    let calls = Arc::new(AtomicUsize::new(0));
    let saw_follow_up = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let first_started = Arc::new(Notify::new());
    let release_first = Arc::new(Notify::new());
    let chat_fn: ChatOverride = {
        let calls = Arc::clone(&calls);
        let saw_follow_up = Arc::clone(&saw_follow_up);
        let first_started = Arc::clone(&first_started);
        let release_first = Arc::clone(&release_first);
        Arc::new(move |messages, _tools, _config| {
            let calls = Arc::clone(&calls);
            let saw_follow_up = Arc::clone(&saw_follow_up);
            let first_started = Arc::clone(&first_started);
            let release_first = Arc::clone(&release_first);
            Box::pin(async move {
                let call = calls.fetch_add(1, Ordering::SeqCst);
                if call > 0
                    && messages
                        .iter()
                        .any(|message| message.text_content() == "follow up")
                {
                    saw_follow_up.store(true, Ordering::SeqCst);
                }
                if call == 0 {
                    first_started.notify_one();
                    release_first.notified().await;
                }
                let text = if call == 0 { "first" } else { "second" };
                Ok(Box::pin(futures::stream::iter(vec![
                    Ok(StreamChunk::Text(text.into())),
                    Ok(StreamChunk::Done {
                        finish_reason: "stop".into(),
                    }),
                ])) as CompletionStream)
            })
        })
    };
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let run = tokio::spawn({
        let session = Arc::clone(&session);
        async move {
            run_projected_stream_with_chat_fn(
                session,
                chat_fn,
                ProviderConfig {
                    model: "test".into(),
                    ..Default::default()
                },
                "system".into(),
                PauseControl::new(),
                None,
                tx,
            )
            .await;
        }
    });

    first_started.notified().await;
    let turn_id = session
        .steer_input_for_turn("follow up", &[], None, Some("client-steer-follow-up"))
        .await
        .expect("steer admission succeeds")
        .expect("active regular task accepts steer");
    assert!(!turn_id.is_empty());
    release_first.notify_one();
    let mut saw_commit = false;
    while let Some(item) = rx.recv().await {
        if matches!(
            item.unwrap(),
            ProjectedStreamItem::UserInputCommitted(ref client_message_id)
                if client_message_id == "client-steer-follow-up"
        ) {
            saw_commit = true;
        }
    }
    run.await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(saw_follow_up.load(Ordering::SeqCst));
    assert!(
        saw_commit,
        "durably persisted steer must emit its client ack"
    );
    let messages = session.clone_history().await;
    assert!(messages.iter().any(|message| {
        matches!(
            &message.content,
            types::message::MessageContent::Text(text) if text == "follow up"
        )
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_turn_emits_text_tool_result_and_usage() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "test-session".into()).unwrap();
    let session = Arc::new(agent);
    {
        let a = session.as_ref();
        a.record_items(vec![types::message::Message::user("call a tool")])
            .await;
    }

    let chat_fn = scripted_chat(vec![
        vec![
            StreamChunk::Text("thinking…".into()),
            StreamChunk::ToolCallStart {
                index: 0,
                id: "call_1".into(),
                name: "echo".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: r#"{"text":"hi"}"#.into(),
            },
            StreamChunk::Usage(Usage::from_parts(10, 5)),
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        vec![
            StreamChunk::Text("ok".into()),
            StreamChunk::Usage(Usage::from_parts(12, 3)),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            cfg,
            "You are a test agent".into(),
            pause,
            None,
            tx,
        )
        .await;
    });

    let mut items = Vec::new();
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }

    assert!(matches!(
        items.first(),
        Some(ProjectedStreamItem::RunStarted { .. })
    ));
    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::Assistant(StreamedAssistantContent::Text(t)) if t == "thinking…"
    )));
    let tool_started_index = items
        .iter()
        .position(|i| matches!(i, ProjectedStreamItem::ToolStarted { name, .. } if name == "echo"))
        .expect("tool started event");
    let tool_completed_index = items
        .iter()
        .position(|i| matches!(i, ProjectedStreamItem::ToolResult { name, .. } if name == "echo"))
        .expect("tool completed event");
    assert!(tool_started_index < tool_completed_index);
    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::Assistant(StreamedAssistantContent::FinalUsage(u))
        if u.prompt_tokens() == 22 && u.completion_tokens() == 8
    )));
    assert_eq!(
        items
            .iter()
            .filter(|item| matches!(
                item,
                ProjectedStreamItem::Assistant(StreamedAssistantContent::FinalUsage(_))
            ))
            .count(),
        1,
        "context snapshots must not be exposed as billing usage"
    );
    let context_snapshots = items
        .iter()
        .filter_map(|item| match item {
            ProjectedStreamItem::ContextUsage(snapshot) => Some(snapshot),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!context_snapshots.is_empty());
    assert!(context_snapshots
        .iter()
        .all(|snapshot| !snapshot.segments.is_empty()));
    assert!(context_snapshots.iter().any(|snapshot| snapshot
        .segments
        .iter()
        .any(|segment| !segment.items.is_empty())));
    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::RunFinished {
            outcome_type,
            ..
        } if outcome_type == "success"
    )));
    assert!(matches!(items.last(), Some(ProjectedStreamItem::Done)));
}

#[tokio::test]
async fn multi_turn_tool_exec_works_on_current_thread_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "current-thread-session".into()).unwrap();
    let session = Arc::new(agent);
    session
        .record_items(vec![types::message::Message::user("call a tool")])
        .await;

    let chat_fn = scripted_chat(vec![
        vec![
            StreamChunk::ToolCallStart {
                index: 0,
                id: "call_current_thread".into(),
                name: "echo".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: r#"{"text":"safe"}"#.into(),
            },
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        vec![
            StreamChunk::Text("current-thread ok".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);

    run_projected_stream_with_chat_fn(
        session,
        chat_fn,
        ProviderConfig {
            model: "test".into(),
            ..Default::default()
        },
        "You are a test agent".into(),
        PauseControl::new(),
        None,
        tx,
    )
    .await;

    let mut items = Vec::new();
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }
    assert!(items.iter().any(
        |item| matches!(item, ProjectedStreamItem::ToolResult { name, .. } if name == "echo")
    ));
    assert!(items.iter().any(|item| matches!(
        item,
        ProjectedStreamItem::Assistant(StreamedAssistantContent::Text(text))
            if text == "current-thread ok"
    )));
    assert!(matches!(items.last(), Some(ProjectedStreamItem::Done)));
}

#[tokio::test]
async fn cold_start_hydrates_history_from_db() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    {
        let agent = AgentLoop::with_session_id(
            AgentConfig::with_defaults(path.clone()),
            "hydrate-me".into(),
        )
        .unwrap();
        agent.ensure_session("test").unwrap();
        agent.start_or_steer_turn("hello", "hydrate").await.unwrap();
        agent.record_assistant_message("world").await.unwrap();
    }

    let agent =
        AgentLoop::with_session_id(AgentConfig::with_defaults(path), "hydrate-me".into()).unwrap();
    let history = agent.clone_history().await;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].content_str(), "hello");
    assert_eq!(history[1].content_str(), "world");
}

#[tokio::test]
async fn cold_start_preserves_projected_user_audio_and_video_media_kinds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let image_url = "data:image/png;base64,aW1hZ2U=";
    let audio_url = "data:audio/mpeg;base64,YXVkaW8=";
    let video_url = "data:video/webm;base64,dmlkZW8=";
    let projected_media = vec![
        types::MediaAsset::data_url(types::MediaKind::Image, image_url, "image/png"),
        types::MediaAsset::data_url(types::MediaKind::Audio, audio_url, "audio/mpeg"),
        types::MediaAsset::data_url(types::MediaKind::Video, video_url, "video/webm"),
    ];
    let mut user = types::message::Message::user("inspect cold media");
    user.media = projected_media.clone();
    {
        let store = session::SessionStore::open_sessions_dir(&path.join("sessions")).unwrap();
        session::store::rebuild_messages_from_rollout(
            &store,
            "hydrate-media",
            &[agent_rollout::RolloutItem::ResponseItem(user)],
        )
        .unwrap();
    }

    let agent =
        AgentLoop::with_session_id(AgentConfig::with_defaults(path), "hydrate-media".into())
            .unwrap();
    let history = agent.clone_history().await;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].media, projected_media);
    assert!(matches!(
        &history[0].content,
        types::message::MessageContent::Parts(parts)
            if parts.iter().filter(|part| part.kind == "image_url").count() == 1
                && parts.iter().any(|part| {
                    part.image_url.as_ref().is_some_and(|image| image.url == image_url)
                })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_turn_persists_reasoning_and_tool_activities() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "persist-session".into()).unwrap();
    let session = Arc::new(agent);
    {
        let a = session.as_ref();
        a.ensure_session("test").unwrap();
        a.record_items(vec![types::message::Message::user("call a tool")])
            .await;
    }

    let chat_fn = scripted_chat(vec![
        vec![
            StreamChunk::Thinking("deep ".into()),
            StreamChunk::Thinking("thought".into()),
            StreamChunk::Text("calling…".into()),
            StreamChunk::ToolCallStart {
                index: 0,
                id: "call_persist".into(),
                name: "echo".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: r#"{"text":"hi"}"#.into(),
            },
            StreamChunk::Usage(Usage::from_parts(10, 5)),
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        vec![
            StreamChunk::Text("ok".into()),
            StreamChunk::Usage(Usage::from_parts(4, 2)),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            cfg,
            "You are a test agent".into(),
            pause,
            None,
            tx,
        )
        .await;
    });

    while let Some(item) = rx.recv().await {
        item.unwrap();
    }

    let store = session::SessionStore::open(&dir.path().join("sessions/state.db")).unwrap();
    let hist = store.build_chat_history("persist-session", 50).unwrap();
    assert!(
        hist.iter()
            .any(|m| m.reasoning.as_deref() == Some("deep thought")),
        "expected persisted reasoning; hist={hist:?}"
    );
    assert!(
        hist.iter().any(|m| !m.activities.is_empty()),
        "expected tool activities; hist={hist:?}"
    );
    let activity = hist
        .iter()
        .find(|m| !m.activities.is_empty())
        .unwrap()
        .activities
        .first()
        .unwrap();
    assert_eq!(activity.id, "call_persist");
    assert_eq!(activity.title, "echo");
    assert!(activity.output.is_some(), "expected tool result output");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_turn_fires_post_llm_call_after_model_stream() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "completion-hook".into()).unwrap();
    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    ::hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));
    agent
        .record_items(vec![types::message::Message::user("say hi")])
        .await;
    let session = Arc::new(agent);

    let chat_fn = scripted_chat(vec![vec![
        StreamChunk::Text("hello".into()),
        StreamChunk::Usage(Usage::from_parts(3, 2)),
        StreamChunk::Done {
            finish_reason: "stop".into(),
        },
    ]]);

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            cfg,
            "You are a test agent".into(),
            pause,
            None,
            tx,
        )
        .await;
    });

    while let Some(item) = rx.recv().await {
        item.unwrap();
    }

    let events = log.lock().unwrap().clone();
    assert!(
        events.iter().any(|e| e == "PostLlmCall:5"),
        "expected post_llm_call for \"hello\" (5 chars), events={events:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transform_llm_output_replaces_before_post_llm_call() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "transform-llm-session".into()).unwrap();
    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    ::hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));
    agent
        .hook_bus()
        .register(::hooks::TRANSFORM_LLM_OUTPUT, |_| {
            ::hooks::HookOutcome::ReplaceText("REPLACED".into())
        });
    agent
        .record_items(vec![types::message::Message::user("say hi")])
        .await;
    let session = Arc::new(agent);
    let session_for_check = Arc::clone(&session);

    let chat_fn = scripted_chat(vec![vec![
        StreamChunk::Text("hello world".into()),
        StreamChunk::Usage(Usage::from_parts(3, 2)),
        StreamChunk::Done {
            finish_reason: "stop".into(),
        },
    ]]);

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            cfg,
            "You are a test agent".into(),
            pause,
            None,
            tx,
        )
        .await;
    });

    while let Some(item) = rx.recv().await {
        item.unwrap();
    }

    let events = log.lock().unwrap().clone();
    let transform_idx = events.iter().position(|e| e == "TransformLlmOutput:11");
    let post_idx = events.iter().position(|e| e == "PostLlmCall:8");
    assert!(
        transform_idx.is_some() && post_idx.is_some() && transform_idx < post_idx,
        "expected transform_llm_output before post_llm_call with replaced length, events={events:?}"
    );

    let agent = session_for_check.as_ref();
    let history = agent.clone_history().await;
    assert_eq!(
        history.last().map(|m| m.content_str().to_string()),
        Some("REPLACED".to_string()),
        "final assistant message should reflect transform_llm_output replacement"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_fires_at_terminal_boundary_without_disk_write() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "pre-verify-no-write".into()).unwrap();
    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    ::hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));
    agent
        .record_items(vec![types::message::Message::user("just say hi, no tools")])
        .await;
    let session = Arc::new(agent);

    let chat_fn = scripted_chat(vec![vec![
        StreamChunk::Text("hi there".into()),
        StreamChunk::Done {
            finish_reason: "stop".into(),
        },
    ]]);

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            cfg,
            "You are a test agent".into(),
            pause,
            None,
            tx,
        )
        .await;
    });

    let mut items = Vec::new();
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }

    let events = log.lock().unwrap().clone();
    assert!(
        events.iter().filter(|e| e.as_str() == "Stop").count() == 1,
        "Stop must fire at the terminal boundary even when no disk write happened, events={events:?}"
    );
    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::RunFinished { outcome_type, .. } if outcome_type == "success"
    )));
    assert!(matches!(items.last(), Some(ProjectedStreamItem::Done)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_verify_keep_going_retries_capped_at_two() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "pre-verify-keep-going".into()).unwrap();
    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    ::hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));
    agent.hook_bus().register(::hooks::PRE_VERIFY, |_| {
        ::hooks::HookOutcome::KeepGoing("请再检查一下你的改动".into())
    });
    agent
        .record_items(vec![types::message::Message::user(
            "write a file then confirm",
        )])
        .await;
    let session = Arc::new(agent);
    let session_for_check = Arc::clone(&session);

    let chat_fn = scripted_chat(vec![
        // round 1: 写盘工具调用，置位 turn_wrote_disk
        vec![
            StreamChunk::ToolCallStart {
                index: 0,
                id: "call_write".into(),
                name: "file_ops".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: r#"{"path":"verify.txt","operation":"write","content":"hi"}"#.into(),
            },
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        // round 2: 无工具终态草稿一 -> pre_verify attempt 1 -> KeepGoing
        vec![
            StreamChunk::Text("draft one".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
        // round 3: 无工具终态草稿二 -> pre_verify attempt 2 -> KeepGoing
        vec![
            StreamChunk::Text("draft two".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
        // round 4: 尝试次数已达上限，直接收尾
        vec![
            StreamChunk::Text("final answer".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            cfg,
            "You are a test agent".into(),
            pause,
            None,
            tx,
        )
        .await;
    });

    let mut items = Vec::new();
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }

    let events = log.lock().unwrap().clone();
    let verify_count = events.iter().filter(|e| e.as_str() == "Stop").count();
    assert_eq!(
        verify_count, 3,
        "Stop fires for two keep-going attempts and the final capped boundary, events={events:?}"
    );
    let api_request_count = events
        .iter()
        .filter(|e| e.as_str() == "PreApiRequest")
        .count();
    assert_eq!(
        api_request_count, 4,
        "expect one pre_api_request round per LLM call (1 tool round + 2 keep-going + 1 final), events={events:?}"
    );
    let post_llm_count = events
        .iter()
        .filter(|e| e.starts_with("PostLlmCall"))
        .count();
    assert_eq!(
        post_llm_count, 2,
        "post_llm_call must be skipped while pre_verify keeps going; only the tool round and the final round should fire it, events={events:?}"
    );

    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::Assistant(StreamedAssistantContent::Text(t)) if t == "final answer"
    )));
    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::RunFinished { outcome_type, .. } if outcome_type == "success"
    )));
    assert!(matches!(items.last(), Some(ProjectedStreamItem::Done)));

    let agent = session_for_check.as_ref();
    let history = agent.clone_history().await;
    assert!(
        agent::runtime::validate_message_order(&history),
        "history must alternate roles (no consecutive same role) after capped KeepGoing retries, messages={:?}",
        history
            .iter()
            .map(|m| (m.role.clone(), m.content_str().to_string()))
            .collect::<Vec<_>>()
    );

    let bridge_users: Vec<&types::message::Message> = history
        .iter()
        .filter(|m| {
            m.role == types::message::Role::User
                && m.content_str().starts_with("[astro:hook-context]")
        })
        .collect();
    assert_eq!(
        bridge_users.len(),
        2,
        "expect one persisted bridging user message per KeepGoing attempt (capped at 2), messages={:?}",
        history
            .iter()
            .map(|m| (m.role.clone(), m.content_str().to_string()))
            .collect::<Vec<_>>()
    );
    for m in &bridge_users {
        assert_eq!(
            m.content_str(),
            "[astro:hook-context]\n请再检查一下你的改动",
            "persisted bridging message text must match the inject format used at multi_turn.rs"
        );
    }

    let provider_messages = agent::prompt::messages::to_provider_messages("sys", &history);
    for window in provider_messages.windows(2) {
        let (a, b) = (window[0].role(), window[1].role());
        assert!(
            !(a == providers::types::message::Role::Assistant
                && b == providers::types::message::Role::Assistant),
            "to_provider_messages must not contain consecutive assistant entries"
        );
        assert!(
            !(a == providers::types::message::Role::User
                && b == providers::types::message::Role::User),
            "to_provider_messages must not contain consecutive user entries"
        );
    }
}

#[tokio::test]
async fn pause_control_blocks_then_cancels() {
    let pause = PauseControl::new();
    pause.pause();
    let p2 = pause.clone();
    let handle = tokio::spawn(async move { p2.wait_if_paused().await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(!handle.is_finished());
    pause.cancel();
    assert!(!handle.await.unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cumulative_usage_chunks_use_last_per_round() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "usage-session".into()).unwrap();
    let session = Arc::new(agent);
    {
        let a = session.as_ref();
        a.record_items(vec![types::message::Message::user("hi")])
            .await;
    }
    let chat_fn = scripted_chat(vec![vec![
        StreamChunk::Text("a".into()),
        StreamChunk::Usage(Usage::from_parts(1, 1)),
        StreamChunk::Text("b".into()),
        StreamChunk::Usage(Usage::from_parts(10, 5)),
        StreamChunk::Done {
            finish_reason: "stop".into(),
        },
    ]]);
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            ProviderConfig {
                model: "test".into(),
                ..Default::default()
            },
            "sys".into(),
            pause,
            None,
            tx,
        )
        .await;
    });
    let mut items = Vec::new();
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }
    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::Assistant(StreamedAssistantContent::FinalUsage(u))
        if u.prompt_tokens() == 10 && u.completion_tokens() == 5
    )));
    assert!(matches!(items.last(), Some(ProjectedStreamItem::Done)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn error_has_single_error_terminal_before_done() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "err-session".into()).unwrap();
    let session = Arc::new(agent);
    {
        let a = session.as_ref();
        a.record_items(vec![types::message::Message::user("x")])
            .await;
    }
    let chat_fn = boom_chat();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            ProviderConfig {
                model: "test".into(),
                ..Default::default()
            },
            "sys".into(),
            pause,
            None,
            tx,
        )
        .await;
    });
    let mut items = Vec::new();
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }
    assert!(matches!(
        items.first(),
        Some(ProjectedStreamItem::RunStarted { .. })
    ));
    assert!(items
        .iter()
        .any(|i| matches!(i, ProjectedStreamItem::Error(_))));
    let terminal_outcomes: Vec<&str> = items
        .iter()
        .filter_map(|item| match item {
            ProjectedStreamItem::RunFinished { outcome_type, .. } => Some(outcome_type.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(terminal_outcomes, ["error"]);
    assert!(matches!(items.last(), Some(ProjectedStreamItem::Done)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_has_single_interrupt_terminal_before_done() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "cancel-session".into()).unwrap();
    let session = Arc::new(agent);
    {
        let agent = session.as_ref();
        agent
            .record_items(vec![types::message::Message::user("wait")])
            .await;
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let run_pause = pause.clone();
    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            pending_chat(),
            ProviderConfig {
                model: "test".into(),
                ..Default::default()
            },
            "sys".into(),
            run_pause,
            None,
            tx,
        )
        .await;
    });

    let started = rx.recv().await.unwrap().unwrap();
    assert!(matches!(started, ProjectedStreamItem::RunStarted { .. }));
    pause.cancel();

    let mut items = vec![started];
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }
    assert!(!items
        .iter()
        .any(|item| matches!(item, ProjectedStreamItem::Error(_))));
    let terminal_outcomes: Vec<&str> = items
        .iter()
        .filter_map(|item| match item {
            ProjectedStreamItem::RunFinished { outcome_type, .. } => Some(outcome_type.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(terminal_outcomes, ["interrupt"]);
    assert!(matches!(items.last(), Some(ProjectedStreamItem::Done)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_call_delta_and_memory_path() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "mem-session".into()).unwrap();
    let session = Arc::new(agent);
    {
        let a = session.as_ref();
        a.record_items(vec![types::message::Message::user("remember this")])
            .await;
    }

    let chat_fn = scripted_chat(vec![
        vec![
            StreamChunk::ToolCallStart {
                index: 0,
                id: "c1".into(),
                name: "memory".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: r#"{"action":"add","content":""#.into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: r#"hello from test","target":"memory"}"#.into(),
            },
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        vec![
            StreamChunk::Text("saved".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            ProviderConfig {
                model: "test".into(),
                ..Default::default()
            },
            "sys".into(),
            pause,
            None,
            tx,
        )
        .await;
    });

    let mut items = Vec::new();
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }

    let delta_count = items
        .iter()
        .filter(|i| {
            matches!(
                i,
                ProjectedStreamItem::Assistant(StreamedAssistantContent::ToolCallDelta(_))
            )
        })
        .count();
    assert!(delta_count >= 2, "expected streamed tool_call_deltas");
    assert!(items
        .iter()
        .any(|i| matches!(i, ProjectedStreamItem::ToolResult { name, .. } if name == "memory")));
    assert!(
        items.iter().any(|i| {
            matches!(i, ProjectedStreamItem::MemoryUpdate { op, .. } if op == "memory")
        }),
        "memory success should emit MemoryUpdate; got: {:?}",
        items.iter().map(|i| format!("{i:?}")).collect::<Vec<_>>()
    );
    assert!(matches!(items.last(), Some(ProjectedStreamItem::Done)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hitl_waiting_parks_then_continues_same_run() {
    use agent::{HitlGate, ResumeItem};

    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "hitl-session".into()).unwrap();
    let session = Arc::new(agent);
    {
        let a = session.as_ref();
        a.record_items(vec![types::message::Message::user("please confirm")])
            .await;
    }

    let chat_fn = scripted_chat(vec![
        vec![
            StreamChunk::ToolCallStart {
                index: 0,
                id: "call_confirm".into(),
                name: "ask_user".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments:
                    r#"{"mode":"confirm","title":"Delete?","body":"Really delete the file?"}"#
                        .into(),
            },
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        vec![
            StreamChunk::Text("confirmed".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);

    let gate = HitlGate::new("hitl-session");
    let gate_resolve = gate.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();

    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            ProviderConfig {
                model: "test".into(),
                ..Default::default()
            },
            "sys".into(),
            pause,
            Some(gate),
            tx,
        )
        .await;
    });

    let mut saw_waiting = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            Ok(Some(Ok(ProjectedStreamItem::RunFinished {
                outcome_type,
                interrupts_json,
                ..
            }))) if outcome_type == "hitl_waiting" => {
                saw_waiting = true;
                let interrupts: Vec<serde_json::Value> =
                    serde_json::from_str(&interrupts_json).unwrap();
                let id = interrupts[0]["id"].as_str().unwrap().to_string();
                gate_resolve
                    .resolve(&[ResumeItem {
                        interrupt_id: id,
                        status: "resolved".into(),
                        payload_json: r#"{"approved":true}"#.into(),
                    }])
                    .await
                    .unwrap();
                break;
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => panic!("stream err: {e}"),
            Ok(None) => panic!("stream ended before hitl_waiting"),
            Err(_) => continue,
        }
    }
    assert!(saw_waiting, "expected hitl_waiting");

    let mut items = Vec::new();
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }
    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::ToolResult { name, result, .. }
        if name == "ask_user" && result.contains("approved")
    )));
    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::Assistant(StreamedAssistantContent::Text(t)) if t == "confirmed"
    )));
    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::RunFinished { outcome_type, .. } if outcome_type == "success"
    )));
    assert!(matches!(items.last(), Some(ProjectedStreamItem::Done)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn approval_hooks_fire_pre_then_post_on_allow() {
    type CapturedApproval = Arc<std::sync::Mutex<Option<(Option<String>, String)>>>;
    use agent::{HitlGate, ResumeItem};

    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "approval-allow-session".into()).unwrap();

    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));

    let captured_pre: CapturedApproval = Arc::new(std::sync::Mutex::new(None));
    let captured_pre2 = Arc::clone(&captured_pre);
    agent
        .hook_bus()
        .register(hooks::PRE_APPROVAL_REQUEST, move |payload| {
            let command = payload
                .tool_input
                .as_ref()
                .and_then(|input| input.get("command").or_else(|| input.get("summary")))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            *captured_pre2.lock().unwrap() = Some((command, payload.detail.clone()));
            hooks::HookOutcome::Continue
        });
    let captured_post: CapturedApproval = Arc::new(std::sync::Mutex::new(None));
    let captured_post2 = Arc::clone(&captured_post);
    agent
        .hook_bus()
        .register(hooks::POST_APPROVAL_RESPONSE, move |payload| {
            let command = payload
                .tool_input
                .as_ref()
                .and_then(|input| input.get("command").or_else(|| input.get("summary")))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            *captured_post2.lock().unwrap() = Some((command, payload.detail.clone()));
            hooks::HookOutcome::Continue
        });

    let session = Arc::new(agent);
    {
        let a = session.as_ref();
        a.record_items(vec![types::message::Message::user("clean up the temp dir")])
            .await;
    }

    let cmd = "rm -rf /tmp/astro-approval-test-allow";
    let chat_fn = scripted_chat(vec![
        vec![
            StreamChunk::ToolCallStart {
                index: 0,
                id: "call_term_allow".into(),
                name: "terminal".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: format!(r#"{{"command":"{cmd}"}}"#),
            },
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        vec![
            StreamChunk::Text("done".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);

    let gate = HitlGate::new("approval-allow-session");
    let gate_resolve = gate.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();

    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            ProviderConfig {
                model: "test".into(),
                ..Default::default()
            },
            "sys".into(),
            pause,
            Some(gate),
            tx,
        )
        .await;
    });

    let mut saw_waiting = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            Ok(Some(Ok(ProjectedStreamItem::RunFinished {
                outcome_type,
                interrupts_json,
                ..
            }))) if outcome_type == "hitl_waiting" => {
                saw_waiting = true;
                let interrupts: Vec<serde_json::Value> =
                    serde_json::from_str(&interrupts_json).unwrap();
                let id = interrupts[0]["id"].as_str().unwrap().to_string();
                gate_resolve
                    .resolve(&[ResumeItem {
                        interrupt_id: id,
                        status: "resolved".into(),
                        payload_json: r#"{"approved":true}"#.into(),
                    }])
                    .await
                    .unwrap();
                break;
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => panic!("stream err: {e}"),
            Ok(None) => panic!("stream ended before hitl_waiting"),
            Err(_) => continue,
        }
    }
    assert!(saw_waiting, "expected hitl_waiting for dangerous command");

    while let Some(item) = rx.recv().await {
        item.unwrap();
    }

    let events = log.lock().unwrap().clone();
    let pre_idx = events.iter().position(|e| e == "PermissionRequest:Bash");
    let post_idx = events.iter().position(|e| e == "PostApprovalResponse:Bash");
    assert!(
        pre_idx.is_some() && post_idx.is_some() && pre_idx < post_idx,
        "expected pre_approval_request before post_approval_response, events={events:?}"
    );
    let post_tool_idx = events.iter().position(|e| e == "PostToolUse:terminal");
    assert!(
        post_idx < post_tool_idx,
        "expected post_approval_response before post_tool_call, events={events:?}"
    );

    let pre = captured_pre
        .lock()
        .unwrap()
        .clone()
        .expect("pre_approval_request payload captured");
    assert_eq!(
        pre.0.as_deref(),
        Some(cmd),
        "pre payload.message should be the command"
    );
    assert!(
        pre.1.contains("surface=terminal") && pre.1.contains("ask="),
        "pre detail should include surface/ask: {}",
        pre.1
    );

    let post = captured_post
        .lock()
        .unwrap()
        .clone()
        .expect("post_approval_response payload captured");
    assert_eq!(
        post.0.as_deref(),
        Some(cmd),
        "post payload.message should be the command"
    );
    assert!(
        post.1.contains("surface=terminal") && post.1.contains("choice=allow"),
        "post detail should include surface/choice=allow: {}",
        post.1
    );

    let audits = memory::list_recent_permission_audits(dir.path(), 20).unwrap();
    for expected in [
        memory::PermissionAuditKind::Requested,
        memory::PermissionAuditKind::Reviewed,
        memory::PermissionAuditKind::Granted,
        memory::PermissionAuditKind::Applied,
    ] {
        assert!(
            audits.iter().any(|event| event.event == expected),
            "missing {expected:?} in {audits:?}"
        );
    }
    let raw_audit = std::fs::read_to_string(memory::permission_audit_path(dir.path())).unwrap();
    assert!(!raw_audit.contains(cmd), "audit must omit command text");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn approval_hooks_fire_pre_then_post_on_deny() {
    use agent::{HitlGate, ResumeItem};

    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "approval-deny-session".into()).unwrap();

    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));

    let captured_post: Arc<std::sync::Mutex<Option<String>>> =
        Arc::new(std::sync::Mutex::new(None));
    let captured_post2 = Arc::clone(&captured_post);
    agent
        .hook_bus()
        .register(hooks::POST_APPROVAL_RESPONSE, move |payload| {
            *captured_post2.lock().unwrap() = Some(payload.detail.clone());
            hooks::HookOutcome::Continue
        });

    let session = Arc::new(agent);
    {
        let a = session.as_ref();
        a.record_items(vec![types::message::Message::user("clean up the temp dir")])
            .await;
    }

    let cmd = "rm -rf /tmp/astro-approval-test-deny";
    let chat_fn = scripted_chat(vec![
        vec![
            StreamChunk::ToolCallStart {
                index: 0,
                id: "call_term_deny".into(),
                name: "terminal".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: format!(r#"{{"command":"{cmd}"}}"#),
            },
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        vec![
            StreamChunk::Text("acknowledged".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);

    let gate = HitlGate::new("approval-deny-session");
    let gate_resolve = gate.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();

    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            ProviderConfig {
                model: "test".into(),
                ..Default::default()
            },
            "sys".into(),
            pause,
            Some(gate),
            tx,
        )
        .await;
    });

    let mut saw_waiting = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            Ok(Some(Ok(ProjectedStreamItem::RunFinished {
                outcome_type,
                interrupts_json,
                ..
            }))) if outcome_type == "hitl_waiting" => {
                saw_waiting = true;
                let interrupts: Vec<serde_json::Value> =
                    serde_json::from_str(&interrupts_json).unwrap();
                let id = interrupts[0]["id"].as_str().unwrap().to_string();
                gate_resolve
                    .resolve(&[ResumeItem {
                        interrupt_id: id,
                        status: "resolved".into(),
                        payload_json: r#"{"approved":false}"#.into(),
                    }])
                    .await
                    .unwrap();
                break;
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => panic!("stream err: {e}"),
            Ok(None) => panic!("stream ended before hitl_waiting"),
            Err(_) => continue,
        }
    }
    assert!(saw_waiting, "expected hitl_waiting for dangerous command");

    let mut items = Vec::new();
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }

    let events = log.lock().unwrap().clone();
    let pre_idx = events.iter().position(|e| e == "PermissionRequest:Bash");
    let post_idx = events.iter().position(|e| e == "PostApprovalResponse:Bash");
    assert!(
        pre_idx.is_some() && post_idx.is_some() && pre_idx < post_idx,
        "expected pre_approval_request before post_approval_response, events={events:?}"
    );

    let post_detail = captured_post
        .lock()
        .unwrap()
        .clone()
        .expect("post_approval_response payload captured");
    assert!(
        post_detail.contains("choice=deny"),
        "post detail should include choice=deny: {post_detail}"
    );

    assert!(
        items.iter().any(|i| matches!(
            i,
            ProjectedStreamItem::ToolResult { name, result, .. }
            if name == "terminal" && result.contains("denied by user")
        )),
        "expected denial tool result; got: {:?}",
        items.iter().map(|i| format!("{i:?}")).collect::<Vec<_>>()
    );

    let audits = memory::list_recent_permission_audits(dir.path(), 20).unwrap();
    assert!(audits
        .iter()
        .any(|event| event.event == memory::PermissionAuditKind::Denied));
    assert!(
        !audits
            .iter()
            .any(|event| event.event == memory::PermissionAuditKind::Applied),
        "denied permission must not be recorded as applied"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_turn_budget_exhausted_forces_toolless_summary() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = AgentConfig::with_defaults(dir.path().to_path_buf());
    config.multi_turn = 1;
    let agent = AgentLoop::with_session_id(config, "budget-session".into()).unwrap();
    let session = Arc::new(agent);
    let recorder = agent_rollout::RolloutRecorder::open(
        dir.path().join("budget-rollout.jsonl"),
        agent_rollout::ThreadHistoryMode::Paginated,
    )
    .await
    .unwrap();
    let thread = agent::AstroThread::spawn(Arc::clone(&session), recorder).unwrap();
    {
        let a = session.as_ref();
        a.record_items(vec![types::message::Message::user("keep using tools")])
            .await;
    }

    let chat_fn = scripted_chat(vec![
        vec![
            StreamChunk::ToolCallStart {
                index: 0,
                id: "call_b".into(),
                name: "echo".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: r#"{"text":"x"}"#.into(),
            },
            StreamChunk::Usage(Usage::from_parts(5, 2)),
            StreamChunk::Done {
                finish_reason: "tool_calls".into(),
            },
        ],
        // 预算耗尽后的无工具总结轮
        vec![
            StreamChunk::Thinking("summary-reasoning".into()),
            StreamChunk::Text("summary-after-budget".into()),
            StreamChunk::Usage(Usage::from_parts(6, 4)),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
    ]);

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    tokio::spawn(async move {
        run_projected_stream_with_chat_fn(
            session,
            chat_fn,
            ProviderConfig {
                model: "test".into(),
                ..Default::default()
            },
            "sys".into(),
            pause,
            None,
            tx,
        )
        .await;
    });

    let mut items = Vec::new();
    while let Some(item) = rx.recv().await {
        items.push(item.expect("stream item"));
    }
    let mut events = Vec::new();
    loop {
        let event = thread.next_event().await.unwrap();
        let terminal = event.msg.is_terminal();
        events.push(event);
        if terminal {
            break;
        }
    }

    assert!(
        !items
            .iter()
            .any(|i| matches!(i, ProjectedStreamItem::Error(e) if e.contains("轮次已用尽"))),
        "budget exhaustion should not hard-error"
    );
    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::Assistant(StreamedAssistantContent::Text(t))
        if t.contains("迭代预算已用尽")
    )));
    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::Assistant(StreamedAssistantContent::Text(t))
        if t == "summary-after-budget"
    )));
    assert!(items.iter().any(|i| matches!(
        i,
        ProjectedStreamItem::RunFinished { outcome_type, .. } if outcome_type == "success"
    )));
    assert!(matches!(items.last(), Some(ProjectedStreamItem::Done)));
    assert_text_item_lifecycle(&events, false, "summary-after-budget");
    assert_text_item_lifecycle(&events, true, "summary-reasoning");
}
