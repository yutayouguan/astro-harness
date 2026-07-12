//! 后台 cron ticker：迁移遗留输出、认领到期任务并调用 `agent::cron_exec`。
//!
//! 注意：`rusqlite::Connection` 不可跨 `.await`，故先同步 `claim_due` 再异步执行。

use agent::cron_exec::{self, CronExecCredentials};
use memory::{cron_dir, CronJob, CronRunDb, CronStore};

/// 从任务字段与环境变量解析执行凭据；缺省 provider 为 `ollama`。
fn resolve_cron_credentials(job: &CronJob) -> CronExecCredentials {
    let provider = job
        .provider_id
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "ollama".into());
    let model = job.model.clone().unwrap_or_default();
    let api_key = providers::client::read_env_api_key(&provider).unwrap_or_default();
    let base_url = std::env::var(format!("{}_BASE_URL", provider.to_uppercase())).unwrap_or_default();
    CronExecCredentials {
        provider,
        model,
        api_key,
        base_url,
    }
}

/// 将 `cron/output` 目录下遗留 JSON 迁入 [`CronRunDb`]（失败仅打日志）。
pub fn migrate_legacy_output() {
    match CronRunDb::open_default() {
        Ok(db) => {
            let output_dir = cron_dir().join("output");
            match db.migrate_output_dir(&output_dir) {
                Ok(count) if count > 0 => {
                    tracing::info!(count, "migrated legacy cron output JSON to SQLite");
                }
                Ok(_) => {}
                Err(err) => tracing::warn!(error = %err, "cron output migration failed"),
            }
        }
        Err(err) => tracing::warn!(error = %err, "failed to open cron run db for migration"),
    }
}

/// 认领到期任务并逐个执行；打开 store / claim 失败时提前返回。
pub async fn tick_and_execute() {
    // 先同步 claim，再 await 执行，避免 rusqlite Connection 跨 .await 导致 Future !Send
    let jobs = {
        let store = match CronStore::open_default() {
            Ok(store) => store,
            Err(err) => {
                tracing::warn!(error = %err, "cron tick: failed to open store");
                return;
            }
        };

        match store.claim_due() {
            Ok(jobs) => jobs,
            Err(err) => {
                tracing::warn!(error = %err, "cron tick failed");
                return;
            }
        }
    };

    for job in jobs {
        let creds = resolve_cron_credentials(&job);
        match cron_exec::execute_job(&job, creds, "due").await {
            Ok(row) => {
                memory::UsageDb::try_record(memory::NewUsageEvent {
                    ts: chrono::Utc::now().to_rfc3339(),
                    kind: "cron".into(),
                    name: job.id.clone(),
                    agent_id: job.agent_id.clone(),
                    session_id: row.session_id.clone(),
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    cost_usd: 0.0,
                    meta_json: Some(
                        serde_json::json!({ "title": job.title, "trigger": "due" }).to_string(),
                    ),
                });
                tracing::info!(
                    job_id = %job.id,
                    run_id = %row.id,
                    status = %row.status,
                    schedule = %job.schedule,
                    task = %job.task,
                    "cron job executed"
                );
            }
            Err(err) => tracing::warn!(
                job_id = %job.id,
                schedule = %job.schedule,
                task = %job.task,
                error = %err,
                "cron job execution failed"
            ),
        }
    }
}
