//! 多轮流式与 `PauseControl` 集成测试。

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use providers::streaming::{PauseControl, Usage};
use providers::trait_::{
    AiProvider, ChatChunk, ChatMessage, ChatProvider, ChatStream, ProviderConfig,
    ToolCallDeltaChunk, VerifyProvider, VerifyResult,
};
use tokio::sync::Mutex;

use agent::loop_::{AgentConfig, AgentLoop};
use agent::streaming::{
    run_multi_turn_stream, MultiTurnStreamItem, StreamedAssistantContent,
};

struct ScriptedProvider {
    rounds: Mutex<Vec<Vec<ChatChunk>>>,
}

#[async_trait]
impl ChatProvider for ScriptedProvider {
    async fn chat_stream(
        &self,
        _messages: Vec<ChatMessage>,
        _tools: Vec<serde_json::Value>,
        _config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        let mut rounds = self.rounds.lock().await;
        let chunks = if rounds.is_empty() {
            vec![ChatChunk {
                token: Some("done".into()),
                finish_reason: Some("stop".into()),
                ..Default::default()
            }]
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
                ChatChunk {
                    token: Some("thinking…".into()),
                    ..Default::default()
                },
                ChatChunk {
                    tool_call_deltas: vec![ToolCallDeltaChunk {
                        index: 0,
                        id: Some("call_1".into()),
                        name: Some("echo".into()),
                        arguments: Some(r#"{"text":"hi"}"#.into()),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    usage: Some(Usage::from_parts(10, 5)),
                    ..Default::default()
                },
            ],
            vec![ChatChunk {
                token: Some("ok".into()),
                finish_reason: Some("stop".into()),
                usage: Some(Usage::from_parts(12, 3)),
                ..Default::default()
            }],
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
        run_multi_turn_stream(
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
        let mut agent =
            AgentLoop::with_session_id(AgentConfig::with_defaults(path.clone()), "hydrate-me".into())
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
                ChatChunk {
                    reasoning: Some("deep ".into()),
                    ..Default::default()
                },
                ChatChunk {
                    reasoning: Some("thought".into()),
                    ..Default::default()
                },
                ChatChunk {
                    token: Some("calling…".into()),
                    ..Default::default()
                },
                ChatChunk {
                    tool_call_deltas: vec![ToolCallDeltaChunk {
                        index: 0,
                        id: Some("call_persist".into()),
                        name: Some("echo".into()),
                        arguments: Some(r#"{"text":"hi"}"#.into()),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    usage: Some(Usage::from_parts(10, 5)),
                    ..Default::default()
                },
            ],
            vec![ChatChunk {
                token: Some("ok".into()),
                finish_reason: Some("stop".into()),
                usage: Some(Usage::from_parts(4, 2)),
                ..Default::default()
            }],
        ]),
    });

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_multi_turn_stream(
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

    let store = memory::SessionStore::open(&dir.path().join("sessions/state.db")).unwrap();
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
async fn multi_turn_fires_on_completion_after_model_stream() {
    use agent::hooks::RecordingHooks;

    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig::with_defaults(dir.path().to_path_buf());
    let mut agent = AgentLoop::with_session_id(config, "completion-hook".into()).unwrap();
    let hooks = Arc::new(RecordingHooks::new());
    agent.set_hooks(hooks.clone());
    agent
        .session_messages
        .push(common::message::Message::user("say hi"));
    let session = Arc::new(Mutex::new(agent));

    let provider: Arc<dyn AiProvider> = Arc::new(ScriptedProvider {
        rounds: Mutex::new(vec![vec![ChatChunk {
            token: Some("hello".into()),
            finish_reason: Some("stop".into()),
            usage: Some(Usage::from_parts(3, 2)),
            ..Default::default()
        }]]),
    });

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    let cfg = ProviderConfig {
        model: "test".into(),
        ..Default::default()
    };

    tokio::spawn(async move {
        run_multi_turn_stream(
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

    let events = hooks.snapshot();
    assert!(
        events.iter().any(|e| e == "completion:5"),
        "expected on_completion for \"hello\" (5 chars), events={events:?}"
    );
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
            ChatChunk {
                token: Some("a".into()),
                usage: Some(Usage::from_parts(1, 1)),
                ..Default::default()
            },
            ChatChunk {
                token: Some("b".into()),
                finish_reason: Some("stop".into()),
                usage: Some(Usage::from_parts(10, 5)),
                ..Default::default()
            },
        ]]),
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    tokio::spawn(async move {
        run_multi_turn_stream(
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
        a.session_messages
            .push(common::message::Message::user("x"));
    }
    struct Boom;
    #[async_trait]
    impl ChatProvider for Boom {
        async fn chat_stream(
            &self,
            _messages: Vec<ChatMessage>,
            _tools: Vec<serde_json::Value>,
            _config: &ProviderConfig,
        ) -> anyhow::Result<ChatStream> {
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
        fn name(&self) -> &str { "boom" }
        fn default_model(&self) -> &str { "x" }
    }
    let provider: Arc<dyn AiProvider> = Arc::new(Boom);
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    tokio::spawn(async move {
        run_multi_turn_stream(
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
async fn tool_call_delta_and_memory_add_path() {
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
            vec![ChatChunk {
                tool_call_deltas: vec![
                    ToolCallDeltaChunk {
                        index: 0,
                        id: Some("c1".into()),
                        name: Some("memory_add".into()),
                        arguments: Some(r#"{"entry":""#.into()),
                    },
                    ToolCallDeltaChunk {
                        index: 0,
                        id: None,
                        name: None,
                        arguments: Some(r#"hello from test","target":"project"}"#.into()),
                    },
                ],
                finish_reason: Some("tool_calls".into()),
                ..Default::default()
            }],
            vec![ChatChunk {
                token: Some("saved".into()),
                finish_reason: Some("stop".into()),
                ..Default::default()
            }],
        ]),
    });

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();
    tokio::spawn(async move {
        run_multi_turn_stream(
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
        .any(|i| matches!(i, MultiTurnStreamItem::ToolResult { name, .. } if name == "memory_add")));
    assert!(
        items.iter().any(|i| {
            matches!(i, MultiTurnStreamItem::MemoryUpdate { op, .. } if op == "memory_add")
        }),
        "memory_add success should emit MemoryUpdate; got: {:?}",
        items
            .iter()
            .map(|i| format!("{i:?}"))
            .collect::<Vec<_>>()
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
            vec![ChatChunk {
                tool_call_deltas: vec![ToolCallDeltaChunk {
                    index: 0,
                    id: Some("call_confirm".into()),
                    name: Some("confirm".into()),
                    arguments: Some(
                        r#"{"title":"Delete?","body":"Really delete the file?"}"#.into(),
                    ),
                }],
                finish_reason: Some("tool_calls".into()),
                ..Default::default()
            }],
            vec![ChatChunk {
                token: Some("confirmed".into()),
                finish_reason: Some("stop".into()),
                ..Default::default()
            }],
        ]),
    });

    let gate = HitlGate::new("hitl-session");
    let gate_resolve = gate.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let pause = PauseControl::new();

    tokio::spawn(async move {
        run_multi_turn_stream(
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
        if name == "confirm" && result.contains("approved")
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
