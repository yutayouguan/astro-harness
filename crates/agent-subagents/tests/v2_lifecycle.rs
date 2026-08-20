use std::time::Duration;

use agent::exec::dispatch::test_support::{LifecycleTestApp, ScriptedTurn};
use subagents::{AgentStatusKind, AgentStatusV2};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_agent_tree_survives_interrupt_restart_resume_and_recursive_close() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = LifecycleTestApp::new(
        temp.path().join("memory"),
        "acceptance-root",
        vec![
            ScriptedTurn::Complete("research complete".into()),
            ScriptedTurn::Complete("citations complete".into()),
            ScriptedTurn::Pending,
        ],
    )
    .unwrap();

    let research = app
        .spawn("/root", "research", "collect sources")
        .await
        .unwrap();
    app.wait_for_status("/root/research", AgentStatusKind::Completed)
        .await
        .unwrap();
    let citations = app
        .spawn("/root/research", "citations", "check citations")
        .await
        .unwrap();
    app.wait_for_status("/root/research/citations", AgentStatusKind::Completed)
        .await
        .unwrap();

    let queued = app
        .send_message("/root/research/citations", "queued context")
        .await
        .unwrap();
    assert!(queued.queued);
    assert!(!queued.turn_triggered);
    assert!(!app.is_running(&citations.thread_id));
    assert_eq!(app.pending_mailbox("/root/research/citations").unwrap(), 1);

    let followup = app
        .followup("/root/research/citations", "continue")
        .await
        .unwrap();
    assert!(followup.turn_triggered);
    app.wait_for_status("/root/research/citations", AgentStatusKind::Running)
        .await
        .unwrap();
    let interrupted = app.interrupt("/root/research/citations").await.unwrap();
    assert_eq!(interrupted.previous_status.kind(), AgentStatusKind::Running);
    assert_eq!(
        interrupted.thread.status.kind(),
        AgentStatusKind::Interrupted
    );
    assert_eq!(app.pending_mailbox("/root/research/citations").unwrap(), 0);
    let before_restart = app.session_contents("/root/research/citations").unwrap();
    assert!(before_restart
        .iter()
        .any(|text| text.contains("queued context")));
    assert!(before_restart.iter().any(|text| text.contains("continue")));
    let pre_restart_calls = app.provider_calls();
    let interrupted_call = pre_restart_calls.last().unwrap();
    let interrupted_prompt = interrupted_call
        .messages
        .iter()
        .filter(|(role, _)| role != "system")
        .map(|(_, content)| content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        interrupted_prompt.matches("queued context").count(),
        1,
        "{:?}",
        interrupted_call.messages
    );
    assert_eq!(
        interrupted_prompt.matches("continue").count(),
        1,
        "{:?}",
        interrupted_call.messages
    );
    assert!(!interrupted_call.tool_names.is_empty());
    assert!(interrupted_call
        .messages
        .windows(2)
        .all(|pair| pair[0].0 != pair[1].0));

    let old_process_hooks = app
        .restart_with(vec![ScriptedTurn::Complete("resumed after restart".into())])
        .await
        .unwrap();
    assert_eq!(
        app.status("/root/research/citations").unwrap().kind(),
        AgentStatusKind::Interrupted
    );
    assert!(!app.is_running(&citations.thread_id));
    assert!(!app.has_runtime_handle(&citations.thread_id).unwrap());
    assert_eq!(
        app.session_contents("/root/research/citations").unwrap(),
        before_restart
    );

    app.followup("/root/research/citations", "resume after restart")
        .await
        .unwrap();
    app.wait_for_status("/root/research/citations", AgentStatusKind::Completed)
        .await
        .unwrap();
    let history = app.session_contents("/root/research/citations").unwrap();
    assert!(history
        .iter()
        .any(|text| text.contains("resume after restart")));
    assert!(history
        .iter()
        .any(|text| text.contains("resumed after restart")));
    let recovered_calls = app.provider_calls();
    let recovered_call = recovered_calls.last().unwrap();
    let recovered_prompt = recovered_call
        .messages
        .iter()
        .filter(|(role, _)| role != "system")
        .map(|(_, content)| content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for marker in ["queued context", "continue", "resume after restart"] {
        assert_eq!(recovered_prompt.matches(marker).count(), 1, "{marker}");
    }
    assert!(recovered_call
        .messages
        .windows(2)
        .all(|pair| pair[0].0 != pair[1].0));

    let first = app.close_subtree("/root/research").await.unwrap();
    let second = app.close_subtree("/root/research").await.unwrap();
    assert_eq!(first.threads, second.threads);
    for path in ["/root/research", "/root/research/citations"] {
        assert_eq!(app.status(path).unwrap(), AgentStatusV2::Shutdown);
    }

    // The root is not charged against the descendant identity quota.
    assert_eq!(app.identity_count().unwrap(), 2);
    assert_eq!(app.active_execution_count().unwrap(), 0);
    assert_eq!(app.active_runtime_count(), 0);
    assert_eq!(app.runtime_request_count(), 0);
    assert!(!app.has_runtime_handle(&research.thread_id).unwrap());
    assert!(!app.has_runtime_handle(&citations.thread_id).unwrap());
    let session_ids = app.session_ids().unwrap();
    assert_eq!(session_ids.len(), 3, "session ids: {session_ids:?}");
    assert_eq!(
        old_process_hooks.lock().unwrap().clone(),
        vec![
            "start:/root/research".to_string(),
            "stop:/root/research".to_string(),
            "start:/root/research/citations".to_string(),
            "stop:/root/research/citations".to_string(),
            "stop:/root/research/citations".to_string(),
        ]
    );
    // A new process observes the terminal turn only for runtime material it
    // recovered in that process. Desktop close does not duplicate SubagentStop.
    assert_eq!(
        app.hook_events(),
        vec!["stop:/root/research/citations".to_string()]
    );

    tokio::time::timeout(Duration::from_millis(100), async {
        while app.active_runtime_count() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
