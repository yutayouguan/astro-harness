//! 多轮流式与 `PauseControl` 集成测试。

use std::sync::Arc;
use std::time::Duration;

use providers::types::stream::StreamChunk;
use providers::{CompletionStream, ProviderConfig};
use providers::{PauseControl, Usage};
use tokio::sync::Mutex;

use agent::runtime::{AgentConfig, AgentLoop};
use agent::streaming::{
    run_multi_turn_stream_with_chat_fn, ChatOverride, MultiTurnStreamItem, StreamedAssistantContent,
};

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
async fn multi_turn_emits_text_tool_result_and_usage() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "test-session".into()).unwrap();
    let session = Arc::new(Mutex::new(agent));
    {
        let mut a = session.lock().await;
        a.session_messages
            .push(types::message::Message::user("call a tool"));
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
        run_multi_turn_stream_with_chat_fn(
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
        Some(MultiTurnStreamItem::RunStarted { .. })
    ));
    assert!(items.iter().any(|i| matches!(
        i,
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Text(t)) if t == "thinking…"
    )));
    let tool_started_index = items
        .iter()
        .position(|i| matches!(i, MultiTurnStreamItem::ToolStarted { name, .. } if name == "echo"))
        .expect("tool started event");
    let tool_completed_index = items
        .iter()
        .position(|i| matches!(i, MultiTurnStreamItem::ToolResult { name, .. } if name == "echo"))
        .expect("tool completed event");
    assert!(tool_started_index < tool_completed_index);
    assert!(items.iter().any(|i| matches!(
        i,
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::FinalUsage(u))
        if u.prompt_tokens() == 22 && u.completion_tokens() == 8
    )));
    assert!(items.iter().any(|i| matches!(
        i,
        MultiTurnStreamItem::RunFinished {
            outcome_type,
            ..
        } if outcome_type == "success"
    )));
    assert!(matches!(items.last(), Some(MultiTurnStreamItem::Done)));
}

#[tokio::test]
async fn multi_turn_tool_exec_works_on_current_thread_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "current-thread-session".into()).unwrap();
    let session = Arc::new(Mutex::new(agent));
    session
        .lock()
        .await
        .session_messages
        .push(types::message::Message::user("call a tool"));

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

    run_multi_turn_stream_with_chat_fn(
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
        |item| matches!(item, MultiTurnStreamItem::ToolResult { name, .. } if name == "echo")
    ));
    assert!(items.iter().any(|item| matches!(
        item,
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Text(text))
            if text == "current-thread ok"
    )));
    assert!(matches!(items.last(), Some(MultiTurnStreamItem::Done)));
}

#[test]
fn cold_start_hydrates_session_messages_from_db() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    {
        let mut agent = AgentLoop::with_session_id(
            AgentConfig::with_defaults(path.clone()),
            "hydrate-me".into(),
        )
        .unwrap();
        agent.ensure_session("test").unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            agent.start_or_steer_turn("hello", "hydrate").await.unwrap();
        });
        agent.record_assistant_message("world").unwrap();
    }

    let agent =
        AgentLoop::with_session_id(AgentConfig::with_defaults(path), "hydrate-me".into()).unwrap();
    assert_eq!(agent.session_messages.len(), 2);
    assert_eq!(agent.session_messages[0].content_str(), "hello");
    assert_eq!(agent.session_messages[1].content_str(), "world");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_turn_persists_reasoning_and_tool_activities() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "persist-session".into()).unwrap();
    let session = Arc::new(Mutex::new(agent));
    {
        let mut a = session.lock().await;
        a.ensure_session("test").unwrap();
        a.session_messages
            .push(types::message::Message::user("call a tool"));
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
        run_multi_turn_stream_with_chat_fn(
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
    let mut agent = AgentLoop::with_session_id(config, "completion-hook".into()).unwrap();
    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    ::hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));
    agent
        .session_messages
        .push(types::message::Message::user("say hi"));
    let session = Arc::new(Mutex::new(agent));

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
        run_multi_turn_stream_with_chat_fn(
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
        events.iter().any(|e| e == "post_llm_call:5"),
        "expected post_llm_call for \"hello\" (5 chars), events={events:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transform_llm_output_replaces_before_post_llm_call() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let mut agent = AgentLoop::with_session_id(config, "transform-llm-session".into()).unwrap();
    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    ::hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));
    agent
        .hook_bus()
        .register(::hooks::TRANSFORM_LLM_OUTPUT, |_| {
            ::hooks::HookOutcome::ReplaceText("REPLACED".into())
        });
    agent
        .session_messages
        .push(types::message::Message::user("say hi"));
    let session = Arc::new(Mutex::new(agent));
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
        run_multi_turn_stream_with_chat_fn(
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
    let transform_idx = events.iter().position(|e| e == "transform_llm_output:11");
    let post_idx = events.iter().position(|e| e == "post_llm_call:8");
    assert!(
        transform_idx.is_some() && post_idx.is_some() && transform_idx < post_idx,
        "expected transform_llm_output before post_llm_call with replaced length, events={events:?}"
    );

    let agent = session_for_check.lock().await;
    assert_eq!(
        agent
            .session_messages
            .last()
            .map(|m| m.content_str().to_string()),
        Some("REPLACED".to_string()),
        "final assistant message should reflect transform_llm_output replacement"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_verify_never_fires_without_disk_write() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let mut agent = AgentLoop::with_session_id(config, "pre-verify-no-write".into()).unwrap();
    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    ::hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));
    agent
        .session_messages
        .push(types::message::Message::user("just say hi, no tools"));
    let session = Arc::new(Mutex::new(agent));

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
        run_multi_turn_stream_with_chat_fn(
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
        !events.iter().any(|e| e == "pre_verify"),
        "pre_verify must not fire when no disk write happened this turn, events={events:?}"
    );
    assert!(items.iter().any(|i| matches!(
        i,
        MultiTurnStreamItem::RunFinished { outcome_type, .. } if outcome_type == "success"
    )));
    assert!(matches!(items.last(), Some(MultiTurnStreamItem::Done)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_verify_keep_going_retries_capped_at_two() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let mut agent = AgentLoop::with_session_id(config, "pre-verify-keep-going".into()).unwrap();
    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    ::hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));
    agent.hook_bus().register(::hooks::PRE_VERIFY, |_| {
        ::hooks::HookOutcome::KeepGoing("请再检查一下你的改动".into())
    });
    agent
        .session_messages
        .push(types::message::Message::user("write a file then confirm"));
    let session = Arc::new(Mutex::new(agent));
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
        run_multi_turn_stream_with_chat_fn(
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
    let verify_count = events.iter().filter(|e| e.as_str() == "pre_verify").count();
    assert_eq!(
        verify_count, 2,
        "pre_verify attempts must be capped at MAX_VERIFY_ATTEMPTS=2, events={events:?}"
    );
    let api_request_count = events
        .iter()
        .filter(|e| e.as_str() == "pre_api_request")
        .count();
    assert_eq!(
        api_request_count, 4,
        "expect one pre_api_request round per LLM call (1 tool round + 2 keep-going + 1 final), events={events:?}"
    );
    let post_llm_count = events
        .iter()
        .filter(|e| e.starts_with("post_llm_call"))
        .count();
    assert_eq!(
        post_llm_count, 2,
        "post_llm_call must be skipped while pre_verify keeps going; only the tool round and the final round should fire it, events={events:?}"
    );

    assert!(items.iter().any(|i| matches!(
        i,
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Text(t)) if t == "final answer"
    )));
    assert!(items.iter().any(|i| matches!(
        i,
        MultiTurnStreamItem::RunFinished { outcome_type, .. } if outcome_type == "success"
    )));
    assert!(matches!(items.last(), Some(MultiTurnStreamItem::Done)));

    let agent = session_for_check.lock().await;
    assert!(
        agent::runtime::validate_message_order(&agent.session_messages),
        "session_messages must alternate roles (no consecutive same role) after capped KeepGoing retries, messages={:?}",
        agent
            .session_messages
            .iter()
            .map(|m| (m.role.clone(), m.content_str().to_string()))
            .collect::<Vec<_>>()
    );

    let bridge_users: Vec<&types::message::Message> = agent
        .session_messages
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
        agent
            .session_messages
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

    let provider_messages =
        agent::prompt::messages::to_provider_messages("sys", &agent.session_messages);
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
    drop(agent);
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
    let session = Arc::new(Mutex::new(agent));
    {
        let mut a = session.lock().await;
        a.session_messages.push(types::message::Message::user("hi"));
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
        run_multi_turn_stream_with_chat_fn(
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
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::FinalUsage(u))
        if u.prompt_tokens() == 10 && u.completion_tokens() == 5
    )));
    assert!(matches!(items.last(), Some(MultiTurnStreamItem::Done)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn error_has_single_error_terminal_before_done() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "err-session".into()).unwrap();
    let session = Arc::new(Mutex::new(agent));
    {
        let mut a = session.lock().await;
        a.session_messages.push(types::message::Message::user("x"));
    }
    let chat_fn = boom_chat();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    tokio::spawn(async move {
        run_multi_turn_stream_with_chat_fn(
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
        Some(MultiTurnStreamItem::RunStarted { .. })
    ));
    assert!(items
        .iter()
        .any(|i| matches!(i, MultiTurnStreamItem::Error(_))));
    let terminal_outcomes: Vec<&str> = items
        .iter()
        .filter_map(|item| match item {
            MultiTurnStreamItem::RunFinished { outcome_type, .. } => Some(outcome_type.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(terminal_outcomes, ["error"]);
    assert!(matches!(items.last(), Some(MultiTurnStreamItem::Done)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_has_single_interrupt_terminal_before_done() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "cancel-session".into()).unwrap();
    let session = Arc::new(Mutex::new(agent));
    {
        let mut agent = session.lock().await;
        agent
            .session_messages
            .push(types::message::Message::user("wait"));
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let run_pause = pause.clone();
    tokio::spawn(async move {
        run_multi_turn_stream_with_chat_fn(
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
    assert!(matches!(started, MultiTurnStreamItem::RunStarted { .. }));
    pause.cancel();

    let mut items = vec![started];
    while let Some(item) = rx.recv().await {
        items.push(item.unwrap());
    }
    assert!(!items
        .iter()
        .any(|item| matches!(item, MultiTurnStreamItem::Error(_))));
    let terminal_outcomes: Vec<&str> = items
        .iter()
        .filter_map(|item| match item {
            MultiTurnStreamItem::RunFinished { outcome_type, .. } => Some(outcome_type.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(terminal_outcomes, ["interrupt"]);
    assert!(matches!(items.last(), Some(MultiTurnStreamItem::Done)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_call_delta_and_memory_path() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "mem-session".into()).unwrap();
    let session = Arc::new(Mutex::new(agent));
    {
        let mut a = session.lock().await;
        a.session_messages
            .push(types::message::Message::user("remember this"));
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
        run_multi_turn_stream_with_chat_fn(
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
                MultiTurnStreamItem::Assistant(StreamedAssistantContent::ToolCallDelta(_))
            )
        })
        .count();
    assert!(delta_count >= 2, "expected streamed tool_call_deltas");
    assert!(items
        .iter()
        .any(|i| matches!(i, MultiTurnStreamItem::ToolResult { name, .. } if name == "memory")));
    assert!(
        items.iter().any(|i| {
            matches!(i, MultiTurnStreamItem::MemoryUpdate { op, .. } if op == "memory")
        }),
        "memory success should emit MemoryUpdate; got: {:?}",
        items.iter().map(|i| format!("{i:?}")).collect::<Vec<_>>()
    );
    assert!(matches!(items.last(), Some(MultiTurnStreamItem::Done)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hitl_waiting_parks_then_continues_same_run() {
    use agent::{HitlGate, ResumeItem};

    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "hitl-session".into()).unwrap();
    let session = Arc::new(Mutex::new(agent));
    {
        let mut a = session.lock().await;
        a.session_messages
            .push(types::message::Message::user("please confirm"));
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
        run_multi_turn_stream_with_chat_fn(
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
            Ok(Some(Ok(MultiTurnStreamItem::RunFinished {
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
        MultiTurnStreamItem::ToolResult { name, result, .. }
        if name == "ask_user" && result.contains("approved")
    )));
    assert!(items.iter().any(|i| matches!(
        i,
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Text(t)) if t == "confirmed"
    )));
    assert!(items.iter().any(|i| matches!(
        i,
        MultiTurnStreamItem::RunFinished { outcome_type, .. } if outcome_type == "success"
    )));
    assert!(matches!(items.last(), Some(MultiTurnStreamItem::Done)));
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
            *captured_pre2.lock().unwrap() =
                Some((payload.message.clone(), payload.detail.clone()));
            hooks::HookOutcome::Continue
        });
    let captured_post: CapturedApproval = Arc::new(std::sync::Mutex::new(None));
    let captured_post2 = Arc::clone(&captured_post);
    agent
        .hook_bus()
        .register(hooks::POST_APPROVAL_RESPONSE, move |payload| {
            *captured_post2.lock().unwrap() =
                Some((payload.message.clone(), payload.detail.clone()));
            hooks::HookOutcome::Continue
        });

    let session = Arc::new(Mutex::new(agent));
    {
        let mut a = session.lock().await;
        a.session_messages
            .push(types::message::Message::user("clean up the temp dir"));
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
        run_multi_turn_stream_with_chat_fn(
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
            Ok(Some(Ok(MultiTurnStreamItem::RunFinished {
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
    let pre_idx = events.iter().position(|e| e == "pre_approval_request");
    let post_idx = events.iter().position(|e| e == "post_approval_response");
    assert!(
        pre_idx.is_some() && post_idx.is_some() && pre_idx < post_idx,
        "expected pre_approval_request before post_approval_response, events={events:?}"
    );
    let post_tool_idx = events.iter().position(|e| e == "post_tool_call:terminal");
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

    let session = Arc::new(Mutex::new(agent));
    {
        let mut a = session.lock().await;
        a.session_messages
            .push(types::message::Message::user("clean up the temp dir"));
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
        run_multi_turn_stream_with_chat_fn(
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
            Ok(Some(Ok(MultiTurnStreamItem::RunFinished {
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
    let pre_idx = events.iter().position(|e| e == "pre_approval_request");
    let post_idx = events.iter().position(|e| e == "post_approval_response");
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
            MultiTurnStreamItem::ToolResult { name, result, .. }
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
    let session = Arc::new(Mutex::new(agent));
    {
        let mut a = session.lock().await;
        a.session_messages
            .push(types::message::Message::user("keep using tools"));
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
        run_multi_turn_stream_with_chat_fn(
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

    assert!(
        !items
            .iter()
            .any(|i| matches!(i, MultiTurnStreamItem::Error(e) if e.contains("轮次已用尽"))),
        "budget exhaustion should not hard-error"
    );
    assert!(items.iter().any(|i| matches!(
        i,
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Text(t))
        if t.contains("迭代预算已用尽")
    )));
    assert!(items.iter().any(|i| matches!(
        i,
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Text(t))
        if t == "summary-after-budget"
    )));
    assert!(items.iter().any(|i| matches!(
        i,
        MultiTurnStreamItem::RunFinished { outcome_type, .. } if outcome_type == "success"
    )));
    assert!(matches!(items.last(), Some(MultiTurnStreamItem::Done)));
}
