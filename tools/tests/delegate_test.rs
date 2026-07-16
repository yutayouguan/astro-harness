//! 异步委派参数与 registry 冒烟测试。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tools::{ToolContext, ToolRegistry};

static ASYNC_TEST_LOCK: Mutex<()> = Mutex::new(());

fn make_ctx<'a>(
    memory: &'a mut memory::MemoryManager,
    sessions: &'a session::SessionStore,
    dir: &std::path::Path,
    providers: &'a providers::registry::ProviderRegistry,
    targets: &'a tools::ImageGenTargets,
) -> ToolContext<'a> {
    ToolContext {
        memory,
        sessions,
        memory_dir: dir.to_path_buf(),
        workspace_dir: dir.to_path_buf(),
        project_root: None,
        image_gen_targets: targets,
        providers,
        session_id: "s".into(),
        turn_id: None,
        chat_api_key: "k".into(),
        chat_base_url: String::new(),
        chat_provider: "openai".into(),
        chat_model: "test".into(),
        chat_targets: vec![],
        delegate_runner: None,
        async_spawner: None,
        orchestration_spawner: None,
        hook_bus: None,
    }
}

#[tokio::test]
async fn delegate_requires_goal() {
    let _registry = ToolRegistry::new();
    let dir = tempfile::tempdir().unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let providers = providers::registry::ProviderRegistry::new();
    let targets = tools::ImageGenTargets::default();
    let mut ctx = make_ctx(&mut memory, &sessions, dir.path(), &providers, &targets);
    let args = serde_json::json!({});
    let err = tools::dispatch_tool(|_| true, &mut ctx, "delegate", &args)
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
    let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let providers = providers::registry::ProviderRegistry::new();
    let targets = tools::ImageGenTargets::default();
    let mut ctx = make_ctx(&mut memory, &sessions, dir.path(), &providers, &targets);
    let args = serde_json::json!({"goal": "do thing"});
    let err = tools::dispatch_tool(|_| true, &mut ctx, "delegate", &args)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("runner") || err.to_string().contains("API Key"),
        "got: {err}"
    );
}

#[tokio::test]
async fn delegate_async_status_collect_cancel_flow() {
    let _guard = ASYNC_TEST_LOCK.lock().unwrap();
    let spawner: delegate::DelegateAsyncSpawner = Arc::new(|task_id, _req| {
        let tid = task_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            delegate::AsyncDelegateRegistry::global().finish_ok(
                &tid,
                serde_json::json!({"ok": true, "summary": "done"}).to_string(),
            );
        });
    });

    let dir = tempfile::tempdir().unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let providers = providers::registry::ProviderRegistry::new();
    let targets = tools::ImageGenTargets::default();
    let mut ctx = make_ctx(&mut memory, &sessions, dir.path(), &providers, &targets);
    ctx.async_spawner = Some(spawner);

    let started = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "delegate_async",
        &serde_json::json!({"goal": "async job"}),
    )
    .await
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&started).unwrap();
    let task_id = v["task_id"].as_str().unwrap().to_string();
    assert_eq!(v["status"], "running");

    let status = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "delegate_status",
        &serde_json::json!({"task_id": task_id}),
    )
    .await
    .unwrap();
    let st: serde_json::Value = serde_json::from_str(&status).unwrap();
    assert!(
        st["status"] == "running" || st["status"] == "done",
        "status={st}"
    );

    let collected = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "delegate_collect",
        &serde_json::json!({"task_id": task_id, "timeout_secs": 5}),
    )
    .await
    .unwrap();
    let done: serde_json::Value = serde_json::from_str(&collected).unwrap();
    assert_eq!(done["status"], "done", "body={done}");
    assert_eq!(done["result"]["ok"], true, "body={done}");
}

#[tokio::test]
async fn delegate_async_cancel_marks_cancelled() {
    let _guard = ASYNC_TEST_LOCK.lock().unwrap();
    let spawner: delegate::DelegateAsyncSpawner = Arc::new(|task_id, _req| {
        let tid = task_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            delegate::AsyncDelegateRegistry::global()
                .finish_ok(&tid, r#"{"late":true}"#.into());
        });
    });

    let dir = tempfile::tempdir().unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let providers = providers::registry::ProviderRegistry::new();
    let targets = tools::ImageGenTargets::default();
    let mut ctx = make_ctx(&mut memory, &sessions, dir.path(), &providers, &targets);
    ctx.async_spawner = Some(spawner);

    let started = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "delegate_async",
        &serde_json::json!({"goal": "cancel me"}),
    )
    .await
    .unwrap();
    let task_id = serde_json::from_str::<serde_json::Value>(&started).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let cancelled = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "delegate_cancel",
        &serde_json::json!({"task_id": task_id}),
    )
    .await
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&cancelled).unwrap();
    assert_eq!(v["status"], "cancelled");
}

#[tokio::test]
async fn delegate_blocked_at_max_spawn_depth() {
    let _guard = ASYNC_TEST_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let providers = providers::registry::ProviderRegistry::new();
    let targets = tools::ImageGenTargets::default();
    let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let mut ctx = make_ctx(&mut memory, &sessions, dir.path(), &providers, &targets);

    let ctx_depth = home::SpawnDepthCtx {
        depth: 1,
        max_depth: 1,
    };
    let err = home::scope_spawn_depth(ctx_depth, async {
        tools::dispatch_tool(
            |_| true,
            &mut ctx,
            "delegate",
            &serde_json::json!({"goal": "nope"}),
        )
        .await
    })
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("spawn depth"),
        "got: {err}"
    );
}
