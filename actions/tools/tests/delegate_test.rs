//! 异步委派参数与 registry 冒烟测试。

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use tools::{ToolContext, ToolRegistry};

/// 测试用 mock：仅实现 `spawn_async`，其余 panic。
struct MockExecution<F: Fn(String, delegate::DelegateRunRequest) + Send + Sync>(F);

impl<F: Fn(String, delegate::DelegateRunRequest) + Send + Sync> tools::ExecutionDispatch
    for MockExecution<F>
{
    fn run_sync(&self, _req: delegate::DelegateRunRequest) -> anyhow::Result<String> {
        unimplemented!("not used in async tests")
    }
    fn spawn_async(&self, task_id: String, req: delegate::DelegateRunRequest) {
        (self.0)(task_id, req);
    }
    fn spawn_orchestration(&self, _req: orchestration::OrchestrationSpawnRequest) {
        unimplemented!("not used in async tests")
    }
}

static ASYNC_TEST_LOCK: Mutex<()> = Mutex::const_new(());

fn make_ctx<'a>(
    memory: &'a mut memory::MemoryManager,
    sessions: &'a session::SessionStore,
    dir: &std::path::Path,
    targets: &'a tools::ImageGenTargets,
    creds: &'a tools::ModelCredentials,
) -> ToolContext<'a> {
    ToolContext {
        memory,
        sessions,
        memory_dir: dir.to_path_buf(),
        workspace_dir: dir.to_path_buf(),
        project_root: None,
        image_gen_targets: targets,
        session_id: "s".into(),
        turn_id: None,
        credentials: creds,
        chat_targets: &[],
        execution: None,
        hook_bus: None,
    }
}

#[tokio::test]
async fn delegate_requires_goal() {
    let _registry = ToolRegistry::new();
    let dir = tempfile::tempdir().unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions =
        session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let targets = tools::ImageGenTargets::default();
    let creds = tools::ModelCredentials {
        provider: "openai".into(),
        model: "test".into(),
        api_key: "k".into(),
        base_url: String::new(),
    };
    let mut ctx = make_ctx(&mut memory, &sessions, dir.path(), &targets, &creds);
    let args = serde_json::json!({});
    let err = tools::dispatch_tool(|_| true, &mut ctx, "subagent", &args, None)
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("goal") || msg.contains("runner"),
        "unexpected: {msg}"
    );
}

#[tokio::test]
async fn delegate_goal_hits_runner_or_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions =
        session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let targets = tools::ImageGenTargets::default();
    let creds = tools::ModelCredentials {
        provider: "openai".into(),
        model: "test".into(),
        api_key: "k".into(),
        base_url: String::new(),
    };
    let mut ctx = make_ctx(&mut memory, &sessions, dir.path(), &targets, &creds);
    let args = serde_json::json!({"goal": "do thing"});
    let err = tools::dispatch_tool(|_| true, &mut ctx, "subagent", &args, None)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("runner") || err.to_string().contains("API Key"),
        "got: {err}"
    );
}

#[tokio::test]
async fn delegate_async_status_collect_cancel_flow() {
    let _guard = ASYNC_TEST_LOCK.lock().await;
    let exec: Arc<dyn tools::ExecutionDispatch> =
        Arc::new(MockExecution(|task_id: String, _req| {
            let tid = task_id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                delegate::AsyncDelegateRegistry::global().finish_ok(
                    &tid,
                    serde_json::json!({"ok": true, "summary": "done"}).to_string(),
                );
            });
        }));

    let dir = tempfile::tempdir().unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions =
        session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let targets = tools::ImageGenTargets::default();
    let creds = tools::ModelCredentials {
        provider: "openai".into(),
        model: "test".into(),
        api_key: "k".into(),
        base_url: String::new(),
    };
    let mut ctx = make_ctx(&mut memory, &sessions, dir.path(), &targets, &creds);
    ctx.execution = Some(exec);

    let started = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "subagent",
        &serde_json::json!({"action": "async", "goal": "async job"}),
        None,
    )
    .await
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(started.text()).unwrap();
    let task_id = v["task_id"].as_str().unwrap().to_string();
    assert_eq!(v["status"], "running");

    let status = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "subagent",
        &serde_json::json!({"action": "status", "task_id": task_id}),
        None,
    )
    .await
    .unwrap();
    let st: serde_json::Value = serde_json::from_str(status.text()).unwrap();
    assert!(
        st["status"] == "running" || st["status"] == "done",
        "status={st}"
    );

    let collected = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "subagent",
        &serde_json::json!({"action": "collect", "task_id": task_id, "timeout_secs": 5}),
        None,
    )
    .await
    .unwrap();
    let done: serde_json::Value = serde_json::from_str(collected.text()).unwrap();
    assert_eq!(done["status"], "done", "body={done}");
    assert_eq!(done["result"]["ok"], true, "body={done}");
}

#[tokio::test]
async fn delegate_async_cancel_marks_cancelled() {
    let _guard = ASYNC_TEST_LOCK.lock().await;
    let exec: Arc<dyn tools::ExecutionDispatch> =
        Arc::new(MockExecution(|task_id: String, _req| {
            let tid = task_id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(500)).await;
                delegate::AsyncDelegateRegistry::global()
                    .finish_ok(&tid, r#"{"late":true}"#.into());
            });
        }));

    let dir = tempfile::tempdir().unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions =
        session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let targets = tools::ImageGenTargets::default();
    let creds = tools::ModelCredentials {
        provider: "openai".into(),
        model: "test".into(),
        api_key: "k".into(),
        base_url: String::new(),
    };
    let mut ctx = make_ctx(&mut memory, &sessions, dir.path(), &targets, &creds);
    ctx.execution = Some(exec);

    let started = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "subagent",
        &serde_json::json!({"action": "async", "goal": "cancel me"}),
        None,
    )
    .await
    .unwrap();
    let task_id = serde_json::from_str::<serde_json::Value>(started.text()).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let cancelled = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "subagent",
        &serde_json::json!({"action": "cancel", "task_id": task_id}),
        None,
    )
    .await
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(cancelled.text()).unwrap();
    assert_eq!(v["status"], "cancelled");
}

#[tokio::test]
async fn delegate_blocked_at_max_spawn_depth() {
    let _guard = ASYNC_TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let targets = tools::ImageGenTargets::default();
    let sessions =
        session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let creds = tools::ModelCredentials {
        provider: "openai".into(),
        model: "test".into(),
        api_key: "k".into(),
        base_url: String::new(),
    };
    let mut ctx = make_ctx(&mut memory, &sessions, dir.path(), &targets, &creds);

    let ctx_depth = home::SpawnDepthCtx {
        depth: 1,
        max_depth: 1,
    };
    let err = home::scope_spawn_depth(ctx_depth, async {
        tools::dispatch_tool(
            |_| true,
            &mut ctx,
            "subagent",
            &serde_json::json!({"goal": "nope"}),
            None,
        )
        .await
    })
    .await
    .unwrap_err();
    assert!(err.to_string().contains("spawn depth"), "got: {err}");
}
