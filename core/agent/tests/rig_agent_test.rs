//! `AgentBuilder` 构建与会话集成测试。

use std::sync::Arc;

use agent::builder::AgentBuilder;
use agent::prompt::context::{DynamicContext, StaticContext};
use agent::prompt::prompt_builder::PromptBuilder;
use agent::runtime::{AgentConfig, AgentLoop, MaxDepthError};
use common::message::Message;
use home::AgentRuntimeConfig;
use tempfile::TempDir;

fn test_config(dir: &TempDir) -> AgentConfig {
    AgentConfig::with_defaults(dir.path().to_path_buf())
}

#[tokio::test]
async fn test_agent_loop_creates_task_id() {
    let dir = TempDir::new().unwrap();
    let agent = AgentLoop::new(test_config(&dir)).unwrap();
    let task_id = agent.new_task_id();
    assert!(!task_id.is_empty());
    assert_eq!(task_id.len(), 36);
}

#[tokio::test]
async fn test_turn_budget_enforcement() {
    let dir = TempDir::new().unwrap();
    let mut config = test_config(&dir);
    config.max_turns = 2;
    let mut agent = AgentLoop::new(config).unwrap();
    assert!(!agent.is_budget_exhausted());
    agent.increment_turn();
    agent.increment_turn();
    assert!(agent.is_budget_exhausted());
}

#[tokio::test]
async fn test_multi_turn_max_depth() {
    let dir = TempDir::new().unwrap();
    let mut config = test_config(&dir);
    config.multi_turn = 2;
    let mut agent = AgentLoop::new(config).unwrap();
    agent.begin_user_turn();
    agent.increment_tool_round().unwrap();
    agent.increment_tool_round().unwrap();
    let err = agent.increment_tool_round().unwrap_err();
    assert!(matches!(err, MaxDepthError { limit: 2, used: 2 }));
}

#[tokio::test]
async fn test_message_alternation_validation() {
    use agent::runtime::validate_message_order;
    let messages = vec![
        Message::system("You are an assistant"),
        Message::user("Hello"),
        Message::assistant("Hi there!"),
    ];
    assert!(validate_message_order(&messages));
    let invalid = vec![Message::user("First"), Message::user("Second")];
    assert!(!validate_message_order(&invalid));
}

#[tokio::test]
async fn test_prompt_builder_static_dynamic_layers() {
    let static_ctx = StaticContext::from_workspace_files(
        "你是 Astro",
        "记得用户喜欢 Rust",
        "称呼：老板",
        "今天开了周会",
    );
    let mut dynamic = DynamicContext::new(2);
    dynamic.push("召回片段 A");
    dynamic.push("召回片段 B");
    dynamic.push("召回片段 C");

    let prompt = PromptBuilder::new()
        .with_static_context(&static_ctx)
        .with_dynamic_context(&dynamic)
        .with_timestamp()
        .build();

    assert!(prompt.contains("Astro"));
    assert!(prompt.contains("Rust"));
    assert!(prompt.contains("老板"));
    assert!(prompt.contains("周会"));
    assert!(prompt.contains("召回片段 A"));
    assert!(prompt.contains("召回片段 B"));
    assert!(!prompt.contains("召回片段 C")); // top-2
    assert!(prompt.contains("时间"));
}

#[tokio::test]
async fn test_agent_builder_from_runtime_config() {
    let dir = TempDir::new().unwrap();
    let cfg = AgentRuntimeConfig {
        id: "coder".into(),
        name: "Coder".into(),
        inherit_from: None,
        provider_id: Some("openai".into()),
        model: Some("gpt-4o".into()),
        temperature: Some(0.3),
        max_turns: Some(5),
        additional_params: Some(serde_json::json!({"foo": "bar"})),
        tools_enabled: None,
        mcp: None,
        created_at: String::new(),
    };

    let (agent, spec) = AgentBuilder::new(dir.path())
        .from_runtime_config(&cfg)
        .static_context(StaticContext::from_workspace_files(
            "preamble", "mem", "user", "",
        ))
        .dynamic_context(4)
        .build()
        .unwrap();

    assert_eq!(spec.temperature, 0.3);
    assert_eq!(spec.multi_turn, 5);
    assert_eq!(spec.additional_params["foo"], "bar");
    assert_eq!(agent.temperature(), 0.3);
    assert_eq!(agent.multi_turn(), 5);
    assert_eq!(agent.additional_params()["foo"], "bar");
}

#[tokio::test]
async fn test_prompt_hooks_on_run_turn() {
    let dir = TempDir::new().unwrap();
    let (mut agent, _) = AgentBuilder::new(dir.path())
        .preamble("你是测试助手")
        .build()
        .unwrap();
    let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(vec![]));
    ::hooks::install_recording(&agent.hook_bus(), Arc::clone(&log));

    let _ = agent.run_turn("你好", "t1").await.unwrap();
    let events = log.lock().unwrap().clone();
    assert!(
        events.iter().any(|e| e.starts_with("pre_llm_call:")),
        "events={events:?}"
    );
    // on_session_end 在 streaming 收尾触发；run_turn 仅准备阶段
    assert!(
        events.iter().any(|e| *e == "on_session_start"),
        "events={events:?}"
    );
}

#[tokio::test]
async fn pre_tool_call_block_via_hook_bus() {
    let dir = TempDir::new().unwrap();
    let mut agent = AgentLoop::new(test_config(&dir)).unwrap();
    let bus = agent.hook_bus();
    bus.register(hooks::PRE_TOOL_CALL, |_| {
        hooks::HookOutcome::Block("denied-by-test".into())
    });
    let out = agent
        .handle_tool_call_async("echo", &serde_json::json!({"text": "hi"}))
        .await
        .unwrap();
    assert!(
        out.text().contains("blocked by hook") && out.text().contains("denied-by-test"),
        "out={:?}",
        out
    );
}

#[tokio::test]
async fn pre_llm_call_inject_context_via_hook_bus() {
    let dir = TempDir::new().unwrap();
    let mut agent = AgentLoop::new(test_config(&dir)).unwrap();
    let bus = agent.hook_bus();
    bus.register(hooks::PRE_LLM_CALL, |_| {
        hooks::HookOutcome::InjectContext("tz=Asia/Shanghai".into())
    });
    let _ = agent.run_turn("你好", "inject-test").await.unwrap();
    let ctx = agent.take_inject_context();
    assert_eq!(ctx.as_deref(), Some("tz=Asia/Shanghai"));
}

#[tokio::test]
async fn transform_tool_result_replaces_before_post_tool_call() {
    let dir = TempDir::new().unwrap();
    let mut agent = AgentLoop::new(test_config(&dir)).unwrap();
    let bus = agent.hook_bus();
    bus.register(hooks::TRANSFORM_TOOL_RESULT, |_| {
        hooks::HookOutcome::ReplaceText("REDACTED".into())
    });
    let captured: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
    let captured2 = Arc::clone(&captured);
    bus.register(hooks::POST_TOOL_CALL, move |payload| {
        *captured2.lock().unwrap() = payload.tool_result.clone();
        hooks::HookOutcome::Continue
    });

    let out = agent
        .handle_tool_call_async(
            "file_ops",
            &serde_json::json!({"path": "x.txt", "operation": "write", "content": "hello"}),
        )
        .await
        .unwrap();

    assert_eq!(out.text(), "REDACTED");
    assert_eq!(
        captured.lock().unwrap().as_deref(),
        Some("REDACTED"),
        "post_tool_call must observe the transformed result"
    );
}

#[tokio::test]
async fn turn_wrote_disk_tracks_writes_and_resets_on_new_turn() {
    let dir = TempDir::new().unwrap();
    let mut agent = AgentLoop::new(test_config(&dir)).unwrap();
    assert!(!agent.turn_wrote_disk());

    // 只读操作不应置位
    let _ = agent
        .handle_tool_call_async(
            "file_ops",
            &serde_json::json!({"path": ".", "operation": "list"}),
        )
        .await
        .unwrap();
    assert!(!agent.turn_wrote_disk());

    // 写操作应置位
    let _ = agent
        .handle_tool_call_async(
            "file_ops",
            &serde_json::json!({"path": "a.txt", "operation": "write", "content": "hi"}),
        )
        .await
        .unwrap();
    assert!(agent.turn_wrote_disk());

    // 新用户轮次开始应清零
    agent.begin_user_turn();
    assert!(!agent.turn_wrote_disk());

    // terminal 工具调用也应置位
    let _ = agent
        .handle_tool_call_async("terminal", &serde_json::json!({"command": "true"}))
        .await
        .unwrap();
    assert!(agent.turn_wrote_disk());
}

#[tokio::test]
async fn test_agent_loop_memory_injection() {
    let dir = TempDir::new().unwrap();
    let mut agent = AgentLoop::new(test_config(&dir)).unwrap();

    let wrote = agent
        .handle_tool_call_async(
            "memory",
            &serde_json::json!({
                "action": "add",
                "target": "user",
                "content": "用户偏好 Rust 和深色主题"
            }),
        )
        .await
        .unwrap();
    assert!(wrote.text().contains("已写盘（live）") || wrote.text().contains("已存在"));

    let mut agent = AgentLoop::new(test_config(&dir)).unwrap();
    let result = agent.run_turn("你好", "task-1").await.unwrap();
    match result {
        agent::TurnResult::Continue { system_prompt, .. } => {
            assert!(system_prompt.contains("用户偏好 Rust"));
            assert!(system_prompt.contains("Astro") || system_prompt.contains("身份"));
        }
        other => panic!("expected Continue, got {other:?}"),
    }
}
