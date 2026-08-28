//! Agent 主循环（`AgentLoop`）回合与工具调用测试。

use agent::runtime::*;
use tempfile::TempDir;
use types::message::*;

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
async fn test_message_alternation_validation() {
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
async fn test_prompt_builder_layers() {
    use agent::prompt::prompt_builder::PromptBuilder;
    let builder = PromptBuilder::new();
    let prompt = builder
        .with_soul("你是 Astro，一个自我进化的 AI 助手")
        .with_timestamp()
        .build();
    assert!(prompt.contains("Astro"));
    assert!(prompt.contains("时间"));
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

    // 工具写入只改 live；新 AgentLoop（新 session）open/reload 会把盘上内容固化进 snapshot
    let agent = AgentLoop::new(test_config(&dir)).unwrap();
    let result = agent.start_or_steer_turn("你好", "task-1").await.unwrap();
    match result {
        TurnResult::Continue { system_prompt, .. } => {
            assert!(system_prompt.contains("用户偏好 Rust"));
            assert!(system_prompt.contains("Astro"));
        }
        _ => panic!("expected Continue"),
    }
}

#[tokio::test]
async fn test_agent_loop_fts_recall_after_long_session() {
    let dir = TempDir::new().unwrap();
    let mut config = test_config(&dir);
    config.recent_turns = 2;
    let agent = AgentLoop::new(config).unwrap();

    agent.start_or_steer_turn("消息一", "task-1").await.unwrap();
    agent.record_assistant_message("回复一").await.unwrap();
    agent.start_or_steer_turn("消息二", "task-2").await.unwrap();
    agent.record_assistant_message("回复二").await.unwrap();

    let result = agent
        .start_or_steer_turn("三个月前我们定了地图点叫 Aurora", "task-3")
        .await
        .unwrap();

    match result {
        TurnResult::Continue { system_prompt, .. } => {
            assert!(
                system_prompt.contains("Aurora")
                    || agent.recalled_context().await.contains("Aurora")
            );
        }
        _ => panic!("expected Continue"),
    }
}

#[tokio::test]
async fn test_memory_tools_registered() {
    let dir = TempDir::new().unwrap();
    let agent = AgentLoop::new(test_config(&dir)).unwrap();
    let registry = agent.tool_registry().await;
    let names: Vec<_> = registry
        .available_tools()
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert!(names.contains(&"memory"));
    assert!(names.contains(&"context_search"));
    assert!(!names.contains(&"session_search"));
    assert!(!names.contains(&"search"));
    assert!(names.contains(&"pin_context"));
}

#[tokio::test]
async fn test_set_model_agno_style_entry() {
    let dir = TempDir::new().unwrap();
    let mut agent = AgentLoop::new(test_config(&dir)).unwrap();
    agent.set_chat_credentials("openai", "gpt-old", "sk-test", "https://api.openai.com/v1");
    agent.set_chat_targets(vec![types::ChatTarget {
        provider_id: "openai".into(),
        backend_id: "openai".into(),
        model: "gpt-old".into(),
        api_key: "sk-test".into(),
        base_url: "https://api.openai.com/v1".into(),
        api_mode: String::new(),
    }]);

    let spec = types::ModelSpec::parse("claude:claude-sonnet-4-5")
        .unwrap()
        .with_temperature(0.2);
    agent.set_model(spec);

    assert_eq!(agent.chat_provider(), "claude");
    assert_eq!(agent.chat_model(), "claude-sonnet-4-5");
    assert_eq!(agent.temperature(), 0.2);
    let primary = &agent.chat_targets()[0];
    assert_eq!(primary.backend_id, "claude");
    assert_eq!(primary.model, "claude-sonnet-4-5");
    assert_eq!(primary.api_key, "sk-test");
    assert_eq!(
        agent.model_spec().map(|s| s.as_str()),
        Some("claude:claude-sonnet-4-5".into())
    );

    agent.set_role_model(
        types::ModelRole::Auxiliary(types::AuxiliaryTask::Compaction),
        types::ModelSpec::parse("openai:gpt-5.6").unwrap(),
    );
    let aux = agent.auxiliary_targets(types::AuxiliaryTask::Compaction);
    assert_eq!(aux[0].backend_id, "openai");
    assert_eq!(aux[0].model, "gpt-5.6");
}

#[tokio::test]
async fn test_set_fallback_models_keeps_primary() {
    let dir = TempDir::new().unwrap();
    let mut agent = AgentLoop::new(test_config(&dir)).unwrap();
    agent.set_chat_targets(vec![types::ChatTarget {
        provider_id: "claude".into(),
        backend_id: "claude".into(),
        model: "opus".into(),
        api_key: "sk-claude".into(),
        base_url: "https://api.anthropic.com".into(),
        api_mode: String::new(),
    }]);

    agent.set_fallback_models(&[
        types::ModelSpec::parse("openai:gpt-5.6").unwrap(),
        types::ModelSpec::parse("google:gemini-2.5-flash").unwrap(),
        types::ModelSpec::parse("openai:gpt-4o").unwrap(), // same provider → skip
        types::ModelSpec::parse("deepseek:chat").unwrap(), // would be 4th → capped
        types::ModelSpec::parse("zhipu:glm").unwrap(),
    ]);

    let chain = agent.chat_targets();
    assert_eq!(chain.len(), 4); // primary + 3 fallbacks
    assert_eq!(chain[0].backend_id, "claude");
    assert_eq!(chain[0].model, "opus");
    assert_eq!(chain[1].backend_id, "openai");
    assert_eq!(chain[1].model, "gpt-5.6");
    assert_eq!(chain[1].api_key, "sk-claude"); // inherits primary creds
    assert_eq!(chain[2].backend_id, "google");
    assert_eq!(chain[3].backend_id, "deepseek");

    agent.set_role_fallback_models(
        types::ModelRole::Auxiliary(types::AuxiliaryTask::TitleGeneration),
        &[types::ModelSpec::parse("openai:gpt-5.6").unwrap()],
    );
    let aux = agent.auxiliary_targets(types::AuxiliaryTask::TitleGeneration);
    assert_eq!(aux.len(), 2);
    assert_eq!(aux[0].backend_id, "claude"); // preferred = primary
    assert_eq!(aux[1].backend_id, "openai");
}

#[tokio::test]
async fn run_turn_clears_prior_cancel_signal() {
    let dir = TempDir::new().unwrap();
    let agent = AgentLoop::new(test_config(&dir)).unwrap();
    agent.cancel_signal().cancel();
    let result = agent
        .start_or_steer_turn("重试", "task-retry")
        .await
        .unwrap();
    assert!(matches!(result, TurnResult::Continue { .. }));
    assert!(!agent.cancel_signal().is_cancelled());
}

#[tokio::test]
async fn system_prompt_includes_interaction_mode_guidance() {
    let dir = TempDir::new().unwrap();
    let agent = AgentLoop::new(test_config(&dir)).unwrap();

    agent
        .set_interaction_mode(tools::InteractionMode::Plan)
        .await;
    let plan_prompt = agent.build_system_prompt().await;
    assert!(
        plan_prompt.contains("Interaction mode: Plan") && plan_prompt.contains("交互模式：Plan"),
        "Plan guidance missing from system prompt:\n{plan_prompt}"
    );
    // 固定工具规则属于稳定基础指令；动态 mode 保持 developer 角色。
    let mode_pos = plan_prompt.find("Interaction mode: Plan").expect("mode");
    let tool_pos = plan_prompt.find("# 工具使用").expect("tool guidance");
    assert!(
        tool_pos < mode_pos,
        "TOOL_GUIDANCE should stay in stable base instructions"
    );

    agent
        .set_interaction_mode(tools::InteractionMode::Ask)
        .await;
    let ask_prompt = agent.build_system_prompt().await;
    assert!(
        ask_prompt.contains("Interaction mode: Ask") && ask_prompt.contains("交互模式：Ask"),
        "Ask guidance missing from system prompt:\n{ask_prompt}"
    );

    // 估算层与真实组装共用同源 guidance+timestamp
    let (system_chars, _, _, _) = agent.system_prompt_layer_chars().await;
    assert!(system_chars > 0);
}

#[tokio::test]
async fn prompt_contract_separates_base_developer_and_user_context() {
    let dir = TempDir::new().unwrap();
    let mut config = test_config(&dir);
    config.static_override = Some(agent::prompt::context::StaticContext {
        soul: "STABLE_SOUL".into(),
        identity: "STABLE_IDENTITY".into(),
        agent_md: "PROJECT_INSTRUCTIONS".into(),
        tools_md: String::new(),
        memory: "MEMORY_CONTEXT".into(),
        user_profile: "USER_CONTEXT".into(),
        daily: "DAILY_CONTEXT".into(),
    });
    let agent = AgentLoop::new(config).unwrap();
    std::fs::write(
        agent.workspace_dir().join("TOOLS.md"),
        "LOCAL_TOOL_INSTRUCTIONS",
    )
    .unwrap();

    let prompt = agent.build_prompt_contract().await;

    assert!(prompt.base_instructions.contains("STABLE_SOUL"));
    assert!(prompt.base_instructions.contains("STABLE_IDENTITY"));
    assert!(!prompt.base_instructions.contains("PROJECT_INSTRUCTIONS"));
    assert!(!prompt.base_instructions.contains("MEMORY_CONTEXT"));
    assert_eq!(prompt.context.len(), 2);
    assert_eq!(
        prompt.context[0].role(),
        providers::types::message::Role::Developer
    );
    assert!(prompt.base_instructions.contains("# 工具使用"));
    assert!(!prompt.context[0].text_content().contains("# 工具使用"));
    assert_eq!(
        prompt.context[1].role(),
        providers::types::message::Role::User
    );
    assert!(prompt.context[1]
        .text_content()
        .contains("LOCAL_TOOL_INSTRUCTIONS"));
    assert!(prompt.context[1].text_content().contains("MEMORY_CONTEXT"));
}
