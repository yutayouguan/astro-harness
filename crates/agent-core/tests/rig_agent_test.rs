//! `AgentBuilder` 构建与会话集成测试。

use std::sync::Arc;

use agent::builder::AgentBuilder;
use agent::prompt::context::{DynamicContext, StaticContext};
use agent::prompt::prompt_builder::PromptBuilder;
use agent::runtime::{AgentConfig, AgentLoop, MaxDepthError};
use home::AgentRuntimeConfig;
use tempfile::TempDir;
use types::message::Message;

#[test]
fn session_fire_hook_uses_shared_runtime_and_common_payload() {
    let dir = TempDir::new().unwrap();
    let (agent, _) = AgentBuilder::new(dir.path()).build().unwrap();
    let runtime = Arc::new(hooks::HookRuntime::new());
    let captured = Arc::new(std::sync::Mutex::new(None));
    let capture = Arc::clone(&captured);
    runtime
        .plugin
        .register(hooks::USER_PROMPT_SUBMIT, move |input| {
            *capture.lock().unwrap() = Some(input.clone());
            hooks::HookOutcome::Continue
        });

    let project_root = dir.path().join("project");
    agent.set_hook_runtime(Arc::clone(&runtime));
    agent.set_project_root(Some(project_root.clone()));
    agent.set_permission_profile(Some("workspace-write".into()));
    agent.set_chat_credentials("openai", "gpt-5.6-sol", "test-key", "");
    agent.fire_hook(
        hooks::USER_PROMPT_SUBMIT,
        hooks::HookInput {
            prompt: Some("hello".into()),
            ..Default::default()
        },
    );

    let payload = captured.lock().unwrap().clone().expect("hook payload");
    assert_eq!(payload.session_id, agent.session_id());
    assert_eq!(payload.cwd, project_root.to_string_lossy());
    assert_eq!(payload.model, "openai/gpt-5.6-sol");
    assert_eq!(payload.permission_mode.as_deref(), Some("workspace-write"));
    assert_eq!(payload.hook_event_name, hooks::USER_PROMPT_SUBMIT);
    assert_eq!(payload.prompt.as_deref(), Some("hello"));
    assert!(Arc::ptr_eq(&agent.hook_runtime(), &runtime));
}

#[test]
fn session_hook_payload_preserves_explicit_values_and_falls_back_to_defaults() {
    let dir = TempDir::new().unwrap();
    let (agent, _) = AgentBuilder::new(dir.path()).build().unwrap();
    let runtime = Arc::new(hooks::HookRuntime::new());
    let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
    let capture = Arc::clone(&captured);
    runtime
        .plugin
        .register(hooks::USER_PROMPT_SUBMIT, move |input| {
            capture.lock().unwrap().push(input.clone());
            hooks::HookOutcome::Continue
        });
    agent.set_hook_runtime(runtime);
    agent.set_project_root(None);
    agent.set_permission_profile(Some("workspace-write".into()));
    agent.set_chat_targets(vec![types::ChatTarget {
        provider_id: String::new(),
        backend_id: String::new(),
        model: "model-only".into(),
        api_key: String::new(),
        base_url: String::new(),
    }]);

    agent.fire_hook(hooks::USER_PROMPT_SUBMIT, hooks::HookInput::default());
    agent.set_chat_targets(vec![types::ChatTarget {
        provider_id: String::new(),
        backend_id: "backend-only".into(),
        model: String::new(),
        api_key: String::new(),
        base_url: String::new(),
    }]);
    agent.fire_hook(hooks::USER_PROMPT_SUBMIT, hooks::HookInput::default());
    agent.set_chat_targets(vec![types::ChatTarget {
        provider_id: String::new(),
        backend_id: String::new(),
        model: String::new(),
        api_key: String::new(),
        base_url: String::new(),
    }]);
    agent.fire_hook(hooks::USER_PROMPT_SUBMIT, hooks::HookInput::default());
    agent.fire_hook(
        hooks::USER_PROMPT_SUBMIT,
        hooks::HookInput {
            session_id: "provided-session".into(),
            cwd: "/provided/cwd".into(),
            model: "provided/model".into(),
            permission_mode: Some("read-only".into()),
            ..Default::default()
        },
    );

    let payloads = captured.lock().unwrap();
    assert_eq!(payloads[0].session_id, agent.session_id());
    assert_eq!(payloads[0].cwd, dir.path().to_string_lossy());
    assert_eq!(payloads[0].model, "model-only");
    assert_eq!(
        payloads[0].permission_mode.as_deref(),
        Some("workspace-write")
    );
    assert_eq!(payloads[1].model, "backend-only");
    assert_eq!(payloads[2].model, "");
    assert_eq!(payloads[3].session_id, "provided-session");
    assert_eq!(payloads[3].cwd, "/provided/cwd");
    assert_eq!(payloads[3].model, "provided/model");
    assert_eq!(payloads[3].permission_mode.as_deref(), Some("read-only"));
}

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
    let agent = AgentLoop::new(config).unwrap();
    assert!(!agent.is_budget_exhausted().await);
    agent.increment_turn().await;
    agent.increment_turn().await;
    assert!(agent.is_budget_exhausted().await);
}

#[tokio::test]
async fn test_multi_turn_max_depth() {
    let dir = TempDir::new().unwrap();
    let mut config = test_config(&dir);
    config.multi_turn = 2;
    let agent = AgentLoop::new(config).unwrap();
    agent.begin_user_turn().await;
    agent.increment_tool_round().await.unwrap();
    agent.increment_tool_round().await.unwrap();
    let err = agent.increment_tool_round().await.unwrap_err();
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
async fn session_start_fires_once_before_each_user_prompt() {
    let dir = TempDir::new().unwrap();
    let (agent, _) = AgentBuilder::new(dir.path()).build().unwrap();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let session_events = Arc::clone(&events);
    agent
        .hook_bus()
        .register(hooks::SESSION_START, move |input| {
            session_events.lock().unwrap().push(format!(
                "{}:{}",
                input.hook_event_name,
                input.source.as_deref().unwrap_or("")
            ));
            hooks::HookOutcome::Continue
        });
    let prompt_events = Arc::clone(&events);
    agent
        .hook_bus()
        .register(hooks::USER_PROMPT_SUBMIT, move |input| {
            prompt_events.lock().unwrap().push(format!(
                "{}:{}",
                input.hook_event_name,
                input.prompt.as_deref().unwrap_or("")
            ));
            hooks::HookOutcome::Continue
        });

    agent.start_or_steer_turn("first", "t1").await.unwrap();
    agent
        .record_assistant_message("first answer")
        .await
        .unwrap();
    agent.start_or_steer_turn("second", "t2").await.unwrap();

    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "SessionStart:startup",
            "UserPromptSubmit:first",
            "UserPromptSubmit:second"
        ]
    );
}

#[tokio::test]
async fn user_prompt_submit_block_prevents_persistence() {
    let dir = TempDir::new().unwrap();
    let (agent, _) = AgentBuilder::new(dir.path()).build().unwrap();
    agent.hook_bus().register(hooks::USER_PROMPT_SUBMIT, |_| {
        hooks::HookOutcome::Block("policy".into())
    });

    let error = agent
        .start_or_steer_turn("blocked", "t1")
        .await
        .unwrap_err();

    assert!(error.to_string().contains("policy"));
    assert!(agent.clone_history().await.is_empty());
}

#[tokio::test]
async fn user_prompt_submit_context_enters_initial_system_prompt() {
    let dir = TempDir::new().unwrap();
    let (agent, _) = AgentBuilder::new(dir.path()).build().unwrap();
    agent.hook_bus().register(hooks::USER_PROMPT_SUBMIT, |_| {
        hooks::HookOutcome::InjectContext("PROMPT_HOOK_CONTEXT".into())
    });

    let result = agent.start_or_steer_turn("hello", "t1").await.unwrap();

    let agent::TurnResult::Continue { system_prompt, .. } = result else {
        panic!("expected Continue");
    };
    assert!(system_prompt.contains("PROMPT_HOOK_CONTEXT"));
}

#[tokio::test]
async fn session_start_block_prevents_prompt_and_persistence() {
    let dir = TempDir::new().unwrap();
    let (agent, _) = AgentBuilder::new(dir.path()).build().unwrap();
    let prompt_hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    agent.hook_bus().register(hooks::SESSION_START, |_| {
        hooks::HookOutcome::Block("session policy".into())
    });
    let hits = Arc::clone(&prompt_hits);
    agent
        .hook_bus()
        .register(hooks::USER_PROMPT_SUBMIT, move |_| {
            hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            hooks::HookOutcome::Continue
        });

    let error = agent
        .start_or_steer_turn("blocked", "t1")
        .await
        .unwrap_err();

    assert!(error.to_string().contains("session policy"));
    assert_eq!(prompt_hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(agent.clone_history().await.is_empty());
}

#[tokio::test]
async fn session_and_prompt_contexts_enter_initial_system_prompt_in_order() {
    let dir = TempDir::new().unwrap();
    let (agent, _) = AgentBuilder::new(dir.path()).build().unwrap();
    agent.hook_bus().register(hooks::SESSION_START, |_| {
        hooks::HookOutcome::InjectContext("SESSION_HOOK_CONTEXT".into())
    });
    agent.hook_bus().register(hooks::USER_PROMPT_SUBMIT, |_| {
        hooks::HookOutcome::InjectContext("PROMPT_HOOK_CONTEXT".into())
    });

    let result = agent.start_or_steer_turn("hello", "t1").await.unwrap();

    let agent::TurnResult::Continue { system_prompt, .. } = result else {
        panic!("expected Continue");
    };
    let session_position = system_prompt.find("SESSION_HOOK_CONTEXT").unwrap();
    let prompt_position = system_prompt.find("PROMPT_HOOK_CONTEXT").unwrap();
    assert!(session_position < prompt_position, "{system_prompt}");
    assert!(system_prompt.contains("SESSION_HOOK_CONTEXT\n\nPROMPT_HOOK_CONTEXT"));
}

#[tokio::test]
async fn prompt_skip_is_not_treated_as_block() {
    let dir = TempDir::new().unwrap();
    let (agent, _) = AgentBuilder::new(dir.path()).build().unwrap();
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hook_hits = Arc::clone(&hits);
    agent
        .hook_bus()
        .register(hooks::USER_PROMPT_SUBMIT, move |_| {
            hook_hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            hooks::HookOutcome::Skip("gateway-only".into())
        });

    let result = agent.start_or_steer_turn("admitted", "t1").await.unwrap();

    assert!(matches!(result, agent::TurnResult::Continue { .. }));
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(agent.clone_history().await[0].content_str(), "admitted");
}

#[tokio::test]
async fn pre_tool_call_block_via_hook_bus() {
    let dir = TempDir::new().unwrap();
    let agent = AgentLoop::new(test_config(&dir)).unwrap();
    let bus = agent.hook_bus();
    let captured: Arc<std::sync::Mutex<Option<(String, serde_json::Value)>>> =
        Arc::new(std::sync::Mutex::new(None));
    let captured2 = Arc::clone(&captured);
    bus.register(hooks::PRE_TOOL_USE, move |payload| {
        *captured2.lock().unwrap() = Some((
            payload.hook_event_name.clone(),
            payload.tool_input.clone().expect("canonical tool_input"),
        ));
        hooks::HookOutcome::Block("denied-by-test".into())
    });
    let args = serde_json::json!({"text": "hi"});
    let out = agent.handle_tool_call_async("echo", &args).await.unwrap();
    assert!(
        out.text().contains("blocked by hook") && out.text().contains("denied-by-test"),
        "out={:?}",
        out
    );
    assert_eq!(
        captured.lock().unwrap().as_ref(),
        Some(&(hooks::PRE_TOOL_USE.to_string(), args))
    );
}

#[tokio::test]
async fn pre_llm_call_inject_context_via_hook_bus() {
    let dir = TempDir::new().unwrap();
    let agent = AgentLoop::new(test_config(&dir)).unwrap();
    let bus = agent.hook_bus();
    bus.register(hooks::PRE_LLM_CALL, |_| {
        hooks::HookOutcome::InjectContext("tz=Asia/Shanghai".into())
    });
    let _ = agent
        .start_or_steer_turn("你好", "inject-test")
        .await
        .unwrap();
    let ctx = agent.take_inject_context().await;
    assert_eq!(ctx.as_deref(), Some("tz=Asia/Shanghai"));
}

#[tokio::test]
async fn transform_tool_result_replaces_before_post_tool_call() {
    let dir = TempDir::new().unwrap();
    let agent = AgentLoop::new(test_config(&dir)).unwrap();
    let bus = agent.hook_bus();
    bus.register(hooks::TRANSFORM_TOOL_RESULT, |_| {
        hooks::HookOutcome::ReplaceText("REDACTED".into())
    });
    let captured: Arc<std::sync::Mutex<Option<(serde_json::Value, String)>>> =
        Arc::new(std::sync::Mutex::new(None));
    let captured2 = Arc::clone(&captured);
    bus.register(hooks::POST_TOOL_USE, move |payload| {
        let tool_input = payload.tool_input.clone().expect("canonical tool_input");
        let tool_response = payload
            .tool_response
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .expect("string tool_response")
            .to_string();
        *captured2.lock().unwrap() = Some((tool_input, tool_response));
        hooks::HookOutcome::Continue
    });

    let args = serde_json::json!({"path": "x.txt", "operation": "write", "content": "hello"});
    let out = agent
        .handle_tool_call_async("file_ops", &args)
        .await
        .unwrap();

    assert_eq!(out.text(), "REDACTED");
    assert_eq!(
        captured.lock().unwrap().as_ref(),
        Some(&(args, "REDACTED".to_string())),
        "PostToolUse must observe canonical input and transformed response"
    );
}

#[tokio::test]
async fn turn_wrote_disk_tracks_writes_and_resets_on_new_turn() {
    let dir = TempDir::new().unwrap();
    let agent = AgentLoop::new(test_config(&dir)).unwrap();
    assert!(!agent.turn_wrote_disk().await);

    // 只读操作不应置位
    let _ = agent
        .handle_tool_call_async(
            "file_ops",
            &serde_json::json!({"path": ".", "operation": "list"}),
        )
        .await
        .unwrap();
    assert!(!agent.turn_wrote_disk().await);

    // 写操作应置位
    let _ = agent
        .handle_tool_call_async(
            "file_ops",
            &serde_json::json!({"path": "a.txt", "operation": "write", "content": "hi"}),
        )
        .await
        .unwrap();
    assert!(agent.turn_wrote_disk().await);

    // 新用户轮次开始应清零
    agent.begin_user_turn().await;
    assert!(!agent.turn_wrote_disk().await);

    // terminal 工具调用也应置位
    let _ = agent
        .handle_tool_call_async("terminal", &serde_json::json!({"command": "true"}))
        .await
        .unwrap();
    assert!(agent.turn_wrote_disk().await);
}

#[tokio::test]
async fn test_agent_loop_memory_injection() {
    let dir = TempDir::new().unwrap();
    let agent = AgentLoop::new(test_config(&dir)).unwrap();

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

    let agent = AgentLoop::new(test_config(&dir)).unwrap();
    let result = agent.start_or_steer_turn("你好", "task-1").await.unwrap();
    match result {
        agent::TurnResult::Continue { system_prompt, .. } => {
            assert!(system_prompt.contains("用户偏好 Rust"));
            assert!(system_prompt.contains("Astro") || system_prompt.contains("身份"));
        }
        other => panic!("expected Continue, got {other:?}"),
    }
}
