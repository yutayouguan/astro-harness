//! 多轮流式与 `PauseControl` 集成测试。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use providers::types::stream::StreamChunk;
use providers::{CompletionStream, ProviderConfig};
use providers::{PauseControl, Usage};
use tokio::sync::Notify;

use agent::runtime::{AgentConfig, AgentLoop};
use agent::streaming::{
    run_multi_turn_stream, run_multi_turn_stream_with_chat_fn, ChatOverride, MultiTurnStreamArgs,
    MultiTurnStreamItem, StreamedAssistantContent,
};
use agent::TurnInput;

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

    run_multi_turn_stream(MultiTurnStreamArgs {
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
        input: vec![TurnInput::UserInput {
            content: "owned by regular task".into(),
            image_data_urls: Vec::new(),
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
    let sess = session.as_ref();
    let history = sess.clone_history().await;
    assert_eq!(history[0].content_str(), "owned by regular task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn steer_during_initial_prompt_preparation_preserves_role_order() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let session =
        Arc::new(AgentLoop::with_session_id(config, "early-steer-role-order".into()).unwrap());
    let initial_hook_entered = Arc::new(Notify::new());
    let initial_hook_release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let follow_up_hook_hits = Arc::new(AtomicUsize::new(0));
    let entered = Arc::clone(&initial_hook_entered);
    let release = Arc::clone(&initial_hook_release);
    let follow_up_hits = Arc::clone(&follow_up_hook_hits);
    session
        .hook_bus()
        .register(hooks::USER_PROMPT_SUBMIT, move |input| {
            match input.prompt.as_deref() {
                Some("initial") => {
                    entered.notify_one();
                    let (released, ready) = &*release;
                    let mut released = released.lock().unwrap();
                    while !*released {
                        released = ready.wait(released).unwrap();
                    }
                }
                Some("follow up") => {
                    follow_up_hits.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            }
            hooks::HookOutcome::Continue
        });

    let calls = Arc::new(AtomicUsize::new(0));
    let first_sampling_was_clean = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let second_sampling_saw_follow_up = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release_first_sampling = Arc::new(Notify::new());
    let chat_fn: ChatOverride = {
        let calls = Arc::clone(&calls);
        let first_sampling_was_clean = Arc::clone(&first_sampling_was_clean);
        let second_sampling_saw_follow_up = Arc::clone(&second_sampling_saw_follow_up);
        let release_first_sampling = Arc::clone(&release_first_sampling);
        Arc::new(move |messages, _tools, _config| {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            let texts: Vec<&str> = messages
                .iter()
                .map(|message| message.text_content())
                .collect();
            if call == 0 {
                first_sampling_was_clean.store(
                    texts.iter().any(|text| *text == "initial")
                        && !texts.iter().any(|text| *text == "follow up"),
                    Ordering::SeqCst,
                );
            } else if call == 1 {
                let first = texts.iter().position(|text| *text == "first");
                let follow_up = texts.iter().position(|text| *text == "follow up");
                second_sampling_saw_follow_up.store(
                    matches!((first, follow_up), (Some(first), Some(follow_up)) if first < follow_up),
                    Ordering::SeqCst,
                );
            }
            let text = if call == 0 { "first" } else { "second" };
            let release_first_sampling = Arc::clone(&release_first_sampling);
            Box::pin(async move {
                if call == 0 {
                    release_first_sampling.notified().await;
                }
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
            run_multi_turn_stream(MultiTurnStreamArgs {
                session,
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
                input: vec![TurnInput::UserInput {
                    content: "initial".into(),
                    image_data_urls: Vec::new(),
                }],
                system_prompt: None,
                pause: PauseControl::new(),
                hitl_gate: None,
                tx,
                chat_override: Some(chat_fn),
            })
            .await;
        }
    });

    initial_hook_entered.notified().await;
    let steer_started = Arc::new(Notify::new());
    let steer = tokio::spawn({
        let session = Arc::clone(&session);
        let steer_started = Arc::clone(&steer_started);
        async move {
            steer_started.notify_one();
            session.steer_input("follow up", &[]).await
        }
    });
    steer_started.notified().await;
    assert_eq!(follow_up_hook_hits.load(Ordering::SeqCst), 0);
    {
        let (released, ready) = &*initial_hook_release;
        *released.lock().unwrap() = true;
        ready.notify_all();
    }
    let steered_turn_id = steer
        .await
        .unwrap()
        .unwrap()
        .expect("active task accepts early steer");
    assert!(!steered_turn_id.is_empty());
    release_first_sampling.notify_one();
    tokio::time::timeout(Duration::from_secs(2), async {
        while rx.recv().await.is_some() {}
        run.await.unwrap();
    })
    .await
    .expect("stream completes after releasing initial prompt hook");

    assert_eq!(follow_up_hook_hits.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(first_sampling_was_clean.load(Ordering::SeqCst));
    assert!(second_sampling_saw_follow_up.load(Ordering::SeqCst));
    let history = session.clone_history().await;
    assert!(agent::runtime::validate_message_order(&history));
    assert_eq!(
        history
            .iter()
            .map(|message| message.content_str())
            .collect::<Vec<_>>(),
        ["initial", "first", "follow up", "second"]
    );
    assert_eq!(
        history
            .iter()
            .map(|message| message.role.clone())
            .collect::<Vec<_>>(),
        [
            types::message::Role::User,
            types::message::Role::Assistant,
            types::message::Role::User,
            types::message::Role::Assistant,
        ]
    );
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
    let prompt_hook_payload = Arc::new(std::sync::Mutex::new(None));
    let prompt_hook_hits = Arc::new(AtomicUsize::new(0));
    let prompt_hook_entered = Arc::new(Notify::new());
    let prompt_hook_release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let capture = Arc::clone(&prompt_hook_payload);
    let hook_hits = Arc::clone(&prompt_hook_hits);
    let hook_entered = Arc::clone(&prompt_hook_entered);
    let hook_release = Arc::clone(&prompt_hook_release);
    session
        .hook_bus()
        .register(hooks::USER_PROMPT_SUBMIT, move |input| {
            hook_hits.fetch_add(1, Ordering::SeqCst);
            *capture.lock().unwrap() = Some((input.prompt.clone(), input.turn_id.clone()));
            hook_entered.notify_one();
            let (released, ready) = &*hook_release;
            let mut released = released.lock().unwrap();
            while !*released {
                released = ready.wait(released).unwrap();
            }
            hooks::HookOutcome::InjectContext("STEER_CONTEXT".into())
        });

    let calls = Arc::new(AtomicUsize::new(0));
    let saw_follow_up = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_hook_context = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let first_started = Arc::new(Notify::new());
    let release_first = Arc::new(Notify::new());
    let chat_fn: ChatOverride = {
        let calls = Arc::clone(&calls);
        let saw_follow_up = Arc::clone(&saw_follow_up);
        let saw_hook_context = Arc::clone(&saw_hook_context);
        let first_started = Arc::clone(&first_started);
        let release_first = Arc::clone(&release_first);
        Arc::new(move |messages, _tools, _config| {
            let calls = Arc::clone(&calls);
            let saw_follow_up = Arc::clone(&saw_follow_up);
            let saw_hook_context = Arc::clone(&saw_hook_context);
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
                if call > 0
                    && messages
                        .iter()
                        .any(|message| message.text_content().contains("STEER_CONTEXT"))
                {
                    saw_hook_context.store(true, Ordering::SeqCst);
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
            run_multi_turn_stream_with_chat_fn(
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
    let steer = tokio::spawn({
        let session = Arc::clone(&session);
        async move { session.steer_input("follow up", &[]).await }
    });
    prompt_hook_entered.notified().await;
    release_first.notify_one();
    let terminal_waited = tokio::time::timeout(Duration::from_millis(200), async {
        while !run.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .is_err();
    {
        let (released, ready) = &*prompt_hook_release;
        *released.lock().unwrap() = true;
        ready.notify_all();
    }
    let steer_result = steer.await.unwrap().unwrap();
    while rx.recv().await.is_some() {}
    run.await.unwrap();

    assert!(terminal_waited, "terminal closed during prompt admission");
    let turn_id = steer_result.expect("active regular task accepts steer");
    assert!(!turn_id.is_empty());
    let captured = prompt_hook_payload
        .lock()
        .unwrap()
        .clone()
        .expect("prompt hook fires before provider release");
    assert_eq!(captured.0.as_deref(), Some("follow up"));
    assert_eq!(captured.1.as_deref(), Some(turn_id.as_str()));

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(prompt_hook_hits.load(Ordering::SeqCst), 1);
    assert!(saw_follow_up.load(Ordering::SeqCst));
    assert!(saw_hook_context.load(Ordering::SeqCst));
    let messages = session.clone_history().await;
    assert!(messages.iter().any(|message| {
        matches!(
            &message.content,
            types::message::MessageContent::Text(text) if text == "follow up"
        )
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_steers_preserve_submission_order() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let session =
        Arc::new(AgentLoop::with_session_id(config, "concurrent-steer-order".into()).unwrap());
    session
        .record_items(vec![types::message::Message::user("initial")])
        .await;

    let hook_order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let first_hook_entered = Arc::new(Notify::new());
    let second_hook_entered = Arc::new(Notify::new());
    let first_hook_release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let captured_order = Arc::clone(&hook_order);
    let first_entered = Arc::clone(&first_hook_entered);
    let second_entered = Arc::clone(&second_hook_entered);
    let hook_release = Arc::clone(&first_hook_release);
    session
        .hook_bus()
        .register(hooks::USER_PROMPT_SUBMIT, move |input| {
            let prompt = input.prompt.clone().unwrap();
            captured_order.lock().unwrap().push(prompt.clone());
            if prompt == "first steer" {
                first_entered.notify_one();
                let (released, ready) = &*hook_release;
                let mut released = released.lock().unwrap();
                while !*released {
                    released = ready.wait(released).unwrap();
                }
                hooks::HookOutcome::InjectContext("FIRST_CONTEXT".into())
            } else {
                second_entered.notify_one();
                hooks::HookOutcome::InjectContext("SECOND_CONTEXT".into())
            }
        });

    let provider_calls = Arc::new(AtomicUsize::new(0));
    let first_sampling_started = Arc::new(Notify::new());
    let release_first_sampling = Arc::new(Notify::new());
    let saw_fifo_input = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_fifo_context = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let chat_fn: ChatOverride = {
        let provider_calls = Arc::clone(&provider_calls);
        let first_sampling_started = Arc::clone(&first_sampling_started);
        let release_first_sampling = Arc::clone(&release_first_sampling);
        let saw_fifo_input = Arc::clone(&saw_fifo_input);
        let saw_fifo_context = Arc::clone(&saw_fifo_context);
        Arc::new(move |messages, _tools, _config| {
            let provider_calls = Arc::clone(&provider_calls);
            let first_sampling_started = Arc::clone(&first_sampling_started);
            let release_first_sampling = Arc::clone(&release_first_sampling);
            let saw_fifo_input = Arc::clone(&saw_fifo_input);
            let saw_fifo_context = Arc::clone(&saw_fifo_context);
            Box::pin(async move {
                let call = provider_calls.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    first_sampling_started.notify_one();
                    release_first_sampling.notified().await;
                } else if call == 1 {
                    saw_fifo_input.store(
                        messages
                            .iter()
                            .any(|message| message.text_content() == "first steer\n\nsecond steer"),
                        Ordering::SeqCst,
                    );
                    let joined = messages
                        .iter()
                        .map(|message| message.text_content())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let first = joined.find("FIRST_CONTEXT");
                    let second = joined.find("SECOND_CONTEXT");
                    saw_fifo_context.store(
                        matches!((first, second), (Some(first), Some(second)) if first < second),
                        Ordering::SeqCst,
                    );
                }
                let text = if call == 0 {
                    "first answer"
                } else {
                    "second answer"
                };
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
            run_multi_turn_stream_with_chat_fn(
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

    first_sampling_started.notified().await;
    let first_steer = tokio::spawn({
        let session = Arc::clone(&session);
        async move { session.steer_input("first steer", &[]).await }
    });
    first_hook_entered.notified().await;
    let second_call_started = Arc::new(Notify::new());
    let second_steer = tokio::spawn({
        let session = Arc::clone(&session);
        let second_call_started = Arc::clone(&second_call_started);
        async move {
            second_call_started.notify_one();
            session.steer_input("second steer", &[]).await
        }
    });
    second_call_started.notified().await;
    let second_hook_ran_before_release =
        tokio::time::timeout(Duration::from_millis(200), second_hook_entered.notified())
            .await
            .is_ok();
    {
        let (released, ready) = &*first_hook_release;
        *released.lock().unwrap() = true;
        ready.notify_all();
    }

    first_steer.await.unwrap().unwrap().unwrap();
    second_steer.await.unwrap().unwrap().unwrap();
    release_first_sampling.notify_one();
    tokio::time::timeout(Duration::from_secs(2), async {
        while rx.recv().await.is_some() {}
        run.await.unwrap();
    })
    .await
    .expect("stream completes after both steers commit");

    assert!(!second_hook_ran_before_release);
    assert_eq!(
        hook_order.lock().unwrap().as_slice(),
        ["first steer", "second steer"]
    );
    assert_eq!(provider_calls.load(Ordering::SeqCst), 2);
    assert!(saw_fifo_input.load(Ordering::SeqCst));
    assert!(saw_fifo_context.load(Ordering::SeqCst));
    assert_eq!(
        session
            .clone_history()
            .await
            .iter()
            .map(|message| message.content_str())
            .collect::<Vec<_>>(),
        [
            "initial",
            "first answer",
            "first steer\n\nsecond steer",
            "second answer",
        ]
    );
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
async fn hydrated_session_starts_with_resume_source() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    {
        let agent = AgentLoop::with_session_id(
            AgentConfig::with_defaults(path.clone()),
            "resume-hooks".into(),
        )
        .unwrap();
        agent.ensure_session("test").unwrap();
        agent.start_or_steer_turn("first", "t1").await.unwrap();
        agent.record_assistant_message("answer").await.unwrap();
    }
    let agent = AgentLoop::with_session_id(AgentConfig::with_defaults(path), "resume-hooks".into())
        .unwrap();
    let source = Arc::new(std::sync::Mutex::new(None));
    let capture = Arc::clone(&source);
    agent
        .hook_bus()
        .register(hooks::SESSION_START, move |input| {
            *capture.lock().unwrap() = input.source.clone();
            hooks::HookOutcome::Continue
        });

    agent.start_or_steer_turn("second", "t2").await.unwrap();

    assert_eq!(source.lock().unwrap().as_deref(), Some("resume"));
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
    let expected = format!("{}:5", ::hooks::POST_LLM_CALL);
    assert!(
        events.iter().any(|e| e == &expected),
        "expected PostLlmCall for \"hello\" (5 chars), events={events:?}"
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
    let transform_event = format!("{}:11", ::hooks::TRANSFORM_LLM_OUTPUT);
    let post_event = format!("{}:8", ::hooks::POST_LLM_CALL);
    let transform_idx = events.iter().position(|e| e == &transform_event);
    let post_idx = events.iter().position(|e| e == &post_event);
    assert!(
        transform_idx.is_some() && post_idx.is_some() && transform_idx < post_idx,
        "expected TransformLlmOutput before PostLlmCall with replaced length, events={events:?}"
    );

    let agent = session_for_check.as_ref();
    let history = agent.clone_history().await;
    assert_eq!(
        history.last().map(|m| m.content_str().to_string()),
        Some("REPLACED".to_string()),
        "final assistant message should reflect TransformLlmOutput replacement"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_fires_without_disk_write() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "pre-verify-no-write".into()).unwrap();
    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    ::hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));
    let stop_inputs = Arc::new(std::sync::Mutex::new(Vec::new()));
    let inputs = Arc::clone(&stop_inputs);
    agent.hook_bus().register(::hooks::STOP, move |input| {
        inputs.lock().unwrap().push((
            input.stop_hook_active,
            input.last_assistant_message.clone(),
            input.turn_id.clone(),
        ));
        ::hooks::HookOutcome::Continue
    });
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
    let stop_inputs = stop_inputs.lock().unwrap().clone();
    assert_eq!(
        stop_inputs.len(),
        1,
        "Stop must fire once for a terminal text-only turn, inputs={stop_inputs:?}"
    );
    assert_eq!(stop_inputs[0].0, Some(false));
    assert_eq!(stop_inputs[0].1.as_deref(), Some("hi there"));
    assert!(
        stop_inputs[0].2.is_some(),
        "Stop must receive the runtime turn id, inputs={stop_inputs:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.as_str() == ::hooks::STOP)
            .count(),
        1,
        "Stop must fire once for a terminal text-only turn, events={events:?}"
    );
    assert!(items.iter().any(|i| matches!(
        i,
        MultiTurnStreamItem::RunFinished { outcome_type, .. } if outcome_type == "success"
    )));
    assert!(matches!(items.last(), Some(MultiTurnStreamItem::Done)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_keep_going_retries_capped_at_two() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "pre-verify-keep-going".into()).unwrap();
    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    ::hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));
    let stop_flags = Arc::new(std::sync::Mutex::new(Vec::new()));
    let flags = Arc::clone(&stop_flags);
    agent.hook_bus().register(::hooks::STOP, move |input| {
        flags.lock().unwrap().push(input.stop_hook_active);
        ::hooks::HookOutcome::KeepGoing("请再检查一下你的改动".into())
    });
    agent
        .record_items(vec![types::message::Message::user("confirm")])
        .await;
    let session = Arc::new(agent);
    let session_for_check = Arc::clone(&session);

    let chat_fn = scripted_chat(vec![
        // round 1: 无工具终态草稿一 -> Stop attempt 1 -> KeepGoing
        vec![
            StreamChunk::Text("draft one".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
        // round 2: 无工具终态草稿二 -> Stop attempt 2 -> KeepGoing
        vec![
            StreamChunk::Text("draft two".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ],
        // round 3: 尝试次数已达上限，直接收尾
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
    assert_eq!(
        stop_flags.lock().unwrap().as_slice(),
        [Some(false), Some(true)],
        "Stop must mark only the second continuation as active"
    );
    let stop_count = events
        .iter()
        .filter(|e| e.as_str() == ::hooks::STOP)
        .count();
    assert_eq!(
        stop_count, 2,
        "Stop attempts must be capped at MAX_VERIFY_ATTEMPTS=2, events={events:?}"
    );
    let api_request_count = events
        .iter()
        .filter(|e| e.as_str() == ::hooks::PRE_API_REQUEST)
        .count();
    assert_eq!(
        api_request_count, 3,
        "expect one PreApiRequest round per LLM call (2 keep-going + 1 final), events={events:?}"
    );
    let post_llm_count = events
        .iter()
        .filter(|e| e.starts_with(::hooks::POST_LLM_CALL))
        .count();
    assert_eq!(
        post_llm_count, 1,
        "PostLlmCall must be skipped while Stop keeps going; only the final round should fire it, events={events:?}"
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
    type CapturedApproval = Arc<std::sync::Mutex<Option<(Option<serde_json::Value>, String)>>>;
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
        .register(hooks::PERMISSION_REQUEST, move |payload| {
            *captured_pre2.lock().unwrap() =
                Some((payload.tool_input.clone(), payload.detail.clone()));
            hooks::HookOutcome::Continue
        });
    let captured_post: CapturedApproval = Arc::new(std::sync::Mutex::new(None));
    let captured_post2 = Arc::clone(&captured_post);
    agent
        .hook_bus()
        .register(hooks::POST_APPROVAL_RESPONSE, move |payload| {
            *captured_post2.lock().unwrap() =
                Some((payload.tool_input.clone(), payload.detail.clone()));
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
    let pre_idx = events.iter().position(|e| e == "PermissionRequest:Bash");
    let post_idx = events.iter().position(|e| e == "PostApprovalResponse:Bash");
    assert!(
        pre_idx.is_some() && post_idx.is_some() && pre_idx < post_idx,
        "expected PermissionRequest before PostApprovalResponse, events={events:?}"
    );
    let post_tool_idx = events.iter().position(|e| e == "PostToolUse:terminal");
    assert!(
        post_idx < post_tool_idx,
        "expected PostApprovalResponse before PostToolUse, events={events:?}"
    );
    assert!(
        events.iter().any(|e| e == "PreToolUse:terminal"),
        "events={events:?}"
    );
    assert!(
        !events.iter().any(|e| e.contains("pre_tool_call")),
        "events={events:?}"
    );

    let pre = captured_pre
        .lock()
        .unwrap()
        .clone()
        .expect("PermissionRequest payload captured");
    assert_eq!(
        pre.0
            .as_ref()
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str()),
        Some(cmd),
        "pre tool_input.command should be the command"
    );
    assert!(
        pre.0
            .as_ref()
            .and_then(|v| v.get("description"))
            .and_then(|v| v.as_str())
            .is_some_and(|description| !description.is_empty()),
        "pre tool_input.description should contain the request summary: {:?}",
        pre.0
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
        .expect("PostApprovalResponse payload captured");
    assert_eq!(
        post.0, pre.0,
        "post tool_input should preserve command and request summary"
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
    let pre_idx = events.iter().position(|e| e == "PermissionRequest:Bash");
    let post_idx = events.iter().position(|e| e == "PostApprovalResponse:Bash");
    assert!(
        pre_idx.is_some() && post_idx.is_some() && pre_idx < post_idx,
        "expected PermissionRequest before PostApprovalResponse, events={events:?}"
    );

    let post_detail = captured_post
        .lock()
        .unwrap()
        .clone()
        .expect("PostApprovalResponse payload captured");
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
    let session = Arc::new(agent);
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
