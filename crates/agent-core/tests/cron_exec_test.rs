//! 定时任务执行（`cron_exec`）凭证与错误路径测试。

#[tokio::test]
async fn execute_job_fails_without_api_key() {
    let dir = tempfile::TempDir::new().unwrap();
    let job = cron::CronJob {
        id: "job-test-1".into(),
        schedule: "every:1d".into(),
        task: "hi".into(),
        title: "test".into(),
        agent_id: "default".into(),
        provider_id: Some("openai".into()),
        model: Some("gpt-4o-mini".into()),
        enabled: true,
        created_at: "2026-07-11T00:00:00Z".into(),
        last_run_at: None,
        next_run_at: None,
        show_in_chat: false,
        archived_at: None,
    };
    let creds = agent::exec::cron::CronExecCredentials {
        provider: "openai".into(),
        model: "gpt-4o-mini".into(),
        api_key: String::new(),
        base_url: String::new(),
        targets: vec![],
    };
    let row = agent::exec::cron::execute_job_with_roots(dir.path(), &job, creds.clone(), "manual")
        .await
        .unwrap();
    assert_eq!(row.status, "failure");
    assert!(row.error.as_deref().unwrap_or("").contains("API"));
    // 旧 jobs 的 "default" 必须记为真实默认工作区 id，避免落到 workspace-default/
    assert_eq!(row.agent_id, home::DEFAULT_AGENT_ID);

    let repeated =
        agent::exec::cron::execute_job_with_roots(dir.path(), &job, creds.clone(), "manual")
            .await
            .unwrap();
    assert_eq!(repeated.session_id, row.session_id);

    let mut other_job = job.clone();
    other_job.id = "job-test-2".into();
    let other = agent::exec::cron::execute_job_with_roots(dir.path(), &other_job, creds, "manual")
        .await
        .unwrap();
    assert_ne!(other.session_id, row.session_id);
}

#[test]
fn normalize_cron_agent_id_maps_legacy_default() {
    assert_eq!(cron::normalize_cron_agent_id(""), home::DEFAULT_AGENT_ID);
    assert_eq!(
        cron::normalize_cron_agent_id("default"),
        home::DEFAULT_AGENT_ID
    );
    assert_eq!(
        cron::normalize_cron_agent_id("DEFAULT"),
        home::DEFAULT_AGENT_ID
    );
    assert_eq!(
        cron::normalize_cron_agent_id("workspace"),
        home::DEFAULT_AGENT_ID
    );
    assert_eq!(cron::normalize_cron_agent_id("Coder"), "coder");
}
