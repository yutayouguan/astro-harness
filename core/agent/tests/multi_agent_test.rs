//! 多代理编排（`Orchestrator`）并行分发测试。

use agent::exec::multi_agent::*;

#[tokio::test]
async fn test_orchestrator_dispatches_sub_agents() {
    let orchestrator = Orchestrator::new(OrchestratorConfig {
        max_sub_agents: 5,
        sub_agent_max_turns: 50,
    });
    let tasks = vec![
        SubTask {
            id: "task_a".to_string(),
            description: "搜索 Rust 异步编程资料".to_string(),
        },
        SubTask {
            id: "task_b".to_string(),
            description: "总结 tokio 文档".to_string(),
        },
    ];
    let results = orchestrator.dispatch_parallel_stub(tasks).await;
    assert_eq!(results.len(), 2);
}

#[test]
fn test_sub_agent_config_defaults() {
    let config = SubAgentConfig::default();
    assert_eq!(config.max_turns, 50);
}
