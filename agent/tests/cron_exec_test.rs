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
    };
    let creds = agent::cron_exec::CronExecCredentials {
        provider: "openai".into(),
        model: "gpt-4o-mini".into(),
        api_key: String::new(),
        base_url: String::new(),
        targets: vec![],
    };
    let row = agent::cron_exec::execute_job_with_roots(
        dir.path(),
        &job,
        creds,
        "manual",
    )
    .await
    .unwrap();
    assert_eq!(row.status, "failure");
    assert!(row.error.as_deref().unwrap_or("").contains("API"));
}
