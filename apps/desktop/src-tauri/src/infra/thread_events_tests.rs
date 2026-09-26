//! `thread_events.rs` 的单元测试（原内联 mod tests 拆出）。

use super::*;

#[test]
fn managed_bridge_state_type_matches_startup_registration() {
    assert_eq!(
        managed_bridge_state_type_id(),
        std::any::TypeId::of::<Arc<ThreadEventsBridge>>()
    );
}
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn terminal_event(thread_id: &str, turn_id: &str) -> proto::ThreadEvent {
    proto::ThreadEvent {
        thread_id: thread_id.into(),
        turn_id: turn_id.into(),
        payload: Some(proto::thread_event::Payload::TurnComplete(
            proto::ThreadTurnComplete {
                last_agent_message: "done".into(),
                error: None,
                has_error: false,
            },
        )),
    }
}

fn terminal_projection(turn_id: &str) -> Vec<ChatStreamEvent> {
    map_thread_event(terminal_event("session-1", turn_id))
}

#[test]
fn token_count_projection_preserves_cache_reasoning_and_reporting_state() {
    let projected = map_thread_event(proto::ThreadEvent {
        thread_id: "session-1".into(),
        turn_id: "turn-1".into(),
        payload: Some(proto::thread_event::Payload::TokenCount(
            proto::ThreadTokenCount {
                input_tokens: 100,
                output_tokens: 25,
                total_tokens: 125,
                uncached_input_tokens: 60,
                cache_read_tokens: 40,
                cache_write_tokens: 0,
                reasoning_tokens: 20,
                request_count: 1,
                provider_total_tokens: 125,
                provider_total_tokens_reported: true,
                cache_read_reported: true,
                cache_write_reported: false,
                reasoning_reported: true,
                input_tokens_include_cache: true,
            },
        )),
    });

    assert!(matches!(
        projected.as_slice(),
        [ChatStreamEvent::Usage {
            prompt_tokens: 100,
            uncached_input_tokens: 60,
            completion_tokens: 25,
            total_tokens: 125,
            cache_read_tokens: 40,
            reasoning_tokens: 20,
            provider_total_tokens: Some(125),
            cache_read_reported: true,
            reasoning_reported: true,
            ..
        }]
    ));

    let legacy = map_thread_event(proto::ThreadEvent {
        thread_id: "session-1".into(),
        turn_id: "turn-legacy".into(),
        payload: Some(proto::thread_event::Payload::TokenCount(
            proto::ThreadTokenCount {
                input_tokens: 60,
                output_tokens: 25,
                total_tokens: 125,
                cache_read_tokens: 40,
                ..Default::default()
            },
        )),
    });
    assert!(matches!(
        legacy.as_slice(),
        [ChatStreamEvent::Usage {
            prompt_tokens: 100,
            uncached_input_tokens: 60,
            ..
        }]
    ));
}

#[test]
fn context_usage_projection_preserves_actual_source_and_breakdown() {
    let projected = context_usage_event(
        r#"{"context_window":128000,"total_tokens":125,"estimated_total_tokens":120,"source":"provider_reported","latest_usage":{"input_tokens":100,"output_tokens":25,"total_tokens":125},"segments":[],"updated_at":7}"#,
    );

    assert!(matches!(
        projected,
        ChatStreamEvent::ContextUsage {
            total_tokens: 125,
            estimated_total_tokens: 120,
            ref source,
            latest_usage: Some(_),
            ..
        } if source == "provider_reported"
    ));
}

fn agent_message_item(id: &str, content: &str) -> proto::ThreadItem {
    proto::ThreadItem {
        id: id.into(),
        item_type: "agent_message".into(),
        status: "completed".into(),
        payload_json: serde_json::to_string(&TurnItem::AgentMessage(
            agent_protocol::AgentMessageItem {
                id: id.into(),
                content: content.into(),
                delivery: None,
                questions: None,
            },
        ))
        .unwrap(),
    }
}

fn async_message_item(id: &str, content: &str) -> proto::ThreadItem {
    proto::ThreadItem {
        id: id.into(),
        item_type: "agent_message".into(),
        status: "completed".into(),
        payload_json: serde_json::to_string(&TurnItem::AgentMessage(
            agent_protocol::AgentMessageItem {
                id: id.into(),
                content: content.into(),
                delivery: Some(agent_protocol::AgentMessageDelivery::Async),
                questions: Some(vec![agent_protocol::AsyncUserInputQuestion {
                    title: "Choose a target".into(),
                    options: Some(vec!["A".into(), "B".into()]),
                }]),
            },
        ))
        .unwrap(),
    }
}

fn reasoning_item(id: &str, content: &str) -> proto::ThreadItem {
    proto::ThreadItem {
        id: id.into(),
        item_type: "reasoning".into(),
        status: "in_progress".into(),
        payload_json: serde_json::to_string(&TurnItem::Reasoning(agent_protocol::TextItem {
            id: id.into(),
            content: content.into(),
        }))
        .unwrap(),
    }
}

async fn deferred_terminal_count(bridge: &ThreadEventsBridge, thread_id: &str) -> usize {
    bridge
        .active_threads
        .read()
        .await
        .deferred_terminals
        .get(thread_id)
        .map(HashMap::len)
        .unwrap_or_default()
}

fn delta_event(payload: proto::thread_event::Payload) -> proto::ThreadEvent {
    proto::ThreadEvent {
        thread_id: "session-1".into(),
        turn_id: "turn-1".into(),
        payload: Some(payload),
    }
}

#[test]
fn streaming_deltas_feed_the_tool_card_instead_of_a_surface() {
    // 通用 activity 会被前端当成 a2ui surface，渲染成标题为事件名的空卡片。
    let cases = [
        proto::thread_event::Payload::ExecOutputDelta(proto::ThreadDelta {
            item_id: "call-1".into(),
            delta: "/tmp\n".into(),
        }),
        proto::thread_event::Payload::PlanDelta(proto::ThreadDelta {
            item_id: "plan-1".into(),
            delta: "1. inspect\n".into(),
        }),
        proto::thread_event::Payload::PatchDelta(proto::ThreadDelta {
            item_id: "patch-1".into(),
            delta: "--- a/foo\n+++ b/foo\n".into(),
        }),
    ];
    let expected = [
        ("call-1", "/tmp\n"),
        ("plan-1", "1. inspect\n"),
        ("patch-1", "--- a/foo\n+++ b/foo\n"),
    ];
    for (payload, (want_id, want_delta)) in cases.into_iter().zip(expected) {
        let mapped = map_thread_event(delta_event(payload));
        assert!(
            matches!(
                mapped.as_slice(),
                [ChatStreamEvent::ToolOutputDelta { id, delta }]
                    if id == want_id && delta == want_delta
            ),
            "{mapped:?}"
        );
    }
}

#[test]
fn review_tool_start_projects_desktop_pet_review_state() {
    let event = ChatStreamEvent::ToolCall {
        id: "review-1".into(),
        name: "code_review".into(),
        arguments_json: "{}".into(),
        result: String::new(),
        web_action: None,
        web_page_title: None,
        phase: "started".into(),
        batch_id: None,
        execution_mode: None,
        media: Vec::new(),
        file_changes: Vec::new(),
    };
    assert_eq!(desktop_pet_activity_for_event(&event), Some("review"));
    let ordinary = ChatStreamEvent::ToolCall {
        id: "terminal-1".into(),
        name: "terminal".into(),
        arguments_json: "{}".into(),
        result: String::new(),
        web_action: None,
        web_page_title: None,
        phase: "started".into(),
        batch_id: None,
        execution_mode: None,
        media: Vec::new(),
        file_changes: Vec::new(),
    };
    assert_eq!(desktop_pet_activity_for_event(&ordinary), None);
    assert_eq!(
        desktop_pet_activity_for_event(&ChatStreamEvent::RunFinished {
            run_id: "turn-1".into(),
            outcome_type: "success".into(),
            interrupts_json: "[]".into(),
        }),
        Some("jumping")
    );
    assert_eq!(
        desktop_pet_activity_for_event(&ChatStreamEvent::RunFinished {
            run_id: "turn-2".into(),
            outcome_type: "interrupt".into(),
            interrupts_json: "[]".into(),
        }),
        Some("idle")
    );
}

#[test]
fn plan_item_opens_a_tool_card() {
    let item = proto::ThreadItem {
        id: "plan-1".into(),
        item_type: "plan".into(),
        status: "in_progress".into(),
        payload_json: serde_json::to_string(&TurnItem::Plan(agent_protocol::TextItem {
            id: "plan-1".into(),
            content: "1. inspect\n".into(),
        }))
        .unwrap(),
    };
    assert!(matches!(
        map_item_event(proto::ThreadItemEvent { item: Some(item) }, true).as_slice(),
        [ChatStreamEvent::ToolCall {
            id,
            name,
            result,
            phase,
            ..
        }] if id == "plan-1"
            && name == "plan"
            && result == "1. inspect\n"
            && phase == "started"
    ));
}

#[test]
fn hook_prompt_item_projects_attributed_feedback_text() {
    let item = proto::ThreadItem {
        id: "msg-1".into(),
        item_type: "hook_prompt".into(),
        status: "completed".into(),
        payload_json: serde_json::to_string(&TurnItem::HookPrompt(
            agent_protocol::HookPromptItem::from_fragments(
                Some("msg-1"),
                vec![
                    agent_protocol::HookPromptFragment::from_single_hook("retry one", "hook-run-1"),
                    agent_protocol::HookPromptFragment::from_single_hook("retry two", "hook-run-2"),
                ],
            ),
        ))
        .unwrap(),
    };

    assert!(matches!(
        map_item_event(proto::ThreadItemEvent { item: Some(item) }, false).as_slice(),
        [ChatStreamEvent::Hook { name, detail, outcome }]
            if name == "hook_prompt"
                && detail == "retry one\n\nretry two"
                && outcome == "completed"
    ));
}

#[test]
fn tool_item_maps_batch_mode_and_terminal_statuses() {
    for (status, expected_phase) in [
        (agent_protocol::ToolStatus::Failed, "failed"),
        (agent_protocol::ToolStatus::Declined, "declined"),
        (agent_protocol::ToolStatus::Interrupted, "interrupted"),
    ] {
        let item = proto::ThreadItem {
            id: "call-1".into(),
            item_type: "dynamic_tool_call".into(),
            status: expected_phase.into(),
            payload_json: serde_json::to_string(&TurnItem::DynamicToolCall(
                agent_protocol::ToolItem {
                    id: "call-1".into(),
                    name: "read_file".into(),
                    arguments: serde_json::json!({"path":"README.md"}),
                    output: Some(serde_json::json!(expected_phase)),
                    web_action: None,
                    web_page_title: None,
                    media: Vec::new(),
                    file_changes: Vec::new(),
                    status,
                    batch_id: Some("batch-1".into()),
                    execution_mode: Some(agent_protocol::ToolExecutionMode::Parallel),
                },
            ))
            .unwrap(),
        };

        assert!(matches!(
            map_item_event(proto::ThreadItemEvent { item: Some(item) }, false).as_slice(),
            [ChatStreamEvent::ToolCall {
                phase,
                batch_id,
                execution_mode,
                ..
            }] if phase == expected_phase
                && batch_id.as_deref() == Some("batch-1")
                && execution_mode.as_deref() == Some("parallel")
        ));
    }
}

#[test]
fn browser_tool_item_maps_structured_web_action() {
    let item = proto::ThreadItem {
        id: "browser-1".into(),
        item_type: "dynamic_tool_call".into(),
        status: "completed".into(),
        payload_json: serde_json::to_string(&TurnItem::DynamicToolCall(agent_protocol::ToolItem {
            id: "browser-1".into(),
            name: "browser.snapshot".into(),
            arguments: serde_json::json!({"action":"read"}),
            output: Some(serde_json::json!({"url":"https://www.bilibili.com/"})),
            web_action: Some(agent_protocol::WebSearchAction::OpenPage {
                url: Some("https://www.bilibili.com/".into()),
            }),
            web_page_title: Some("B站".into()),
            media: Vec::new(),
            file_changes: Vec::new(),
            status: agent_protocol::ToolStatus::Completed,
            batch_id: None,
            execution_mode: None,
        }))
        .unwrap(),
    };

    assert!(matches!(
        map_item_event(proto::ThreadItemEvent { item: Some(item) }, false).as_slice(),
        [ChatStreamEvent::ToolCall {
            web_action: Some(agent_protocol::WebSearchAction::OpenPage { url }),
            web_page_title: Some(title),
            ..
        }] if url.as_deref() == Some("https://www.bilibili.com/") && title == "B站"
    ));
}

#[test]
fn terminal_thread_event_maps_to_run_finished_then_done() {
    let mapped = map_thread_event(terminal_event("session-1", "turn-1"));
    assert!(matches!(
        mapped.as_slice(),
        [
            ChatStreamEvent::RunFinished { outcome_type, .. },
            ChatStreamEvent::Done
        ] if outcome_type == "success"
    ));
}

#[test]
fn async_agent_message_maps_only_when_completed() {
    let item = async_message_item("call-1:async-message", "Still working");

    assert!(map_item_event(
        proto::ThreadItemEvent {
            item: Some(item.clone())
        },
        true,
    )
    .is_empty());
    assert!(matches!(
        map_item_event(proto::ThreadItemEvent { item: Some(item) }, false).as_slice(),
        [ChatStreamEvent::AsyncMessage { id, content, questions }]
            if id == "call-1:async-message" && content == "Still working"
                && questions.as_ref().is_some_and(|questions| questions.len() == 1)
    ));
}

#[tokio::test]
async fn snapshot_recovery_deduplicates_async_agent_messages_by_item_id() {
    let bridge = ThreadEventsBridge::new();
    let event = ChatStreamEvent::AsyncMessage {
        id: "call-1:async-message".into(),
        content: "Still working".into(),
        questions: None,
    };

    let first = bridge
        .recover_snapshot_projection("session-1", "turn-1", vec![event.clone()])
        .await;
    let replay = bridge
        .recover_snapshot_projection("session-1", "turn-1", vec![event])
        .await;

    assert_eq!(first.len(), 1);
    assert!(replay.is_empty());
}

#[tokio::test]
async fn core_error_then_failed_terminal_projects_one_error() {
    let error = proto::ThreadEvent {
        thread_id: "session-1".into(),
        turn_id: "turn-1".into(),
        payload: Some(proto::thread_event::Payload::Error(proto::ThreadError {
            message: "boom".into(),
            error_type: "provider".into(),
        })),
    };
    let terminal = proto::ThreadEvent {
        thread_id: "session-1".into(),
        turn_id: "turn-1".into(),
        payload: Some(proto::thread_event::Payload::TurnComplete(
            proto::ThreadTurnComplete {
                last_agent_message: String::new(),
                error: Some(proto::ThreadError {
                    message: "boom".into(),
                    error_type: "provider".into(),
                }),
                has_error: true,
            },
        )),
    };
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await;
    let mut projected = map_thread_event(error);
    bridge
        .record_delivered_projection("session-1", "turn-1", &projected)
        .await;
    projected.extend(
        bridge
            .dedup_terminal_projection("session-1", "turn-1", map_thread_event(terminal))
            .await,
    );
    assert_eq!(
        projected
            .iter()
            .filter(|event| matches!(event, ChatStreamEvent::Error { .. }))
            .count(),
        1
    );
    assert!(matches!(
        projected.as_slice(),
        [
            ChatStreamEvent::Error { .. },
            ChatStreamEvent::RunFinished { outcome_type, .. },
            ChatStreamEvent::Done
        ] if outcome_type == "error"
    ));
}

#[test]
fn terminal_only_error_is_preserved_before_error_outcome() {
    let terminal = proto::ThreadEvent {
        thread_id: "session-1".into(),
        turn_id: "turn-1".into(),
        payload: Some(proto::thread_event::Payload::TurnComplete(
            proto::ThreadTurnComplete {
                last_agent_message: String::new(),
                error: Some(proto::ThreadError {
                    message: "terminal failure".into(),
                    error_type: "provider".into(),
                }),
                has_error: true,
            },
        )),
    };

    assert!(matches!(
        map_thread_event(terminal).as_slice(),
        [
            ChatStreamEvent::Error { message },
            ChatStreamEvent::RunFinished { outcome_type, .. },
            ChatStreamEvent::Done
        ] if message == "terminal failure" && outcome_type == "error"
    ));
}

#[tokio::test]
async fn distinct_standalone_and_terminal_errors_are_both_projected() {
    let standalone = proto::ThreadEvent {
        thread_id: "session-1".into(),
        turn_id: "turn-1".into(),
        payload: Some(proto::thread_event::Payload::Error(proto::ThreadError {
            message: "stream failure".into(),
            error_type: "stream".into(),
        })),
    };
    let terminal = proto::ThreadEvent {
        thread_id: "session-1".into(),
        turn_id: "turn-1".into(),
        payload: Some(proto::thread_event::Payload::TurnComplete(
            proto::ThreadTurnComplete {
                last_agent_message: String::new(),
                error: Some(proto::ThreadError {
                    message: "final failure".into(),
                    error_type: "provider".into(),
                }),
                has_error: true,
            },
        )),
    };
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await;
    let mut projected = map_thread_event(standalone);
    bridge
        .record_delivered_projection("session-1", "turn-1", &projected)
        .await;
    projected.extend(
        bridge
            .dedup_terminal_projection("session-1", "turn-1", map_thread_event(terminal))
            .await,
    );

    assert_eq!(
        projected
            .iter()
            .filter(|event| matches!(event, ChatStreamEvent::Error { .. }))
            .count(),
        2
    );
}

#[test]
fn memory_extension_maps_to_existing_session_event_shape() {
    let event = proto::ThreadExtension {
        item_id: "memory-1".into(),
        namespace: "astro.memory".into(),
        payload_json: serde_json::json!({
            "source":"review",
            "target":"memory",
            "summary":"updated",
            "live_written":true
        })
        .to_string(),
    };
    let mapped = extension_to_session_event("session-1", event).expect("memory event");
    let memory = mapped.memory_updated.expect("memory payload");
    assert_eq!(mapped.session_id.as_deref(), Some("session-1"));
    assert_eq!(memory.source, "review");
    assert!(memory.live_written);
}

#[test]
fn agent_thread_extension_maps_complete_v2_session_event_projection() {
    let event = proto::ThreadExtension {
        item_id: "agent-thread-7".into(),
        namespace: "astro.agent_thread".into(),
        payload_json: serde_json::json!({
            "activity_sequence": 7,
            "stream_id": "generation-2",
            "agent_id": "root-agent",
            "root_thread_id": "root-session",
            "thread_id": "worker-thread",
            "parent_thread_id": "root-session",
            "canonical_path": "/root/worker",
            "task_name": "worker",
            "agent_type": "reviewer",
            "session_id": "worker-session",
            "status_kind": "completed",
            "status_payload_json": r#"{"kind":"completed","payload":{"last_message":"done"}}"#,
            "activity_kind": "status_changed"
        })
        .to_string(),
    };

    let mapped = extension_to_session_event("root-session", event.clone()).expect("agent event");
    let changed = mapped
        .agent_thread_changed
        .as_ref()
        .expect("agent thread projection");
    assert_eq!(mapped.session_id.as_deref(), Some("root-session"));
    assert_eq!(mapped.agent_id, "root-agent");
    assert_eq!(mapped.event_id, 7);
    assert_eq!(mapped.stream_id, "generation-2");
    assert_eq!(changed.activity_sequence, 7);
    assert_eq!(changed.root_thread_id, "root-session");
    assert_eq!(changed.thread_id, "worker-thread");
    assert_eq!(changed.canonical_path, "/root/worker");
    assert_eq!(changed.status_kind, "completed");
    assert_eq!(changed.activity_kind, "status_changed");
    assert!(mapped.resync_required.is_none());

    let serialized = serde_json::to_value(mapped).expect("serialize session event");
    assert_eq!(serialized["eventId"], 7);
    assert_eq!(serialized["streamId"], "generation-2");
    assert_eq!(
        serialized["agentThreadChanged"]["canonicalPath"],
        "/root/worker"
    );
    assert!(map_extension_to_chat(event).is_empty());
}

#[test]
fn agent_thread_resync_extension_preserves_generation_and_refresh_reason() {
    let event = proto::ThreadExtension {
        item_id: "agent-thread-resync".into(),
        namespace: "astro.agent_thread_resync".into(),
        payload_json: serde_json::json!({
            "stream_id": "generation-3",
            "root_thread_id": "root-session",
            "agent_id": "root-agent",
            "reason": "activity_gap"
        })
        .to_string(),
    };

    let mapped = extension_to_session_event("root-session", event.clone()).expect("resync event");
    assert_eq!(mapped.session_id.as_deref(), Some("root-session"));
    assert_eq!(mapped.agent_id, "root-agent");
    assert_eq!(mapped.event_id, 0);
    assert_eq!(mapped.stream_id, "generation-3");
    assert_eq!(
        mapped
            .resync_required
            .as_ref()
            .map(|reset| reset.reason.as_str()),
        Some("activity_gap")
    );
    assert!(mapped.agent_thread_changed.is_none());
    assert!(map_extension_to_chat(event).is_empty());
}

#[test]
fn agent_thread_extension_without_generation_is_rejected() {
    let event = proto::ThreadExtension {
        item_id: "agent-thread-legacy".into(),
        namespace: "astro.agent_thread".into(),
        payload_json: serde_json::json!({
            "activity_sequence": 1,
            "root_thread_id": "root-session"
        })
        .to_string(),
    };

    assert!(extension_to_session_event("root-session", event).is_none());
}

#[test]
fn user_input_committed_extension_preserves_client_message_identity() {
    let event = proto::ThreadExtension {
        item_id: "turn-1:user-input:queued-7".into(),
        namespace: "astro.user_input_committed".into(),
        payload_json: serde_json::json!({
            "turn_id": "turn-1",
            "client_message_id": "queued-7"
        })
        .to_string(),
    };

    assert!(matches!(
        map_extension_to_chat(event.clone()).as_slice(),
        [ChatStreamEvent::UserInputCommitted { client_message_id }]
            if client_message_id == "queued-7"
    ));
    assert!(extension_to_session_event("session-1", event).is_none());
}

#[test]
fn memory_extension_chat_adapter_requires_current_schema() {
    let current = map_extension_to_chat(proto::ThreadExtension {
        item_id: "memory-current".into(),
        namespace: "astro.memory".into(),
        payload_json: serde_json::json!({
            "source":"review",
            "target":"memory",
            "summary":"updated",
            "live_written":true
        })
        .to_string(),
    });
    assert!(matches!(
        current.as_slice(),
        [ChatStreamEvent::MemoryUpdate { operation, content }]
            if operation == "review" && content == "updated"
    ));

    let obsolete = map_extension_to_chat(proto::ThreadExtension {
        item_id: "memory-obsolete".into(),
        namespace: "astro.memory".into(),
        payload_json: serde_json::json!({"op":"memory","content":"obsolete"}).to_string(),
    });
    assert!(matches!(
        obsolete.as_slice(),
        [ChatStreamEvent::Error { message }]
            if message.contains("invalid memory update payload")
    ));
}

#[tokio::test]
async fn snapshot_extension_recovery_is_idempotent_by_stable_item_id() {
    let extension = TurnItem::Extension(agent_protocol::ExtensionItem {
        id: "turn-1:memory:review".into(),
        namespace: "astro.memory".into(),
        payload: serde_json::json!({
            "source":"review",
            "target":"memory",
            "summary":"updated",
            "live_written":true
        }),
    });
    let mut snapshot = proto::ThreadSnapshot {
        thread_id: "session-1".into(),
        status: "idle".into(),
        provider_id: None,
        backend_id: None,
        model: None,
        reasoning_effort: None,
        turns: vec![proto::ThreadTurn {
            id: "turn-1".into(),
            status: "completed".into(),
            items: vec![proto::ThreadItem {
                id: "turn-1:memory:review".into(),
                item_type: "extension".into(),
                status: "completed".into(),
                payload_json: serde_json::to_string(&extension).unwrap(),
            }],
            last_agent_message: String::new(),
            error: None,
            has_error: false,
        }],
        active_turn: None,
        has_active_turn: false,
        pending_background_turn_ids: vec![],
    };
    let bridge = ThreadEventsBridge::new();
    let first = recover_snapshot_extensions(&bridge, &snapshot).await;
    let second = recover_snapshot_extensions(&bridge, &snapshot).await;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].extension.namespace, "astro.memory");
    assert!(
        second.is_empty(),
        "unchanged snapshot must not replay twice"
    );

    snapshot.turns[0].items[0].payload_json =
        serde_json::to_string(&TurnItem::Extension(agent_protocol::ExtensionItem {
            id: "turn-1:memory:review".into(),
            namespace: "astro.memory".into(),
            payload: serde_json::json!({
                "source":"review",
                "target":"memory",
                "summary":"updated again",
                "live_written":true
            }),
        }))
        .unwrap();
    assert_eq!(
        recover_snapshot_extensions(&bridge, &snapshot).await.len(),
        1,
        "same stable item id with new state must still be delivered"
    );
}

#[test]
fn terminal_snapshot_synthesizes_terminal_before_buffered_live_events() {
    let snapshot = proto::ThreadSnapshot {
        thread_id: "session-1".into(),
        status: "idle".into(),
        provider_id: None,
        backend_id: None,
        model: None,
        reasoning_effort: None,
        turns: vec![proto::ThreadTurn {
            id: "turn-1".into(),
            status: "completed".into(),
            items: vec![
                proto::ThreadItem {
                    id: "message-1".into(),
                    item_type: "agent_message".into(),
                    status: "completed".into(),
                    payload_json: serde_json::to_string(&TurnItem::AgentMessage(
                        agent_protocol::AgentMessageItem {
                            id: "message-1".into(),
                            content: "almost ".into(),
                            delivery: None,
                            questions: None,
                        },
                    ))
                    .unwrap(),
                },
                proto::ThreadItem {
                    id: "message-2".into(),
                    item_type: "agent_message".into(),
                    status: "completed".into(),
                    payload_json: serde_json::to_string(&TurnItem::AgentMessage(
                        agent_protocol::AgentMessageItem {
                            id: "message-2".into(),
                            content: "done".into(),
                            delivery: None,
                            questions: None,
                        },
                    ))
                    .unwrap(),
                },
            ],
            last_agent_message: "done".into(),
            error: None,
            has_error: false,
        }],
        active_turn: None,
        has_active_turn: false,
        pending_background_turn_ids: vec![],
    };
    let outcome = reconcile_snapshot(&snapshot);
    assert!(matches!(
        outcome.terminal.as_slice(),
        [
            ChatStreamEvent::Token { content },
            ChatStreamEvent::RunFinished { outcome_type, .. },
            ChatStreamEvent::Done
        ] if content == "almost done" && outcome_type == "success"
    ));
    assert!(!outcome.keep_active);
}

#[test]
fn running_snapshot_keeps_thread_active_without_terminal_projection() {
    let snapshot = proto::ThreadSnapshot {
        thread_id: "session-1".into(),
        status: "running".into(),
        provider_id: None,
        backend_id: None,
        model: None,
        reasoning_effort: None,
        turns: vec![],
        active_turn: Some(proto::ThreadTurn {
            id: "turn-1".into(),
            status: "in_progress".into(),
            items: vec![],
            last_agent_message: String::new(),
            error: None,
            has_error: false,
        }),
        has_active_turn: true,
        pending_background_turn_ids: vec![],
    };
    let outcome = reconcile_snapshot(&snapshot);
    assert!(outcome.keep_active);
    assert!(outcome.terminal.is_empty());
}

#[tokio::test]
async fn running_snapshot_recovers_agent_text_before_buffered_terminal() {
    let snapshot = proto::ThreadSnapshot {
        thread_id: "session-1".into(),
        status: "running".into(),
        provider_id: None,
        backend_id: None,
        model: None,
        reasoning_effort: None,
        turns: vec![],
        active_turn: Some(proto::ThreadTurn {
            id: "turn-1".into(),
            status: "in_progress".into(),
            items: vec![agent_message_item("message-1", "done")],
            last_agent_message: String::new(),
            error: None,
            has_error: false,
        }),
        has_active_turn: true,
        pending_background_turn_ids: vec![],
    };
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await;

    let reconciled = reconcile_snapshot(&snapshot);
    let mut projected = bridge
        .recover_snapshot_projection("session-1", "turn-1", reconciled.terminal)
        .await;
    projected.extend(
        bridge
            .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
            .await,
    );

    assert!(matches!(
        projected.as_slice(),
        [
            ChatStreamEvent::Token { content },
            ChatStreamEvent::RunFinished { .. },
            ChatStreamEvent::Done
        ] if content == "done"
    ));
}

#[test]
fn reconnect_generation_delivers_every_snapshot_before_buffered_live() {
    let snapshots = vec![proto::ThreadSnapshot {
        thread_id: "session-1".into(),
        status: "running".into(),
        provider_id: None,
        backend_id: None,
        model: None,
        reasoning_effort: None,
        turns: vec![],
        active_turn: None,
        has_active_turn: false,
        pending_background_turn_ids: vec![],
    }];
    let live = vec![Ok(proto::ThreadEvent {
        thread_id: "session-1".into(),
        turn_id: "turn-1".into(),
        payload: Some(proto::thread_event::Payload::AgentMessageDelta(
            proto::ThreadDelta {
                item_id: "message-1".into(),
                delta: "late".into(),
            },
        )),
    })];
    let ordered = reconnect_delivery_order(snapshots, live);
    assert!(matches!(
        ordered.first(),
        Some(ReconnectDelivery::Snapshot(_))
    ));
    assert!(matches!(ordered.get(1), Some(ReconnectDelivery::Live(_))));
}

#[tokio::test]
async fn duplicate_terminal_is_suppressed_and_removes_active_thread() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await;
    assert!(!bridge
        .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
        .await
        .is_empty());
    assert!(bridge
        .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
        .await
        .is_empty());
    assert!(!bridge.is_active("session-1").await);
}

#[tokio::test]
async fn successful_terminal_moves_thread_to_background_resume_set_until_marker() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await;

    assert!(
        !bridge
            .accept_terminal_with_background(
                "session-1",
                "turn-1",
                terminal_projection("turn-1"),
                true,
            )
            .await
            .is_empty()
    );
    assert!(!bridge.is_active("session-1").await);
    assert_eq!(
        bridge.background_resume_threads().await,
        vec!["session-1".to_string()]
    );

    assert!(bridge.complete_background_turn("session-1", "turn-1").await);
    assert!(bridge.background_resume_threads().await.is_empty());
}

#[tokio::test]
async fn live_background_expired_extension_clears_pending_subscription_without_projection() {
    let bridge = ThreadEventsBridge::new();
    let mut cleanup = bridge.take_terminal_unsubscribe_requests();
    let activation = bridge.activate("session-expire").await;
    bridge
        .bind_submitted_turn_if_current("session-expire", activation, "turn-expire")
        .await;
    bridge
        .accept_terminal_with_background(
            "session-expire",
            "turn-expire",
            terminal_projection("turn-expire"),
            true,
        )
        .await;
    let terminal_cleanup = cleanup.recv().await.expect("terminal cleanup");
    assert!(bridge
        .run_terminal_unsubscribe_if_owned(terminal_cleanup, |_| async { Ok(()) })
        .await
        .unwrap());
    let background_targets = bridge.background_resume_targets().await;
    let [(thread_id, subscription)] = background_targets.as_slice() else {
        panic!("one background subscription expected");
    };
    let extension = proto::ThreadExtension {
        item_id: "turn-expire:background_expired".into(),
        namespace: "astro.background_expired".into(),
        payload_json: serde_json::json!({"turn_id":"turn-expire"}).to_string(),
    };

    assert!(
        bridge
            .observe_extension("session-expire", "turn-expire", &extension)
            .await
    );
    assert!(map_extension_to_chat(extension).is_empty());
    assert!(bridge.background_resume_threads().await.is_empty());
    let request = cleanup.recv().await.expect("expiration cleanup");
    assert_eq!(request.thread_id, *thread_id);
    assert_eq!(request.activation, *subscription);
    assert!(bridge
        .run_terminal_unsubscribe_if_owned(request, |_| async { Ok(()) })
        .await
        .unwrap());
}

#[tokio::test]
async fn offline_review_and_marker_recover_once_then_release_background_subscription() {
    let bridge = ThreadEventsBridge::new();
    let mut cleanup = bridge.take_terminal_unsubscribe_requests();
    let activation = bridge.activate("session-offline").await;
    bridge
        .bind_submitted_turn_if_current("session-offline", activation, "turn-offline")
        .await;
    bridge
        .accept_terminal_with_background(
            "session-offline",
            "turn-offline",
            terminal_projection("turn-offline"),
            true,
        )
        .await;
    let terminal_cleanup = cleanup.recv().await.expect("terminal cleanup");
    assert!(bridge
        .run_terminal_unsubscribe_if_owned(terminal_cleanup, |_| async { Ok(()) })
        .await
        .unwrap());
    let background_targets = bridge.background_resume_targets().await;
    let [(thread_id, subscription)] = background_targets.as_slice() else {
        panic!("one background Resume target expected");
    };
    let subscriber = Arc::new(AtomicBool::new(false));
    let resumed = Arc::clone(&subscriber);
    assert_eq!(
        bridge
            .run_background_resume_if_owned(thread_id.clone(), *subscription, move || async move {
                resumed.store(true, Ordering::SeqCst);
                Ok::<_, String>(())
            },)
            .await
            .unwrap(),
        Some(())
    );

    let review = TurnItem::Extension(agent_protocol::ExtensionItem {
        id: "turn-offline:memory:review".into(),
        namespace: "astro.memory".into(),
        payload: serde_json::json!({
            "source":"review",
            "target":"memory",
            "summary":"offline review",
            "live_written":true
        }),
    });
    let marker = TurnItem::Extension(agent_protocol::ExtensionItem {
        id: "turn-offline:background_complete".into(),
        namespace: "astro.background_complete".into(),
        payload: serde_json::json!({}),
    });
    let mut snapshot = proto::ThreadSnapshot {
        thread_id: "session-offline".into(),
        status: "idle".into(),
        provider_id: None,
        backend_id: None,
        model: None,
        reasoning_effort: None,
        turns: vec![proto::ThreadTurn {
            id: "turn-offline".into(),
            status: "completed".into(),
            items: vec![proto::ThreadItem {
                id: "turn-offline:memory:review".into(),
                item_type: "extension".into(),
                status: "completed".into(),
                payload_json: serde_json::to_string(&review).unwrap(),
            }],
            last_agent_message: "done".into(),
            error: None,
            has_error: false,
        }],
        active_turn: None,
        has_active_turn: false,
        pending_background_turn_ids: vec![],
    };

    let recovered = recover_snapshot_extensions(&bridge, &snapshot).await;
    let session_events = recovered
        .iter()
        .filter_map(|extension| {
            extension_to_session_event("session-offline", extension.extension.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        session_events.len(),
        1,
        "offline review must reach session_event"
    );
    assert_eq!(
        session_events[0]
            .memory_updated
            .as_ref()
            .expect("memory event")
            .summary,
        "offline review"
    );
    assert_eq!(
        bridge.background_resume_threads().await,
        vec!["session-offline".to_string()],
        "without the completion marker the resumed formal subscription stays live"
    );
    assert!(subscriber.load(Ordering::SeqCst));
    assert!(cleanup.try_recv().is_err());

    snapshot.turns[0].items.push(proto::ThreadItem {
        id: "turn-offline:background_complete".into(),
        item_type: "extension".into(),
        status: "completed".into(),
        payload_json: serde_json::to_string(&marker).unwrap(),
    });
    let marker_recovery = recover_snapshot_extensions(&bridge, &snapshot).await;
    assert_eq!(marker_recovery.len(), 1);
    assert_eq!(
        marker_recovery[0].extension.namespace,
        "astro.background_complete"
    );
    assert!(bridge.background_resume_threads().await.is_empty());
    let background_cleanup = cleanup.recv().await.expect("background cleanup");
    let unsubscribed = Arc::clone(&subscriber);
    assert!(bridge
        .run_terminal_unsubscribe_if_owned(background_cleanup, move |_| async move {
            unsubscribed.store(false, Ordering::SeqCst);
            Ok(())
        })
        .await
        .unwrap());
    assert!(!subscriber.load(Ordering::SeqCst));
    assert!(recover_snapshot_extensions(&bridge, &snapshot)
        .await
        .is_empty());
}

#[tokio::test]
async fn authoritative_snapshot_expires_missing_background_turns_and_unsubscribes_last() {
    let bridge = ThreadEventsBridge::new();
    let mut cleanup = bridge.take_terminal_unsubscribe_requests();
    for turn_id in ["turn-1", "turn-2"] {
        let activation = bridge.activate("session-expired").await;
        bridge
            .bind_submitted_turn_if_current("session-expired", activation, turn_id)
            .await;
        bridge
            .accept_terminal_with_background(
                "session-expired",
                turn_id,
                terminal_projection(turn_id),
                true,
            )
            .await;
        let request = cleanup.recv().await.expect("terminal cleanup");
        assert!(bridge
            .run_terminal_unsubscribe_if_owned(request, |_| async { Ok(()) })
            .await
            .unwrap());
    }

    let background_targets = bridge.background_resume_targets().await;
    let [(thread_id, subscription)] = background_targets.as_slice() else {
        panic!("one background Resume target expected");
    };
    assert_eq!(thread_id, "session-expired");

    bridge
        .reconcile_background_snapshot(&proto::ThreadSnapshot {
            thread_id: "session-expired".into(),
            pending_background_turn_ids: vec!["turn-2".into()],
            ..Default::default()
        })
        .await;
    assert_eq!(
        bridge
            .active_threads
            .read()
            .await
            .background_pending
            .get("session-expired")
            .cloned(),
        Some(HashSet::from(["turn-2".to_string()]))
    );
    assert!(cleanup.try_recv().is_err());

    bridge
        .reconcile_background_snapshot(&proto::ThreadSnapshot {
            thread_id: "session-expired".into(),
            pending_background_turn_ids: vec![],
            ..Default::default()
        })
        .await;
    assert!(bridge.background_resume_threads().await.is_empty());
    let request = cleanup.recv().await.expect("background cleanup");
    assert_eq!(request.activation, *subscription);
    assert!(bridge
        .run_terminal_unsubscribe_if_owned(request, |_| async { Ok(()) })
        .await
        .unwrap());
    assert!(!bridge
        .terminal_cleanup
        .owners
        .read()
        .await
        .contains_key("session-expired"));
}

#[tokio::test]
async fn empty_authoritative_snapshot_retires_local_background_state() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-legacy").await;
    bridge
        .bind_submitted_turn_if_current("session-legacy", activation, "turn-legacy")
        .await;
    bridge
        .accept_terminal_with_background(
            "session-legacy",
            "turn-legacy",
            terminal_projection("turn-legacy"),
            true,
        )
        .await;

    assert!(
        bridge
            .reconcile_background_snapshot(&proto::ThreadSnapshot {
                thread_id: "session-legacy".into(),
                pending_background_turn_ids: vec![],
                ..Default::default()
            })
            .await
    );
    assert!(bridge.background_resume_threads().await.is_empty());
}

#[tokio::test]
async fn forget_thread_clears_resume_targets_and_stale_cleanup_cannot_touch_reuse() {
    let bridge = ThreadEventsBridge::new();
    let mut cleanup = bridge.take_terminal_unsubscribe_requests();
    let activation = bridge.activate("session-release").await;
    bridge
        .bind_submitted_turn_if_current("session-release", activation, "turn-old")
        .await;
    bridge
        .accept_terminal_with_background(
            "session-release",
            "turn-old",
            terminal_projection("turn-old"),
            true,
        )
        .await;
    let terminal_cleanup = cleanup.recv().await.expect("terminal cleanup");
    assert!(bridge
        .run_terminal_unsubscribe_if_owned(terminal_cleanup, |_| async { Ok(()) })
        .await
        .unwrap());
    let background_targets = bridge.background_resume_targets().await;
    assert_eq!(background_targets.len(), 1);
    assert!(
        bridge
            .accept_extension("session-release", "extension-old", "{}")
            .await
    );

    bridge.forget_thread("session-release").await;
    assert!(bridge.background_resume_targets().await.is_empty());
    assert!(!bridge.is_active("session-release").await);
    assert!(!bridge
        .active_threads
        .read()
        .await
        .delivered_extensions
        .keys()
        .any(|(thread_id, _)| thread_id == "session-release"));

    let forgotten_cleanup = cleanup
        .recv()
        .await
        .expect("forgotten subscription cleanup");
    let replacement = bridge.activate("session-release").await;
    assert!(!bridge
        .run_terminal_unsubscribe_if_owned(forgotten_cleanup, |_| async {
            panic!("stale cleanup must not unsubscribe the reused thread id")
        })
        .await
        .unwrap());
    assert!(bridge.is_active("session-release").await);
    assert_eq!(
        bridge
            .terminal_cleanup
            .owners
            .read()
            .await
            .get("session-release")
            .copied(),
        Some(replacement)
    );
}

#[tokio::test]
async fn accepted_terminal_queues_and_executes_exact_unsubscribe() {
    let bridge = ThreadEventsBridge::new();
    let mut requests = bridge.take_terminal_unsubscribe_requests();
    let activation = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await;
    assert!(!bridge
        .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
        .await
        .is_empty());

    let request = requests.recv().await.expect("terminal unsubscribe");
    assert_eq!(request.thread_id, "session-1");
    assert_eq!(request.activation, activation);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    assert!(bridge
        .run_terminal_unsubscribe_if_owned(request, move |thread_id| async move {
            assert_eq!(thread_id, "session-1");
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await
        .unwrap());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn recovery_skips_captured_activation_after_failure_cleanup_wins() {
    let bridge = Arc::new(ThreadEventsBridge::new());
    let mut cleanup = bridge.take_terminal_unsubscribe_requests();
    let activation = bridge.activate("session-1").await;
    let captured = bridge.active_activations().await;
    let [(thread_id, captured_activation)] = captured.as_slice() else {
        panic!("one captured activation expected");
    };
    let thread_id = thread_id.clone();
    let captured_activation = *captured_activation;
    let subscriber = Arc::new(AtomicBool::new(true));
    let paused = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());

    let resume_bridge = Arc::clone(&bridge);
    let resume_subscriber = Arc::clone(&subscriber);
    let resume_paused = Arc::clone(&paused);
    let resume_release = Arc::clone(&release);
    let resume = tokio::spawn(async move {
        resume_paused.notify_one();
        resume_release.notified().await;
        resume_bridge
            .run_resume_if_owned(thread_id, captured_activation, move || async move {
                resume_subscriber.store(true, Ordering::SeqCst);
                Ok::<_, String>(())
            })
            .await
    });
    paused.notified().await;

    assert!(bridge.fail_activation("session-1", activation).await);
    let request = cleanup.recv().await.expect("failed activation cleanup");
    let cleanup_subscriber = Arc::clone(&subscriber);
    assert!(bridge
        .run_terminal_unsubscribe_if_owned(request, move |_| async move {
            cleanup_subscriber.store(false, Ordering::SeqCst);
            Ok(())
        })
        .await
        .unwrap());

    release.notify_one();
    assert_eq!(resume.await.unwrap().unwrap(), None);
    assert!(!subscriber.load(Ordering::SeqCst));
}

#[tokio::test]
async fn failure_cleanup_runs_after_in_flight_recovery_resume() {
    let bridge = Arc::new(ThreadEventsBridge::new());
    let mut cleanup = bridge.take_terminal_unsubscribe_requests();
    let activation = bridge.activate("session-1").await;
    let subscriber = Arc::new(AtomicBool::new(true));
    let resume_entered = Arc::new(tokio::sync::Notify::new());
    let resume_release = Arc::new(tokio::sync::Notify::new());

    let resume_bridge = Arc::clone(&bridge);
    let resume_subscriber = Arc::clone(&subscriber);
    let entered = Arc::clone(&resume_entered);
    let release = Arc::clone(&resume_release);
    let resume = tokio::spawn(async move {
        resume_bridge
            .run_resume_if_owned("session-1".into(), activation, move || async move {
                entered.notify_one();
                release.notified().await;
                resume_subscriber.store(true, Ordering::SeqCst);
                Ok::<_, String>(())
            })
            .await
    });
    resume_entered.notified().await;

    assert!(bridge.fail_activation("session-1", activation).await);
    let request = cleanup.recv().await.expect("failed activation cleanup");
    let cleanup_bridge = Arc::clone(&bridge);
    let cleanup_subscriber = Arc::clone(&subscriber);
    let mut cleanup_task = tokio::spawn(async move {
        cleanup_bridge
            .run_terminal_unsubscribe_if_owned(request, move |_| async move {
                cleanup_subscriber.store(false, Ordering::SeqCst);
                Ok(())
            })
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut cleanup_task)
            .await
            .is_err()
    );

    resume_release.notify_one();
    assert_eq!(resume.await.unwrap().unwrap(), Some(()));
    assert!(cleanup_task.await.unwrap().unwrap());
    assert!(!subscriber.load(Ordering::SeqCst));
}

#[tokio::test]
async fn captured_recovery_cannot_resume_over_a_new_activation_owner() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-1").await;
    let captured = bridge.active_activations().await;
    let [(thread_id, captured_activation)] = captured.as_slice() else {
        panic!("one captured activation expected");
    };
    assert_eq!(*captured_activation, old);
    let thread_id = thread_id.clone();

    let current = bridge.activate("session-1").await;
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    assert_eq!(
        bridge
            .run_resume_if_owned(thread_id, old, move || async move {
                counted.fetch_add(1, Ordering::SeqCst);
                Ok::<_, String>(())
            })
            .await
            .unwrap(),
        None
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        bridge
            .active_threads
            .read()
            .await
            .activations
            .get("session-1"),
        Some(&current)
    );
    assert_eq!(
        bridge.terminal_cleanup.owners.read().await.get("session-1"),
        Some(&current)
    );
}

#[tokio::test]
async fn new_activation_waits_for_in_flight_recovery_and_then_owns_subscription() {
    let bridge = Arc::new(ThreadEventsBridge::new());
    let old = bridge.activate("session-1").await;
    let resume_entered = Arc::new(tokio::sync::Notify::new());
    let resume_release = Arc::new(tokio::sync::Notify::new());

    let resume_bridge = Arc::clone(&bridge);
    let entered = Arc::clone(&resume_entered);
    let release = Arc::clone(&resume_release);
    let resume = tokio::spawn(async move {
        resume_bridge
            .run_resume_if_owned("session-1".into(), old, move || async move {
                entered.notify_one();
                release.notified().await;
                Ok::<_, String>(())
            })
            .await
    });
    resume_entered.notified().await;

    let activation_bridge = Arc::clone(&bridge);
    let mut activation = tokio::spawn(async move { activation_bridge.activate("session-1").await });
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut activation)
            .await
            .is_err()
    );

    resume_release.notify_one();
    assert_eq!(resume.await.unwrap().unwrap(), Some(()));
    let current = activation.await.unwrap();
    assert_ne!(current, old);
    assert_eq!(
        bridge
            .active_threads
            .read()
            .await
            .activations
            .get("session-1"),
        Some(&current)
    );
    assert_eq!(
        bridge.terminal_cleanup.owners.read().await.get("session-1"),
        Some(&current)
    );
}

#[tokio::test]
async fn stale_terminal_unsubscribe_cannot_remove_new_activation() {
    let bridge = ThreadEventsBridge::new();
    let mut requests = bridge.take_terminal_unsubscribe_requests();
    let old = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", old, "turn-old")
        .await;
    bridge
        .accept_terminal("session-1", "turn-old", terminal_projection("turn-old"))
        .await;
    let stale = requests.recv().await.expect("old terminal unsubscribe");

    let current = bridge.activate("session-1").await;
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    assert!(!bridge
        .run_terminal_unsubscribe_if_owned(stale, move |_| async move {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await
        .unwrap());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(bridge.is_active("session-1").await);
    assert_eq!(
        bridge
            .active_threads
            .read()
            .await
            .activations
            .get("session-1"),
        Some(&current)
    );
}

#[tokio::test]
async fn terminal_subscription_gate_registry_prunes_unique_threads() {
    let bridge = ThreadEventsBridge::new();
    for index in 0..1_000 {
        let gate = bridge
            .terminal_cleanup
            .gate(&format!("unique-session-{index}"));
        let guard = gate.lock().await;
        drop(guard);
        drop(gate);
    }

    assert!(bridge
        .terminal_cleanup
        .gates
        .lock()
        .expect("terminal gate registry")
        .is_empty());
}

#[tokio::test]
async fn steered_terminal_unsubscribe_is_owned_by_current_activation() {
    let bridge = ThreadEventsBridge::new();
    let mut requests = bridge.take_terminal_unsubscribe_requests();
    let old = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", old, "turn-1")
        .await;
    let current = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", current, "turn-1")
        .await;
    assert!(!bridge
        .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
        .await
        .is_empty());

    let request = requests.recv().await.expect("steered unsubscribe");
    assert_eq!(request.activation, current);
}

#[tokio::test]
async fn terminal_before_ack_releases_once_and_clears_thread_state() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge.bind_observed_turn("session-1", "turn-1").await;
    assert!(bridge
        .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
        .await
        .is_empty());
    assert!(bridge.is_active("session-1").await);

    assert!(!bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await
        .is_empty());
    let state = bridge.active_threads.read().await;
    assert!(!state.threads.contains("session-1"));
    assert!(!state.turn_epochs.contains_key("session-1"));
    assert!(!state.activations.contains_key("session-1"));
    assert!(!state.awaiting_submissions.contains_key("session-1"));
    assert!(!state.deferred_terminals.contains_key("session-1"));
    assert!(!state.delivered_agent_text.contains_key("session-1"));
    assert!(!state.delivered_reasoning.contains_key("session-1"));
    assert!(!state.pending_terminal_errors.contains_key("session-1"));
}

#[tokio::test]
async fn stale_submit_failure_cannot_remove_a_newer_activation() {
    let bridge = ThreadEventsBridge::new();
    let mut cleanup = bridge.take_terminal_unsubscribe_requests();
    let old = bridge.activate("session-1").await;
    let current = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", current, "turn-current")
        .await;

    assert!(!bridge.fail_activation("session-1", old).await);
    assert!(cleanup.try_recv().is_err());
    assert!(bridge.is_active("session-1").await);
    assert_eq!(
        bridge
            .active_threads
            .read()
            .await
            .activations
            .get("session-1"),
        Some(&current)
    );
}

#[tokio::test]
async fn current_replacement_failure_queues_activation_owned_unsubscribe() {
    let bridge = ThreadEventsBridge::new();
    let mut cleanup = bridge.take_terminal_unsubscribe_requests();
    let _old = bridge.activate("session-1").await;
    let current = bridge.activate("session-1").await;

    assert!(bridge.fail_activation("session-1", current).await);
    assert!(!bridge.is_active("session-1").await);
    let request = cleanup.recv().await.expect("current failure cleanup");
    assert_eq!(request.activation, current);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    assert!(bridge
        .run_terminal_unsubscribe_if_owned(request, move |_| async move {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await
        .unwrap());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!bridge
        .terminal_cleanup
        .owners
        .read()
        .await
        .contains_key("session-1"));
}

#[tokio::test]
async fn old_turn_terminal_cannot_retire_a_new_activation() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", old, "turn-old")
        .await;
    let current = bridge.activate("session-1").await;

    // A reconnect snapshot can observe the old active turn again. Rebinding it must not
    // promote that turn into the new activation epoch.
    bridge.bind_observed_turn("session-1", "turn-old").await;

    assert!(bridge
        .accept_terminal("session-1", "turn-old", terminal_projection("turn-old"))
        .await
        .is_empty());
    assert!(bridge.is_active("session-1").await);

    bridge
        .bind_submitted_turn_if_current("session-1", current, "turn-current")
        .await;
    assert!(!bridge
        .accept_terminal(
            "session-1",
            "turn-current",
            terminal_projection("turn-current")
        )
        .await
        .is_empty());
    assert!(!bridge.is_active("session-1").await);
}

#[tokio::test]
async fn authoritative_steered_submit_rebinds_same_turn_to_current_activation() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", old, "turn-1")
        .await;

    let current = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", current, "turn-1")
        .await;

    assert!(!bridge
        .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
        .await
        .is_empty());
    assert!(!bridge.is_active("session-1").await);
}

#[tokio::test]
async fn late_old_ack_and_turn_started_cannot_bind_the_current_activation() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-1").await;
    let current = bridge.activate("session-1").await;

    assert!(bridge
        .bind_submitted_turn_if_current("session-1", old, "turn-old")
        .await
        .is_empty());
    bridge.bind_observed_turn("session-1", "turn-old").await;
    assert!(bridge
        .accept_terminal("session-1", "turn-old", terminal_projection("turn-old"))
        .await
        .is_empty());
    assert!(bridge.is_active("session-1").await);

    bridge
        .bind_submitted_turn_if_current("session-1", current, "turn-current")
        .await;
    assert!(!bridge
        .accept_terminal(
            "session-1",
            "turn-current",
            terminal_projection("turn-current")
        )
        .await
        .is_empty());
}

#[tokio::test]
async fn first_turn_started_binds_but_terminal_waits_for_authoritative_ack() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge.bind_observed_turn("session-1", "turn-1").await;

    assert!(bridge
        .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
        .await
        .is_empty());
    assert!(bridge.is_active("session-1").await);
    assert!(!bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await
        .is_empty());
    assert!(!bridge.is_active("session-1").await);
}

#[tokio::test]
async fn observed_turn_with_multiple_pending_acks_waits_for_authoritative_bindings() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-observed").await;
    let current = bridge.activate("session-observed").await;

    bridge
        .bind_observed_turn("session-observed", "turn-x")
        .await;
    assert!(bridge
        .bind_submitted_turn_if_current("session-observed", old, "turn-x")
        .await
        .is_empty());
    assert!(bridge
        .bind_submitted_turn_if_current("session-observed", current, "turn-y")
        .await
        .is_empty());

    assert!(bridge
        .accept_terminal("session-observed", "turn-x", terminal_projection("turn-x"),)
        .await
        .is_empty());
    assert!(bridge.is_active("session-observed").await);
    assert!(!bridge
        .accept_terminal("session-observed", "turn-y", terminal_projection("turn-y"),)
        .await
        .is_empty());
}

#[tokio::test]
async fn observed_terminal_before_mismatched_current_ack_does_not_finish_current_turn() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-observed-preterminal").await;
    let current = bridge.activate("session-observed-preterminal").await;

    bridge
        .bind_observed_turn("session-observed-preterminal", "turn-x")
        .await;
    assert!(bridge
        .accept_terminal(
            "session-observed-preterminal",
            "turn-x",
            terminal_projection("turn-x"),
        )
        .await
        .is_empty());
    assert!(bridge.is_active("session-observed-preterminal").await);
    assert!(bridge
        .bind_submitted_turn_if_current("session-observed-preterminal", old, "turn-x")
        .await
        .is_empty());
    assert!(bridge
        .bind_submitted_turn_if_current("session-observed-preterminal", current, "turn-y")
        .await
        .is_empty());
    assert!(bridge.is_active("session-observed-preterminal").await);
    assert_eq!(
        deferred_terminal_count(&bridge, "session-observed-preterminal").await,
        0
    );
    assert!(bridge
        .accept_terminal(
            "session-observed-preterminal",
            "turn-x",
            terminal_projection("turn-x"),
        )
        .await
        .is_empty());
    assert!(bridge.is_active("session-observed-preterminal").await);
    assert!(!bridge
        .accept_terminal(
            "session-observed-preterminal",
            "turn-y",
            terminal_projection("turn-y"),
        )
        .await
        .is_empty());
}

#[tokio::test]
async fn observed_terminal_with_matching_current_ack_still_releases_current_turn() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-observed-match").await;
    let current = bridge.activate("session-observed-match").await;

    bridge
        .bind_observed_turn("session-observed-match", "turn-x")
        .await;
    assert!(bridge
        .accept_terminal(
            "session-observed-match",
            "turn-x",
            terminal_projection("turn-x"),
        )
        .await
        .is_empty());
    assert!(bridge
        .bind_submitted_turn_if_current("session-observed-match", old, "turn-x")
        .await
        .is_empty());
    assert!(!bridge
        .bind_submitted_turn_if_current("session-observed-match", current, "turn-x")
        .await
        .is_empty());
    assert!(!bridge.is_active("session-observed-match").await);
}

#[tokio::test]
async fn authoritative_current_ack_removes_different_provisional_turn_epoch() {
    let bridge = ThreadEventsBridge::new();
    let current = bridge.activate("session-provisional").await;
    bridge
        .bind_observed_turn("session-provisional", "turn-x")
        .await;

    bridge
        .bind_submitted_turn_if_current("session-provisional", current, "turn-y")
        .await;
    let state = bridge.active_threads.read().await;
    let epochs = state
        .turn_epochs
        .get("session-provisional")
        .expect("authoritative turn epoch");
    assert_eq!(epochs.get("turn-y"), Some(&current));
    assert!(!epochs.contains_key("turn-x"));
}

#[tokio::test]
async fn provisional_terminal_and_later_marker_wait_for_mismatched_authoritative_ack() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-provisional-y").await;
    bridge
        .bind_observed_turn("session-provisional-y", "turn-x")
        .await;
    assert!(bridge
        .accept_terminal_with_background(
            "session-provisional-y",
            "turn-x",
            terminal_projection("turn-x"),
            true,
        )
        .await
        .is_empty());
    assert!(bridge.is_active("session-provisional-y").await);
    assert_eq!(
        deferred_terminal_count(&bridge, "session-provisional-y").await,
        1
    );
    assert!(
        !bridge
            .complete_background_turn("session-provisional-y", "turn-x")
            .await
    );

    assert!(bridge
        .bind_submitted_turn_if_current("session-provisional-y", activation, "turn-y")
        .await
        .is_empty());
    assert!(bridge.is_active("session-provisional-y").await);
    assert_eq!(
        deferred_terminal_count(&bridge, "session-provisional-y").await,
        0
    );
    assert!(bridge.background_resume_threads().await.is_empty());
    assert!(!bridge
        .active_threads
        .read()
        .await
        .completed_background_turns
        .contains_key("session-provisional-y"));
    assert!(!bridge
        .accept_terminal(
            "session-provisional-y",
            "turn-y",
            terminal_projection("turn-y"),
        )
        .await
        .is_empty());
}

#[tokio::test]
async fn provisional_terminal_and_prior_marker_release_on_matching_authoritative_ack() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-provisional-x").await;
    bridge
        .bind_observed_turn("session-provisional-x", "turn-x")
        .await;
    assert!(
        !bridge
            .complete_background_turn("session-provisional-x", "turn-x")
            .await
    );

    assert!(bridge
        .accept_terminal_with_background(
            "session-provisional-x",
            "turn-x",
            terminal_projection("turn-x"),
            true,
        )
        .await
        .is_empty());
    assert!(bridge.is_active("session-provisional-x").await);
    assert_eq!(
        deferred_terminal_count(&bridge, "session-provisional-x").await,
        1
    );

    assert!(!bridge
        .bind_submitted_turn_if_current("session-provisional-x", activation, "turn-x")
        .await
        .is_empty());
    assert!(!bridge.is_active("session-provisional-x").await);
    assert!(bridge.background_resume_threads().await.is_empty());
}

#[tokio::test]
async fn current_ack_drains_event_buffered_before_an_old_ack_bound_the_same_turn() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-buffer-multi-ack").await;
    let current = bridge.activate("session-buffer-multi-ack").await;
    bridge
        .bind_observed_turn("session-buffer-multi-ack", "turn-x")
        .await;
    assert!(
        !bridge
            .record_delivered_projection(
                "session-buffer-multi-ack",
                "turn-x",
                &[ChatStreamEvent::Token {
                    content: "current token".into(),
                }],
            )
            .await
    );

    assert!(bridge
        .bind_submitted_turn_if_current("session-buffer-multi-ack", old, "turn-x")
        .await
        .is_empty());
    let released = bridge
        .bind_submitted_turn_if_current("session-buffer-multi-ack", current, "turn-x")
        .await;
    assert!(matches!(
        released.as_slice(),
        [ChatStreamEvent::Token { content }] if content == "current token"
    ));
}

#[tokio::test]
async fn different_current_ack_discards_buffer_even_when_turn_epoch_belongs_to_old_ack() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-buffer-multi-ack-y").await;
    let current = bridge.activate("session-buffer-multi-ack-y").await;
    assert!(bridge
        .bind_submitted_turn_if_current("session-buffer-multi-ack-y", old, "turn-x")
        .await
        .is_empty());
    assert!(
        !bridge
            .record_delivered_projection(
                "session-buffer-multi-ack-y",
                "turn-x",
                &[ChatStreamEvent::Token {
                    content: "stale token".into(),
                }],
            )
            .await
    );
    assert_eq!(
        bridge
            .active_threads
            .read()
            .await
            .provisional_nonterminal_events
            .get("session-buffer-multi-ack-y")
            .map(|buffers| buffers.values().map(Vec::len).sum::<usize>()),
        Some(1),
        "current pending activation must buffer despite the old turn epoch"
    );

    assert!(bridge
        .bind_submitted_turn_if_current("session-buffer-multi-ack-y", current, "turn-y")
        .await
        .is_empty());
    let state = bridge.active_threads.read().await;
    assert!(!state
        .provisional_nonterminal_events
        .contains_key("session-buffer-multi-ack-y"));
    assert!(!state
        .delivered_agent_text
        .contains_key("session-buffer-multi-ack-y"));
}

#[tokio::test]
async fn mismatched_ack_discards_provisional_nonterminal_events_without_projection_pollution() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-buffer-y").await;
    bridge
        .bind_observed_turn("session-buffer-y", "turn-x")
        .await;

    assert!(
        !bridge
            .record_delivered_projection(
                "session-buffer-y",
                "turn-x",
                &[
                    ChatStreamEvent::Token {
                        content: "stale token".into(),
                    },
                    ChatStreamEvent::Reasoning {
                        content: "stale reasoning".into(),
                    },
                    ChatStreamEvent::Error {
                        message: "stale error".into(),
                    },
                ],
            )
            .await
    );
    {
        let state = bridge.active_threads.read().await;
        assert!(!state.delivered_agent_text.contains_key("session-buffer-y"));
        assert!(!state.delivered_reasoning.contains_key("session-buffer-y"));
        assert!(!state
            .pending_terminal_errors
            .contains_key("session-buffer-y"));
    }

    assert!(bridge
        .bind_submitted_turn_if_current("session-buffer-y", activation, "turn-y")
        .await
        .is_empty());
    let state = bridge.active_threads.read().await;
    assert!(!state.delivered_agent_text.contains_key("session-buffer-y"));
    assert!(!state.delivered_reasoning.contains_key("session-buffer-y"));
    assert!(!state
        .pending_terminal_errors
        .contains_key("session-buffer-y"));
}

#[tokio::test]
async fn matching_ack_drains_provisional_nonterminal_events_before_terminal_in_order() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-buffer-x").await;
    bridge
        .bind_observed_turn("session-buffer-x", "turn-x")
        .await;
    let buffered = vec![
        ChatStreamEvent::RunStarted {
            thread_id: "session-buffer-x".into(),
            run_id: "turn-x".into(),
        },
        ChatStreamEvent::Token {
            content: "hello".into(),
        },
        ChatStreamEvent::Reasoning {
            content: "thinking".into(),
        },
        ChatStreamEvent::ToolCallDelta {
            index: 0,
            id: "tool-1".into(),
            name: "terminal".into(),
            arguments: "{}".into(),
        },
        ChatStreamEvent::Activity {
            message_id: "control-1".into(),
            activity_type: "control".into(),
            content_json: "{}".into(),
            replace: false,
        },
        ChatStreamEvent::Usage {
            prompt_tokens: 1,
            uncached_input_tokens: 1,
            completion_tokens: 2,
            total_tokens: 3,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            request_count: 1,
            provider_total_tokens: Some(3),
            cache_read_reported: true,
            cache_write_reported: false,
            reasoning_reported: true,
        },
        ChatStreamEvent::Error {
            message: "buffered error".into(),
        },
    ];
    assert!(
        !bridge
            .record_delivered_projection("session-buffer-x", "turn-x", &buffered)
            .await
    );
    assert!(bridge
        .accept_terminal("session-buffer-x", "turn-x", terminal_projection("turn-x"),)
        .await
        .is_empty());

    let released = bridge
        .bind_submitted_turn_if_current("session-buffer-x", activation, "turn-x")
        .await;
    assert!(matches!(
        released.as_slice(),
        [
            ChatStreamEvent::RunStarted { run_id, .. },
            ChatStreamEvent::Token { content },
            ChatStreamEvent::Reasoning { content: reasoning },
            ChatStreamEvent::ToolCallDelta { id, .. },
            ChatStreamEvent::Activity { message_id, .. },
            ChatStreamEvent::Usage { total_tokens: 3, .. },
            ChatStreamEvent::Error { message },
            ChatStreamEvent::RunFinished { .. },
            ChatStreamEvent::Done,
        ] if run_id == "turn-x"
            && content == "hello"
            && reasoning == "thinking"
            && id == "tool-1"
            && message_id == "control-1"
            && message == "buffered error"
    ));
}

#[tokio::test]
async fn replacement_and_failure_drop_provisional_nonterminal_buffers() {
    let bridge = ThreadEventsBridge::new();
    let replaced = bridge.activate("session-buffer-cleanup").await;
    bridge
        .bind_observed_turn("session-buffer-cleanup", "turn-old")
        .await;
    assert!(
        !bridge
            .record_delivered_projection(
                "session-buffer-cleanup",
                "turn-old",
                &[ChatStreamEvent::Token {
                    content: "replaced".into(),
                }],
            )
            .await
    );

    let failed = bridge.activate("session-buffer-cleanup").await;
    assert!(bridge
        .bind_submitted_turn_if_current("session-buffer-cleanup", replaced, "turn-old")
        .await
        .is_empty());
    bridge
        .bind_observed_turn("session-buffer-cleanup", "turn-failed")
        .await;
    assert!(
        !bridge
            .record_delivered_projection(
                "session-buffer-cleanup",
                "turn-failed",
                &[ChatStreamEvent::Token {
                    content: "failed".into(),
                }],
            )
            .await
    );
    assert!(
        bridge
            .fail_activation("session-buffer-cleanup", failed)
            .await
    );

    let current = bridge.activate("session-buffer-cleanup").await;
    assert!(bridge
        .bind_submitted_turn_if_current("session-buffer-cleanup", current, "turn-current")
        .await
        .is_empty());
}

#[tokio::test]
async fn provisional_nonterminal_buffer_applies_backpressure_at_live_channel_capacity() {
    let bridge = Arc::new(ThreadEventsBridge::new());
    let activation = bridge.activate("session-buffer-capacity").await;
    bridge
        .bind_observed_turn("session-buffer-capacity", "turn-x")
        .await;

    for index in 0..128 {
        assert!(
            !bridge
                .record_delivered_projection(
                    "session-buffer-capacity",
                    "turn-x",
                    &[ChatStreamEvent::Token {
                        content: format!("{index},"),
                    }],
                )
                .await
        );
    }

    let overflow_bridge = Arc::clone(&bridge);
    let overflow = tokio::spawn(async move {
        overflow_bridge
            .record_delivered_projection(
                "session-buffer-capacity",
                "turn-x",
                &[ChatStreamEvent::Token {
                    content: "overflow".into(),
                }],
            )
            .await
    });
    tokio::task::yield_now().await;
    assert!(
        !overflow.is_finished(),
        "the 129th event must backpressure until the ACK drain is delivered"
    );

    let released = bridge
        .bind_submitted_turn_if_current("session-buffer-capacity", activation, "turn-x")
        .await;
    assert_eq!(released.len(), 128);
    let delivery_acceptance =
        bridge.spawn_provisional_delivery_cleanup("session-buffer-capacity", activation);
    tokio::task::yield_now().await;
    assert!(
        !overflow.is_finished(),
        "the overflow event must remain behind the ACK batch delivery barrier"
    );
    let _ = delivery_acceptance.send(());
    assert!(overflow.await.unwrap());

    let state = bridge.active_threads.read().await;
    let delivered = &state.delivered_agent_text["session-buffer-capacity"]["turn-x"];
    assert!(delivered.ends_with("127,overflow"));
}

#[tokio::test]
async fn terminal_after_ack_drain_waits_for_provisional_delivery_finish() {
    let bridge = Arc::new(ThreadEventsBridge::new());
    let activation = bridge.activate("session-buffer-terminal-order").await;
    bridge
        .bind_observed_turn("session-buffer-terminal-order", "turn-x")
        .await;
    assert!(
        !bridge
            .record_delivered_projection(
                "session-buffer-terminal-order",
                "turn-x",
                &[ChatStreamEvent::Token {
                    content: "before terminal".into(),
                }],
            )
            .await
    );
    let released = bridge
        .bind_submitted_turn_if_current("session-buffer-terminal-order", activation, "turn-x")
        .await;
    assert!(matches!(
        released.as_slice(),
        [ChatStreamEvent::Token { content }] if content == "before terminal"
    ));
    let delivery_acceptance =
        bridge.spawn_provisional_delivery_cleanup("session-buffer-terminal-order", activation);

    let terminal_bridge = Arc::clone(&bridge);
    let terminal = tokio::spawn(async move {
        terminal_bridge
            .accept_terminal(
                "session-buffer-terminal-order",
                "turn-x",
                terminal_projection("turn-x"),
            )
            .await
    });
    tokio::task::yield_now().await;
    assert!(
        !terminal.is_finished(),
        "terminal must not overtake the ACK-drained projection batch"
    );

    let _ = delivery_acceptance.send(());
    let terminal = terminal.await.unwrap();
    assert!(matches!(
        terminal.as_slice(),
        [ChatStreamEvent::RunFinished { .. }, ChatStreamEvent::Done]
    ));
}

#[tokio::test]
async fn aborted_delivery_owner_releases_capacity_and_terminal_waiters() {
    let bridge = Arc::new(ThreadEventsBridge::new());
    let activation = bridge.activate("session-buffer-owner-abort").await;
    bridge
        .bind_observed_turn("session-buffer-owner-abort", "turn-x")
        .await;
    for _ in 0..PROVISIONAL_EVENT_BUFFER_CAPACITY {
        assert!(
            !bridge
                .record_delivered_projection(
                    "session-buffer-owner-abort",
                    "turn-x",
                    &[ChatStreamEvent::Token {
                        content: "buffered".into(),
                    }],
                )
                .await
        );
    }
    let overflow_bridge = Arc::clone(&bridge);
    let overflow = tokio::spawn(async move {
        overflow_bridge
            .record_delivered_projection(
                "session-buffer-owner-abort",
                "turn-x",
                &[ChatStreamEvent::Token {
                    content: "overflow".into(),
                }],
            )
            .await
    });
    tokio::task::yield_now().await;
    assert!(!overflow.is_finished());

    let released = bridge
        .bind_submitted_turn_if_current("session-buffer-owner-abort", activation, "turn-x")
        .await;
    assert_eq!(released.len(), PROVISIONAL_EVENT_BUFFER_CAPACITY);
    let (armed_tx, armed_rx) = oneshot::channel();
    let owner_bridge = Arc::clone(&bridge);
    let owner = tokio::spawn(async move {
        let acceptance = owner_bridge
            .spawn_provisional_delivery_cleanup("session-buffer-owner-abort", activation);
        let _ = armed_tx.send(());
        std::future::pending::<()>().await;
        drop(acceptance);
    });
    armed_rx.await.unwrap();
    tokio::task::yield_now().await;
    assert!(
        !overflow.is_finished(),
        "cleanup must not release before emit acceptance or owner cancellation"
    );

    owner.abort();
    let _ = owner.await;
    assert!(tokio::time::timeout(Duration::from_secs(1), overflow)
        .await
        .expect("owner cancellation must release the delivery barrier")
        .unwrap());
    let terminal = bridge
        .accept_terminal(
            "session-buffer-owner-abort",
            "turn-x",
            terminal_projection("turn-x"),
        )
        .await;
    assert!(matches!(
        terminal.as_slice(),
        [ChatStreamEvent::RunFinished { .. }, ChatStreamEvent::Done]
    ));
}

#[tokio::test]
async fn replacement_and_failure_release_provisional_capacity_waiters() {
    let bridge = Arc::new(ThreadEventsBridge::new());
    let replaced = bridge.activate("session-buffer-waiter-cleanup").await;
    bridge
        .bind_observed_turn("session-buffer-waiter-cleanup", "turn-old")
        .await;
    for _ in 0..PROVISIONAL_EVENT_BUFFER_CAPACITY {
        assert!(
            !bridge
                .record_delivered_projection(
                    "session-buffer-waiter-cleanup",
                    "turn-old",
                    &[ChatStreamEvent::Token {
                        content: "old".into(),
                    }],
                )
                .await
        );
    }
    let replaced_waiter_bridge = Arc::clone(&bridge);
    let replaced_waiter = tokio::spawn(async move {
        replaced_waiter_bridge
            .record_delivered_projection(
                "session-buffer-waiter-cleanup",
                "turn-old",
                &[ChatStreamEvent::Token {
                    content: "stale".into(),
                }],
            )
            .await
    });
    tokio::task::yield_now().await;
    assert!(!replaced_waiter.is_finished());

    let failed = bridge.activate("session-buffer-waiter-cleanup").await;
    assert!(!replaced_waiter.await.unwrap());
    assert!(bridge
        .bind_submitted_turn_if_current("session-buffer-waiter-cleanup", replaced, "turn-old",)
        .await
        .is_empty());
    bridge
        .bind_observed_turn("session-buffer-waiter-cleanup", "turn-failed")
        .await;
    for _ in 0..PROVISIONAL_EVENT_BUFFER_CAPACITY {
        assert!(
            !bridge
                .record_delivered_projection(
                    "session-buffer-waiter-cleanup",
                    "turn-failed",
                    &[ChatStreamEvent::Token {
                        content: "failed".into(),
                    }],
                )
                .await
        );
    }
    let failed_waiter_bridge = Arc::clone(&bridge);
    let failed_waiter = tokio::spawn(async move {
        failed_waiter_bridge
            .record_delivered_projection(
                "session-buffer-waiter-cleanup",
                "turn-failed",
                &[ChatStreamEvent::Token {
                    content: "stale".into(),
                }],
            )
            .await
    });
    tokio::task::yield_now().await;
    assert!(!failed_waiter.is_finished());

    assert!(
        bridge
            .fail_activation("session-buffer-waiter-cleanup", failed)
            .await
    );
    assert!(!failed_waiter.await.unwrap());
}

#[tokio::test]
async fn nonterminal_projection_requires_the_current_turn_epoch() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-fence").await;
    bridge
        .bind_submitted_turn_if_current("session-fence", old, "turn-old")
        .await;
    bridge.forget_thread("session-fence").await;

    assert!(
        !bridge
            .record_delivered_projection(
                "session-fence",
                "turn-old",
                &[ChatStreamEvent::Token {
                    content: "stale".into(),
                }],
            )
            .await
    );

    let current = bridge.activate("session-fence").await;
    assert!(
        !bridge
            .record_delivered_projection(
                "session-fence",
                "turn-old",
                &[ChatStreamEvent::Token {
                    content: "still stale".into(),
                }],
            )
            .await
    );
    bridge
        .bind_submitted_turn_if_current("session-fence", current, "turn-new")
        .await;
    assert!(
        bridge
            .record_delivered_projection(
                "session-fence",
                "turn-new",
                &[ChatStreamEvent::Token {
                    content: "current".into(),
                }],
            )
            .await
    );
}

#[tokio::test]
async fn pre_ack_terminal_is_released_by_authoritative_steered_binding() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", old, "turn-1")
        .await;

    let current = bridge.activate("session-1").await;
    assert!(bridge
        .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
        .await
        .is_empty());
    assert!(bridge.is_active("session-1").await);

    let released = bridge
        .bind_submitted_turn_if_current("session-1", current, "turn-1")
        .await;
    assert!(matches!(
        released.as_slice(),
        [
            ChatStreamEvent::RunFinished {
                run_id,
                outcome_type,
                ..
            },
            ChatStreamEvent::Done
        ] if run_id == "turn-1" && outcome_type == "success"
    ));
    assert!(!bridge.is_active("session-1").await);
}

#[tokio::test]
async fn completed_snapshot_without_turn_started_defers_until_matching_ack() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-snapshot").await;

    assert!(bridge
        .accept_terminal_with_background(
            "session-snapshot",
            "turn-snapshot",
            terminal_projection("turn-snapshot"),
            true,
        )
        .await
        .is_empty());
    assert_eq!(
        deferred_terminal_count(&bridge, "session-snapshot").await,
        1
    );
    assert!(bridge.is_active("session-snapshot").await);

    assert!(!bridge
        .bind_submitted_turn_if_current("session-snapshot", activation, "turn-snapshot",)
        .await
        .is_empty());
    assert!(!bridge.is_active("session-snapshot").await);
    assert_eq!(
        bridge.background_resume_threads().await,
        vec!["session-snapshot"]
    );
}

#[tokio::test]
async fn unknown_terminal_tracks_latest_pending_activation_across_multiple_acks() {
    let bridge = ThreadEventsBridge::new();
    let first = bridge.activate("session-multi-ack").await;
    let current = bridge.activate("session-multi-ack").await;

    assert!(bridge
        .accept_terminal(
            "session-multi-ack",
            "turn-unknown",
            terminal_projection("turn-unknown"),
        )
        .await
        .is_empty());
    assert_eq!(
        deferred_terminal_count(&bridge, "session-multi-ack").await,
        1,
        "an unknown terminal belongs to the latest still-pending activation"
    );

    assert!(bridge
        .bind_submitted_turn_if_current("session-multi-ack", first, "turn-unknown",)
        .await
        .is_empty());
    assert!(bridge.is_active("session-multi-ack").await);

    assert!(!bridge
        .bind_submitted_turn_if_current("session-multi-ack", current, "turn-unknown",)
        .await
        .is_empty());
    assert!(!bridge.is_active("session-multi-ack").await);
}

#[tokio::test]
async fn preterminal_marker_tracks_latest_activation_with_multiple_pending_acks() {
    let bridge = ThreadEventsBridge::new();
    let first = bridge.activate("session-marker-acks").await;
    let current = bridge.activate("session-marker-acks").await;

    assert!(
        !bridge
            .complete_background_turn("session-marker-acks", "turn-marker")
            .await
    );
    assert!(bridge
        .accept_terminal_with_background(
            "session-marker-acks",
            "turn-marker",
            terminal_projection("turn-marker"),
            true,
        )
        .await
        .is_empty());
    assert!(bridge
        .bind_submitted_turn_if_current("session-marker-acks", first, "turn-marker",)
        .await
        .is_empty());
    assert!(!bridge
        .bind_submitted_turn_if_current("session-marker-acks", current, "turn-marker",)
        .await
        .is_empty());
    assert!(bridge.background_resume_threads().await.is_empty());
}

#[tokio::test]
async fn snapshot_marker_before_terminal_without_turn_started_survives_delayed_ack() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-snapshot").await;
    let marker = proto::ThreadExtension {
        item_id: "turn-snapshot:background_complete".into(),
        namespace: "astro.background_complete".into(),
        payload_json: "{}".into(),
    };
    assert!(
        bridge
            .observe_extension("session-snapshot", "turn-snapshot", &marker)
            .await
    );
    assert!(bridge
        .accept_terminal_with_background(
            "session-snapshot",
            "turn-snapshot",
            terminal_projection("turn-snapshot"),
            true,
        )
        .await
        .is_empty());

    assert!(!bridge
        .bind_submitted_turn_if_current("session-snapshot", activation, "turn-snapshot",)
        .await
        .is_empty());
    assert!(bridge.background_resume_threads().await.is_empty());
}

#[tokio::test]
async fn completed_snapshot_terminal_cannot_clear_different_ack_turn() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-snapshot").await;
    assert!(bridge
        .accept_terminal_with_background(
            "session-snapshot",
            "turn-snapshot",
            terminal_projection("turn-snapshot"),
            true,
        )
        .await
        .is_empty());

    assert!(bridge
        .bind_submitted_turn_if_current("session-snapshot", activation, "turn-other")
        .await
        .is_empty());
    assert!(bridge.is_active("session-snapshot").await);
    assert_eq!(
        deferred_terminal_count(&bridge, "session-snapshot").await,
        0
    );
    assert!(bridge.background_resume_threads().await.is_empty());
}

#[tokio::test]
async fn completion_marker_before_deferred_terminal_ack_does_not_recreate_background_pending() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", old, "turn-1")
        .await;
    let current = bridge.activate("session-1").await;

    assert!(
        bridge
            .accept_terminal_with_background(
                "session-1",
                "turn-1",
                terminal_projection("turn-1"),
                true,
            )
            .await
            .is_empty()
    );
    assert_eq!(deferred_terminal_count(&bridge, "session-1").await, 1);

    assert!(!bridge.complete_background_turn("session-1", "turn-1").await);
    let terminal = bridge
        .bind_submitted_turn_if_current("session-1", current, "turn-1")
        .await;
    assert!(!terminal.is_empty());
    assert!(
        bridge.background_resume_threads().await.is_empty(),
        "a completion marker observed before the ACK must prevent pending resurrection"
    );
    assert!(!bridge
        .active_threads
        .read()
        .await
        .completed_background_turns
        .contains_key("session-1"));
}

#[tokio::test]
async fn deduped_completion_marker_still_advances_background_state() {
    let bridge = ThreadEventsBridge::new();
    let marker = proto::ThreadExtension {
        item_id: "turn-1:complete".into(),
        namespace: "astro.background_complete".into(),
        payload_json: "{}".into(),
    };
    assert!(
        bridge
            .accept_extension("session-1", &marker.item_id, &marker.payload_json)
            .await
    );

    let old = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", old, "turn-1")
        .await;
    let current = bridge.activate("session-1").await;
    assert!(
        bridge
            .accept_terminal_with_background(
                "session-1",
                "turn-1",
                terminal_projection("turn-1"),
                true,
            )
            .await
            .is_empty()
    );

    assert!(
        !bridge
            .observe_extension("session-1", "turn-1", &marker)
            .await
    );
    assert!(!bridge
        .bind_submitted_turn_if_current("session-1", current, "turn-1")
        .await
        .is_empty());
    assert!(bridge.background_resume_threads().await.is_empty());
}

#[tokio::test]
async fn submission_failure_clears_its_deferred_terminal() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", old, "turn-1")
        .await;
    let failed = bridge.activate("session-1").await;
    assert!(bridge
        .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
        .await
        .is_empty());
    assert_eq!(deferred_terminal_count(&bridge, "session-1").await, 1);

    assert!(bridge.fail_activation("session-1", failed).await);
    assert_eq!(deferred_terminal_count(&bridge, "session-1").await, 0);

    let next = bridge.activate("session-1").await;
    assert!(bridge
        .bind_submitted_turn_if_current("session-1", next, "turn-1")
        .await
        .is_empty());
    assert!(bridge.is_active("session-1").await);
}

#[tokio::test]
async fn replacement_activation_clears_older_deferred_terminal() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", old, "turn-1")
        .await;
    let replaced = bridge.activate("session-1").await;
    assert!(bridge
        .accept_terminal("session-1", "turn-1", terminal_projection("turn-1"))
        .await
        .is_empty());
    assert_eq!(deferred_terminal_count(&bridge, "session-1").await, 1);

    let current = bridge.activate("session-1").await;
    assert_eq!(deferred_terminal_count(&bridge, "session-1").await, 0);
    assert!(bridge
        .bind_submitted_turn_if_current("session-1", replaced, "turn-1")
        .await
        .is_empty());
    assert!(bridge
        .bind_submitted_turn_if_current("session-1", current, "turn-1")
        .await
        .is_empty());
    assert!(bridge.is_active("session-1").await);
}

#[tokio::test]
async fn start_chat_registers_new_epoch_before_blocked_ready_wait() {
    let bridge = ThreadEventsBridge::new();
    let old = bridge.activate("session-1").await;
    bridge.mark_recovering();

    // The invocation is registered synchronously even though its RPC must wait for recovery.
    let current = bridge.activate("session-1").await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), bridge.wait_ready())
            .await
            .is_err()
    );

    let old_is_current = bridge.fail_activation("session-1", old).await;
    assert!(submission_failure_events(old_is_current, "old failure").is_empty());
    assert!(bridge.is_active("session-1").await);

    assert!(bridge.fail_activation("session-1", current).await);
    assert!(!bridge.is_active("session-1").await);

    // Keep the command integration honest: activation must happen before the task can block
    // on readiness, otherwise the state assertions above do not describe `start_chat`.
    let source = include_str!("../commands/chat/core.rs");
    let activation = source
        .find("let activation = bridge.activate(sid2.clone()).await;")
        .expect("start_chat activation marker");
    let spawn = activation
        + source[activation..]
            .find("tauri::async_runtime::spawn(async move {")
            .expect("start_chat spawn marker");
    let wait_ready = spawn
        + source[spawn..]
            .find(".wait_ready_for(THREAD_EVENTS_READY_TIMEOUT)")
            .expect("start_chat readiness marker");
    assert!(activation < spawn && spawn < wait_ready);
}

#[tokio::test]
async fn ready_state_cannot_lose_a_wakeup() {
    let bridge = std::sync::Arc::new(ThreadEventsBridge::new());
    bridge.set_ready(true);
    tokio::time::timeout(std::time::Duration::from_millis(50), bridge.wait_ready())
        .await
        .expect("already-ready state must return immediately");
}

#[tokio::test]
async fn reconnect_is_not_publicly_ready_until_snapshot_barrier_finishes() {
    let bridge = std::sync::Arc::new(ThreadEventsBridge::new());
    bridge.mark_recovering();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), bridge.wait_ready())
            .await
            .is_err()
    );
    bridge.mark_recovered();
    tokio::time::timeout(std::time::Duration::from_millis(50), bridge.wait_ready())
        .await
        .expect("completed recovery must release submitters");
}

#[tokio::test]
async fn completed_snapshot_only_recovers_missing_agent_text_before_done() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await;
    bridge
        .record_delivered_projection(
            "session-1",
            "turn-1",
            &[
                ChatStreamEvent::Token {
                    content: "hello".into(),
                },
                ChatStreamEvent::Error {
                    message: "boom".into(),
                },
            ],
        )
        .await;

    let recovered = bridge
        .recover_snapshot_projection(
            "session-1",
            "turn-1",
            vec![
                ChatStreamEvent::Token {
                    content: "hello world".into(),
                },
                ChatStreamEvent::Error {
                    message: "boom".into(),
                },
                ChatStreamEvent::RunFinished {
                    run_id: "turn-1".into(),
                    outcome_type: "success".into(),
                    interrupts_json: "[]".into(),
                },
                ChatStreamEvent::Done,
            ],
        )
        .await;
    let recovered = bridge
        .dedup_terminal_projection("session-1", "turn-1", recovered)
        .await;

    assert!(matches!(
        recovered.as_slice(),
        [
            ChatStreamEvent::Token { content },
            ChatStreamEvent::RunFinished { .. },
            ChatStreamEvent::Done
        ] if content == " world"
    ));
}

#[tokio::test]
async fn divergent_snapshot_uses_canonical_text_reconciliation() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await;
    bridge
        .record_delivered_projection(
            "session-1",
            "turn-1",
            &[ChatStreamEvent::Token {
                content: "hel world".into(),
            }],
        )
        .await;

    let recovered = bridge
        .recover_snapshot_projection(
            "session-1",
            "turn-1",
            vec![ChatStreamEvent::Token {
                content: "hello world".into(),
            }],
        )
        .await;

    assert!(matches!(
        recovered.as_slice(),
        [ChatStreamEvent::TextReconcile { content }] if content == "hello world"
    ));
}

#[tokio::test]
async fn completed_empty_agent_message_clears_delivered_draft_before_terminal() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await;
    bridge
        .record_delivered_projection(
            "session-1",
            "turn-1",
            &[ChatStreamEvent::Token {
                content: "draft".into(),
            }],
        )
        .await;
    let snapshot = proto::ThreadSnapshot {
        thread_id: "session-1".into(),
        status: "completed".into(),
        provider_id: None,
        backend_id: None,
        model: None,
        reasoning_effort: None,
        turns: vec![proto::ThreadTurn {
            id: "turn-1".into(),
            status: "completed".into(),
            items: vec![agent_message_item("message-1", "")],
            last_agent_message: String::new(),
            error: None,
            has_error: false,
        }],
        active_turn: None,
        has_active_turn: false,
        pending_background_turn_ids: vec![],
    };

    let reconciled = reconcile_snapshot(&snapshot);
    let recovered = bridge
        .recover_snapshot_projection("session-1", "turn-1", reconciled.terminal)
        .await;

    assert!(matches!(
        recovered.as_slice(),
        [
            ChatStreamEvent::TextReconcile { content },
            ChatStreamEvent::RunFinished { .. },
            ChatStreamEvent::Done
        ] if content.is_empty()
    ));
}

#[tokio::test]
async fn running_snapshot_recovers_only_missing_reasoning_before_buffered_live() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await;
    bridge
        .record_delivered_projection(
            "session-1",
            "turn-1",
            &[ChatStreamEvent::Reasoning {
                content: "seen".into(),
            }],
        )
        .await;
    let snapshot = proto::ThreadSnapshot {
        thread_id: "session-1".into(),
        status: "running".into(),
        provider_id: None,
        backend_id: None,
        model: None,
        reasoning_effort: None,
        turns: vec![],
        active_turn: Some(proto::ThreadTurn {
            id: "turn-1".into(),
            status: "in_progress".into(),
            items: vec![reasoning_item("reasoning-1", "seenlost")],
            last_agent_message: String::new(),
            error: None,
            has_error: false,
        }),
        has_active_turn: true,
        pending_background_turn_ids: vec![],
    };

    let reconciled = reconcile_snapshot(&snapshot);
    let mut projected = bridge
        .recover_snapshot_projection("session-1", "turn-1", reconciled.terminal)
        .await;
    projected.extend(map_thread_event(proto::ThreadEvent {
        thread_id: "session-1".into(),
        turn_id: "turn-1".into(),
        payload: Some(proto::thread_event::Payload::ReasoningDelta(
            proto::ThreadDelta {
                item_id: "reasoning-1".into(),
                delta: "next".into(),
            },
        )),
    }));

    assert!(matches!(
        projected.as_slice(),
        [
            ChatStreamEvent::Reasoning { content: missing },
            ChatStreamEvent::Reasoning { content: live }
        ] if missing == "lost" && live == "next"
    ));
}

#[tokio::test]
async fn running_snapshot_recovers_full_reasoning_after_total_disconnect() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await;

    let recovered = bridge
        .recover_snapshot_projection(
            "session-1",
            "turn-1",
            snapshot_turn_recovery_events(&proto::ThreadTurn {
                id: "turn-1".into(),
                status: "in_progress".into(),
                items: vec![reasoning_item("reasoning-1", "full")],
                last_agent_message: String::new(),
                error: None,
                has_error: false,
            }),
        )
        .await;

    assert!(matches!(
        recovered.as_slice(),
        [ChatStreamEvent::Reasoning { content }] if content == "full"
    ));
}

#[tokio::test]
async fn divergent_snapshot_uses_canonical_reasoning_reconciliation() {
    let bridge = ThreadEventsBridge::new();
    let activation = bridge.activate("session-1").await;
    bridge
        .bind_submitted_turn_if_current("session-1", activation, "turn-1")
        .await;
    bridge
        .record_delivered_projection(
            "session-1",
            "turn-1",
            &[ChatStreamEvent::Reasoning {
                content: "hel world".into(),
            }],
        )
        .await;

    let recovered = bridge
        .recover_snapshot_projection(
            "session-1",
            "turn-1",
            vec![ChatStreamEvent::Reasoning {
                content: "hello world".into(),
            }],
        )
        .await;
    let serialized = serde_json::to_value(&recovered).unwrap();

    assert_eq!(serialized[0]["type"], "reasoning_reconcile");
    assert_eq!(serialized[0]["content"], "hello world");
}

#[tokio::test]
async fn recovery_boundary_drains_events_queued_during_snapshot_rpc() {
    let bridge = ThreadEventsBridge::new();
    bridge.mark_recovering();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    tx.send(RecoveryIngress::Event(Ok(proto::ThreadEvent {
        thread_id: "session-1".into(),
        turn_id: "turn-1".into(),
        payload: Some(proto::thread_event::Payload::AgentMessageDelta(
            proto::ThreadDelta {
                item_id: "message-1".into(),
                delta: "before-ready".into(),
            },
        )),
    })))
    .await
    .unwrap();
    tx.send(RecoveryIngress::Boundary).await.unwrap();
    tx.send(RecoveryIngress::Event(Ok(proto::ThreadEvent {
        thread_id: "session-1".into(),
        turn_id: "turn-1".into(),
        payload: Some(proto::thread_event::Payload::AgentMessageDelta(
            proto::ThreadDelta {
                item_id: "message-1".into(),
                delta: "after-ready".into(),
            },
        )),
    })))
    .await
    .unwrap();

    let recovery = receive_recovery_batch(&mut rx).await.unwrap();
    assert!(!bridge.is_ready());
    assert_eq!(recovery.len(), 1);
    assert!(matches!(
        rx.recv().await,
        Some(RecoveryIngress::Event(Ok(_)))
    ));
    bridge.complete_recovery(Ok(())).unwrap();
    assert!(bridge.is_ready());
}

#[tokio::test]
async fn ready_wait_timeout_only_retires_its_own_activation() {
    let bridge = ThreadEventsBridge::new();
    let mut cleanup = bridge.take_terminal_unsubscribe_requests();
    bridge.mark_recovering();
    let timed_out = bridge.activate("session-1").await;
    assert!(bridge
        .wait_ready_for(Duration::from_millis(1))
        .await
        .is_err());

    let current = bridge.activate("session-1").await;
    let timed_out_is_current = bridge.fail_activation("session-1", timed_out).await;
    assert!(submission_failure_events(timed_out_is_current, "backend unavailable").is_empty());
    assert!(cleanup.try_recv().is_err());
    assert!(bridge.is_active("session-1").await);
    assert!(bridge.fail_activation("session-1", current).await);
}

#[tokio::test]
async fn current_ready_timeout_emits_one_error_terminal_sequence() {
    let bridge = ThreadEventsBridge::new();
    let mut cleanup = bridge.take_terminal_unsubscribe_requests();
    bridge.mark_recovering();
    let activation = bridge.activate("session-1").await;
    let error = bridge
        .wait_ready_for(Duration::from_millis(1))
        .await
        .unwrap_err();
    let is_current = bridge.fail_activation("session-1", activation).await;
    let projected = submission_failure_events(is_current, error);

    assert!(matches!(
        projected.as_slice(),
        [
            ChatStreamEvent::Error { .. },
            ChatStreamEvent::RunFinished { outcome_type, .. },
            ChatStreamEvent::Done
        ] if outcome_type == "error"
    ));
    assert!(!bridge.is_active("session-1").await);
    assert_eq!(
        cleanup
            .recv()
            .await
            .expect("ready timeout cleanup")
            .activation,
        activation
    );
}

#[tokio::test]
async fn failed_resume_cannot_publish_connection_as_ready() {
    let bridge = ThreadEventsBridge::new();
    bridge.mark_recovering();
    assert!(bridge
        .complete_recovery(Err("resume failed".to_string()))
        .is_err());
    assert!(!bridge.is_ready());
    bridge.complete_recovery(Ok(())).unwrap();
    assert!(bridge.is_ready());
}

#[test]
fn emitted_snapshot_keeps_turn_items_and_error_shape() {
    let snapshot = proto::ThreadSnapshot {
        thread_id: "session-1".into(),
        status: "errored".into(),
        provider_id: Some("provider-profile".into()),
        backend_id: Some("openai".into()),
        model: Some("gpt-5.6".into()),
        reasoning_effort: Some("high".into()),
        turns: vec![proto::ThreadTurn {
            id: "turn-1".into(),
            status: "failed".into(),
            items: vec![proto::ThreadItem {
                id: "tool-1".into(),
                item_type: "command_execution".into(),
                status: "failed".into(),
                payload_json: r#"{"type":"command_execution"}"#.into(),
            }],
            last_agent_message: String::new(),
            error: Some(proto::ThreadError {
                message: "boom".into(),
                error_type: "provider".into(),
            }),
            has_error: true,
        }],
        active_turn: None,
        has_active_turn: false,
        pending_background_turn_ids: vec![],
    };
    let value = serde_json::to_value(snapshot_dto(&snapshot)).unwrap();
    assert_eq!(value["turns"][0]["items"][0]["id"], "tool-1");
    assert_eq!(value["turns"][0]["error"]["errorType"], "provider");
    assert_eq!(value["model"], "gpt-5.6");
    assert_eq!(value["reasoningEffort"], "high");
    assert_eq!(value["providerId"], "provider-profile");
    assert_eq!(value["backendId"], "openai");
}

#[test]
fn reconnect_backoff_starts_at_500ms_and_caps_at_15s() {
    let mut backoff = RetryBackoff::default();
    assert_eq!(
        backoff.after_attempt(false),
        std::time::Duration::from_millis(500)
    );
    assert_eq!(
        backoff.after_attempt(false),
        std::time::Duration::from_secs(1)
    );
    assert_eq!(
        backoff.after_attempt(false),
        std::time::Duration::from_secs(2)
    );
    for _ in 0..10 {
        backoff.after_attempt(false);
    }
    assert_eq!(
        backoff.after_attempt(false),
        std::time::Duration::from_secs(15)
    );
    assert_eq!(
        backoff.after_attempt(true),
        std::time::Duration::from_millis(500)
    );
    assert_eq!(
        backoff.after_attempt(false),
        std::time::Duration::from_secs(1)
    );
}

#[test]
fn session_status_timestamps_are_strictly_monotonic() {
    let first = next_session_status_ts_ms();
    let second = next_session_status_ts_ms();
    assert!(second > first);
}

#[test]
fn session_status_snapshot_returns_the_latest_value() {
    let session_id = format!("snapshot-{}", uuid::Uuid::new_v4());
    let first = SessionStatusChangedDto {
        session_id: session_id.clone(),
        status: "active".into(),
        active_flags: vec!["waitingOnUserInput".into()],
        error: None,
        ts_ms: next_session_status_ts_ms(),
    };
    let latest = SessionStatusChangedDto {
        session_id: session_id.clone(),
        status: "idle".into(),
        active_flags: Vec::new(),
        error: None,
        ts_ms: next_session_status_ts_ms(),
    };
    {
        let mut statuses = session_status_registry().lock().unwrap();
        statuses.insert(session_id.clone(), first);
        statuses.insert(session_id.clone(), latest.clone());
    }

    let snapshot = session_status_snapshot();
    let restored = snapshot
        .iter()
        .find(|status| status.session_id == session_id)
        .expect("status should be present in snapshot");
    assert_eq!(restored.status, latest.status);
    assert_eq!(restored.ts_ms, latest.ts_ms);

    session_status_registry()
        .lock()
        .unwrap()
        .remove(&session_id);
}

#[test]
fn not_submitted_response_is_an_error() {
    let response = proto::SubmitTurnResponse {
        submission_id: "submission-1".into(),
        turn_id: String::new(),
        disposition: "not_submitted".into(),
        reason: "thread is busy".into(),
    };
    assert_eq!(accepted_turn_id(response).unwrap_err(), "thread is busy");
}

#[test]
fn started_response_returns_turn_id() {
    let response = proto::SubmitTurnResponse {
        submission_id: "submission-1".into(),
        turn_id: "turn-1".into(),
        disposition: "started".into(),
        reason: String::new(),
    };
    assert_eq!(accepted_turn_id(response).unwrap(), "turn-1");
}

#[test]
fn stale_submission_failure_has_no_terminal_projection() {
    assert!(submission_failure_events(false, "old failure").is_empty());
    assert!(matches!(
        submission_failure_events(true, "current failure").as_slice(),
        [
            ChatStreamEvent::Error { .. },
            ChatStreamEvent::RunFinished { outcome_type, .. },
            ChatStreamEvent::Done
        ] if outcome_type == "error"
    ));
}
