//! 同会话 MEMORY/USER snapshot 冻结：工具写入不进 system prompt，直至 refresh。

use agent::runtime::{AgentConfig, AgentLoop};
use tempfile::TempDir;

#[tokio::test]
async fn snapshot_frozen_within_session() {
    let dir = TempDir::new().unwrap();
    let mut agent = AgentLoop::with_session_id(
        AgentConfig::with_defaults(dir.path().to_path_buf()),
        "freeze-session".into(),
    )
    .unwrap();

    let marker = "唯一冻结条目-aurora-xyz-991";
    let wrote = agent
        .handle_tool_call_async(
            "memory",
            &serde_json::json!({
                "action": "add",
                "target": "memory",
                "content": marker
            }),
        )
        .await
        .unwrap();
    assert!(
        wrote.text().contains("已写盘（live）") || wrote.text().contains("已写入"),
        "unexpected tool result: {:?}",
        wrote
    );

    let frozen = agent.build_system_prompt().await;
    assert!(
        !frozen.contains(marker),
        "tool write must not enter snapshot prompt until refresh; got:\n{frozen}"
    );

    agent.refresh_memory().unwrap();
    let refreshed = agent.build_system_prompt().await;
    assert!(
        refreshed.contains(marker),
        "after refresh_memory, snapshot prompt should contain new entry; got:\n{refreshed}"
    );
}
