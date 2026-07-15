//! `AgentBuilder` 构建与会话集成测试。

use std::sync::Arc;

use agent::builder::AgentBuilder;
use agent::context::{DynamicContext, StaticContext};
use agent::loop_::{AgentConfig, AgentLoop, MaxDepthError};
use agent::prompt_builder::PromptBuilder;
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
    use agent::loop_::validate_message_order;
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
            "preamble",
            "mem",
            "user",
            "",
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
        out.contains("blocked by hook") && out.contains("denied-by-test"),
        "out={out}"
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
    assert!(wrote.contains("已写盘（live）") || wrote.contains("已存在"));

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
