//! 多轮流式与 `PauseControl` 集成测试。

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use providers::streaming::{PauseControl, Usage};
use providers::trait_::{
    AiProvider, ChatProvider, CompletionStream, ProviderConfig, VerifyProvider, VerifyResult,
};
use providers::types::message::Message as ProviderMessage;
use providers::types::stream::StreamChunk;
use tokio::sync::Mutex;
use tools::ToolCallDelta;

use agent::runtime::{AgentConfig, AgentLoop};
use agent::streaming::{
    run_multi_turn_stream_from_provider, MultiTurnStreamItem, StreamedAssistantContent,
};

struct ScriptedProvider {
    rounds: Mutex<Vec<Vec<StreamChunk>>>,
}

#[async_trait]
impl ChatProvider for ScriptedProvider {
    async fn chat_stream(
        &self,
        _messages: Vec<ProviderMessage>,
        _tools: Vec<serde_json::Value>,
        _config: &ProviderConfig,
    ) -> anyhow::Result<CompletionStream> {
        let mut rounds = self.rounds.lock().await;
        let chunks = if rounds.is_empty() {
            vec![
                StreamChunk::Text("done".into()),
                StreamChunk::Done {
                    finish_reason: "stop".into(),
                },
            ]
        } else {
            rounds.remove(0)
        };
        Ok(Box::pin(futures::stream::iter(
            chunks.into_iter().map(Ok::<_, anyhow::Error>),
        )))
    }
}

#[async_trait]
impl VerifyProvider for ScriptedProvider {
    async fn verify(&self, model: &str, _config: &ProviderConfig) -> VerifyResult {
        VerifyResult {
            ok: true,
            latency_ms: 0,
            model: model.to_string(),
            message: "ok".into(),
        }
    }
}

#[async_trait]
impl AiProvider for ScriptedProvider {
    fn name(&self) -> &str {
        "scripted"
    }
    fn default_model(&self) -> &str {
        "test"
    }
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
            .push(common::message::Message::user("call a tool"));
    }

    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![
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
        ]),
    });

    // 确保 echo 工具存在：若没有，resolve 可能仍从 XML/空走；这里用原生 FC
    // Scripted 第二轮在工具结果后返回 ok；若 echo 不存在会得到工具错误字符串仍继续
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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
    assert!(items
        .iter()
        .any(|i| matches!(i, MultiTurnStreamItem::ToolResult { name, .. } if name == "echo")));
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
        // 经公开 API 写入：user 落盘 + assistant 镜像
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            agent.run_turn("hello", "hydrate").await.unwrap();
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
            .push(common::message::Message::user("call a tool"));
    }

    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![
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
        ]),
    });

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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
        .push(common::message::Message::user("say hi"));
    let session = Arc::new(Mutex::new(agent));

    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![vec![
            StreamChunk::Text("hello".into()),
            StreamChunk::Usage(Usage::from_parts(3, 2)),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ]]),
    });

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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
        .push(common::message::Message::user("say hi"));
    let session = Arc::new(Mutex::new(agent));
    let session_for_check = Arc::clone(&session);

    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![vec![
            StreamChunk::Text("hello world".into()),
            StreamChunk::Usage(Usage::from_parts(3, 2)),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ]]),
    });

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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
    // 原始 "hello world" 11 字符触发 transform_llm_output；替换为 "REPLACED"（8 字符）后 post_llm_call 应观察到新长度。
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
        .push(common::message::Message::user("just say hi, no tools"));
    let session = Arc::new(Mutex::new(agent));

    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![vec![
            StreamChunk::Text("hi there".into()),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ]]),
    });

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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
        .push(common::message::Message::user("write a file then confirm"));
    let session = Arc::new(Mutex::new(agent));
    let session_for_check = Arc::clone(&session);

    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![
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
        ]),
    });

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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

    // 回归：KeepGoing 桥接 user 消息必须持久化到 session_messages（而非只走
    // `pending_inject_context` 的临时注入），否则第二次 KeepGoing 时相邻两条都是
    // assistant，下一轮 API 历史会出现连续同角色，触发 Anthropic/Gemini 400。
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

    let bridge_users: Vec<&common::message::Message> = agent
        .session_messages
        .iter()
        .filter(|m| {
            m.role == common::message::Role::User
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

    // hydrate/reload 场景：转换为 Provider 消息后，相邻 user/assistant 仍不得连续同角色
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
        a.session_messages
            .push(common::message::Message::user("hi"));
    }
    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![vec![
            StreamChunk::Text("a".into()),
            StreamChunk::Usage(Usage::from_parts(1, 1)),
            StreamChunk::Text("b".into()),
            StreamChunk::Usage(Usage::from_parts(10, 5)),
            StreamChunk::Done {
                finish_reason: "stop".into(),
            },
        ]]),
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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
async fn error_is_followed_by_done() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let agent = AgentLoop::with_session_id(config, "err-session".into()).unwrap();
    let session = Arc::new(Mutex::new(agent));
    {
        let mut a = session.lock().await;
        a.session_messages.push(common::message::Message::user("x"));
    }
    struct Boom;
    #[async_trait]
    impl ChatProvider for Boom {
        async fn chat_stream(
            &self,
            _messages: Vec<ProviderMessage>,
            _tools: Vec<serde_json::Value>,
            _config: &ProviderConfig,
        ) -> anyhow::Result<CompletionStream> {
            Err(anyhow::anyhow!("boom"))
        }
    }
    #[async_trait]
    impl VerifyProvider for Boom {
        async fn verify(&self, model: &str, _config: &ProviderConfig) -> VerifyResult {
            VerifyResult {
                ok: false,
                latency_ms: 0,
                model: model.to_string(),
                message: "boom".into(),
            }
        }
    }
    #[async_trait]
    impl AiProvider for Boom {
        fn name(&self) -> &str {
            "boom"
        }
        fn default_model(&self) -> &str {
            "x"
        }
    }
    let provider: Arc<dyn AiProvider> = Arc::new(Boom);
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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
            .push(common::message::Message::user("remember this"));
    }

    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![
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
        ]),
    });

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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
            .push(common::message::Message::user("please confirm"));
    }

    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![
            vec![
                StreamChunk::ToolCallStart {
                    index: 0,
                    id: "call_confirm".into(),
                    name: "ask_user".into(),
                },
                StreamChunk::ToolCallDelta {
                    index: 0,
                    arguments: r#"{"mode":"confirm","title":"Delete?","body":"Really delete the file?"}"#
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
        ]),
    });

    let gate = HitlGate::new("hitl-session");
    let gate_resolve = gate.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();

    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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
            .push(common::message::Message::user("clean up the temp dir"));
    }

    let cmd = "rm -rf /tmp/astro-approval-test-allow";
    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![
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
        ]),
    });

    let gate = HitlGate::new("approval-allow-session");
    let gate_resolve = gate.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();

    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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
            .push(common::message::Message::user("clean up the temp dir"));
    }

    let cmd = "rm -rf /tmp/astro-approval-test-deny";
    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![
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
        ]),
    });

    let gate = HitlGate::new("approval-deny-session");
    let gate_resolve = gate.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();

    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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
            .push(common::message::Message::user("keep using tools"));
    }

    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![
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
        ]),
    });

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    tokio::spawn(async move {
        run_multi_turn_stream_from_provider(
            session,
            provider,
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
