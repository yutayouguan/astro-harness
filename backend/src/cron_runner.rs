//! 后台 cron ticker：认领到期任务并调用 `agent::cron_exec`。
//!
//! 注意：`rusqlite::Connection` 不可跨 `.await`，故先同步 `claim_due` 再异步执行。

use agent::cron_exec::{self, CronExecCredentials};
use memory::{CronJob, CronStore};

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
        targets: vec![],
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
