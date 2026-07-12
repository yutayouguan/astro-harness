//! Agent 主循环（`AgentLoop`）回合与工具调用测试。

use agent::loop_::*;
use common::message::*;
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
    use agent::prompt_builder::PromptBuilder;
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
    let mut agent = AgentLoop::new(test_config(&dir)).unwrap();

    agent
        .handle_tool_call_async(
            "memory_add",
            &serde_json::json!({
                "entry": "用户偏好 Rust 和深色主题",
                "target": "user"
            }),
        )
        .await
        .unwrap();

    let result = agent.run_turn("你好", "task-1").await.unwrap();
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
    let mut agent = AgentLoop::new(config).unwrap();

    agent.run_turn("消息一", "task-1").await.unwrap();
    agent
        .record_assistant_message("回复一")
        .unwrap();
    agent.run_turn("消息二", "task-2").await.unwrap();
    agent
        .record_assistant_message("回复二")
        .unwrap();

    let result = agent
        .run_turn("三个月前我们定了地图点叫 Aurora", "task-3")
        .await
        .unwrap();

    match result {
        TurnResult::Continue { system_prompt, .. } => {
            assert!(system_prompt.contains("Aurora") || agent.recalled_context().contains("Aurora"));
        }
        _ => panic!("expected Continue"),
    }
}

#[tokio::test]
async fn test_memory_tools_registered() {
    let dir = TempDir::new().unwrap();
    let agent = AgentLoop::new(test_config(&dir)).unwrap();
    let names: Vec<_> = agent
        .tool_registry()
        .available_tools()
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert!(names.contains(&"memory_add"));
    assert!(names.contains(&"session_search"));
}
