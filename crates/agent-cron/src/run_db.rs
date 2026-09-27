//! 定时任务执行记录：SQLite 持久化（sqlx 异步）。

use std::path::{Path, PathBuf};

use agent_db::sqlx::{self, Row};
use agent_db::{AstroDb, DbSpec, SqlitePool};
use types::truncate_utf8;
use uuid::Uuid;

const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS cron_runs (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    title TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    schedule TEXT NOT NULL,
    task TEXT NOT NULL,
    fired_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    output TEXT NOT NULL DEFAULT '',
    error TEXT,
    session_id TEXT,
    trigger TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cron_runs_fired ON cron_runs(fired_at DESC);
CREATE INDEX IF NOT EXISTS idx_cron_runs_job ON cron_runs(job_id, fired_at DESC);
CREATE INDEX IF NOT EXISTS idx_cron_runs_agent ON cron_runs(agent_id, fired_at DESC);
"#;

const DB_SPEC: DbSpec = DbSpec::new("cron", "cron_v1.db");

const MAX_SUMMARY_BYTES: usize = 2 * 1024;
const MAX_OUTPUT_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone)]
pub struct CronRunRow {
    pub id: String,
    pub job_id: String,
    pub title: String,
    pub agent_id: String,
    pub schedule: String,
    pub task: String,
    pub fired_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub summary: String,
    pub output: String,
    pub error: Option<String>,
    pub session_id: Option<String>,
    pub trigger: String,
}

#[derive(Debug, Clone)]
pub struct NewCronRun {
    pub job_id: String,
    pub title: String,
    pub agent_id: String,
    pub schedule: String,
    pub task: String,
    pub fired_at: String,
    pub trigger: String,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CronRunFilters {
    pub job_id: Option<String>,
    pub agent_id: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub limit: u32,
}

pub struct CronRunDb {
    pool: SqlitePool,
    path: PathBuf,
}

pub fn cron_db_path(cron_root: &Path) -> PathBuf {
    cron_root.join(DB_SPEC.filename)
}

fn row_to_cron_run(r: &sqlx::sqlite::SqliteRow) -> CronRunRow {
    CronRunRow {
        id: r.get("id"),
        job_id: r.get("job_id"),
        title: r.get("title"),
        agent_id: r.get("agent_id"),
        schedule: r.get("schedule"),
        task: r.get("task"),
        fired_at: r.get("fired_at"),
        finished_at: r.get("finished_at"),
        status: r.get("status"),
        summary: r.get("summary"),
        output: r.get("output"),
        error: r.get("error"),
        session_id: r.get("session_id"),
        trigger: r.get("trigger"),
    }
}

impl CronRunDb {
    pub async fn new(path: PathBuf) -> anyhow::Result<Self> {
        let db = AstroDb::new(path.parent().unwrap_or(Path::new(".")));
        let pool = db.open_pool_at_path(&DB_SPEC, &path).await?;
        sqlx::query(DDL).execute(&pool).await?;
        Ok(Self { pool, path })
    }

    pub async fn open_default() -> anyhow::Result<Self> {
        let base = home::default_memory_dir();
        home::ensure_workspace_dirs(&base)?;
        Self::new(home::cron_run_db_path(&base)).await
    }

    pub fn db_path(&self) -> &Path {
        &self.path
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn insert_running(&self, row: NewCronRun) -> anyhow::Result<String> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO cron_runs (
                id, job_id, title, agent_id, schedule, task, fired_at, status, trigger, session_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', ?8, ?9)",
        )
        .bind(&id)
        .bind(&row.job_id)
        .bind(&row.title)
        .bind(&row.agent_id)
        .bind(&row.schedule)
        .bind(&row.task)
        .bind(&row.fired_at)
        .bind(&row.trigger)
        .bind(&row.session_id)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn finish_success(
        &self,
        id: &str,
        summary: &str,
        output: &str,
        finished_at: &str,
    ) -> anyhow::Result<()> {
        let summary = truncate_utf8(summary, MAX_SUMMARY_BYTES);
        let output = truncate_utf8(output, MAX_OUTPUT_BYTES);
        let result = sqlx::query(
            "UPDATE cron_runs
             SET status = 'success', summary = ?2, output = ?3, finished_at = ?4, error = NULL
             WHERE id = ?1",
        )
        .bind(id)
        .bind(summary)
        .bind(output)
        .bind(finished_at)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("cron run not found: {id}");
        }
        Ok(())
    }

    pub async fn finish_failure(
        &self,
        id: &str,
        error: &str,
        output: &str,
        finished_at: &str,
    ) -> anyhow::Result<()> {
        let output = truncate_utf8(output, MAX_OUTPUT_BYTES);
        let result = sqlx::query(
            "UPDATE cron_runs
             SET status = 'failure', error = ?2, output = ?3, finished_at = ?4
             WHERE id = ?1",
        )
        .bind(id)
        .bind(error)
        .bind(output)
        .bind(finished_at)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("cron run not found: {id}");
        }
        Ok(())
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Option<CronRunRow>> {
        let row = sqlx::query("SELECT * FROM cron_runs WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_cron_run(&r));
        Ok(row)
    }

    pub async fn get_by_session_id(&self, session_id: &str) -> anyhow::Result<Option<CronRunRow>> {
        let row = sqlx::query("SELECT * FROM cron_runs WHERE session_id = ?1 LIMIT 1")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_cron_run(&r));
        Ok(row)
    }

    pub async fn delete(&self, id: &str) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM cron_runs WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// 删除某个任务的全部运行记录，返回删除条数。
    pub async fn delete_for_job(&self, job_id: &str) -> anyhow::Result<u64> {
        let result = sqlx::query("DELETE FROM cron_runs WHERE job_id = ?1")
            .bind(job_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// 列出运行记录出现过的全部 job_id（用于识别任务已删除的孤儿记录）。
    pub async fn distinct_job_ids(&self) -> anyhow::Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT DISTINCT job_id FROM cron_runs")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    pub async fn has_running_for_job(&self, job_id: &str) -> anyhow::Result<bool> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM cron_runs WHERE job_id = ?1 AND status = 'running'",
        )
        .bind(job_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    pub async fn list_running(&self) -> anyhow::Result<Vec<CronRunRow>> {
        let rows =
            sqlx::query("SELECT * FROM cron_runs WHERE status = 'running' ORDER BY fired_at DESC")
                .fetch_all(&self.pool)
                .await?
                .iter()
                .map(row_to_cron_run)
                .collect();
        Ok(rows)
    }

    pub async fn list_failure_with_error(&self, error: &str) -> anyhow::Result<Vec<CronRunRow>> {
        let rows = sqlx::query(
            "SELECT * FROM cron_runs WHERE status = 'failure' AND error = ?1 ORDER BY fired_at DESC",
        )
        .bind(error)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(row_to_cron_run)
        .collect();
        Ok(rows)
    }

    pub async fn list_filtered(&self, f: CronRunFilters) -> anyhow::Result<Vec<CronRunRow>> {
        let limit = if f.limit == 0 { 50i64 } else { f.limit as i64 };
        let job_id = f.job_id.filter(|s| !s.is_empty());
        let agent_id = f.agent_id.filter(|s| !s.is_empty());
        let date_from = f.date_from.filter(|s| !s.is_empty());
        let date_to = f.date_to.filter(|s| !s.is_empty());

        let rows = sqlx::query(
            "SELECT * FROM cron_runs
             WHERE (?1 IS NULL OR job_id = ?1)
               AND (?2 IS NULL OR agent_id = ?2)
               AND (?3 IS NULL OR fired_at >= ?3)
               AND (?4 IS NULL OR fired_at <= ?4)
             ORDER BY fired_at DESC LIMIT ?5",
        )
        .bind(job_id.as_deref())
        .bind(agent_id.as_deref())
        .bind(date_from.as_deref())
        .bind(date_to.as_deref())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(row_to_cron_run)
        .collect();
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn finish_truncates_summary_and_output() {
        let dir = TempDir::new().unwrap();
        let db = CronRunDb::new(dir.path().join(DB_SPEC.filename))
            .await
            .unwrap();
        let id = db
            .insert_running(NewCronRun {
                job_id: "j".into(),
                title: "t".into(),
                agent_id: "workspace".into(),
                schedule: "every:1d".into(),
                task: "task".into(),
                fired_at: "2026-07-11T11:00:00+08:00".into(),
                trigger: "due".into(),
                session_id: None,
            })
            .await
            .unwrap();
        let long_summary = "x".repeat(MAX_SUMMARY_BYTES + 100);
        let long_output = "y".repeat(MAX_OUTPUT_BYTES + 100);
        db.finish_success(
            &id,
            &long_summary,
            &long_output,
            "2026-07-11T11:01:00+08:00",
        )
        .await
        .unwrap();
        let got = db.get(&id).await.unwrap().unwrap();
        assert!(got.summary.len() <= MAX_SUMMARY_BYTES);
        assert!(got.output.len() <= MAX_OUTPUT_BYTES);
    }

    #[tokio::test]
    async fn delete_removes_run() {
        let dir = TempDir::new().unwrap();
        let db = CronRunDb::new(dir.path().join(DB_SPEC.filename))
            .await
            .unwrap();
        let id = db
            .insert_running(NewCronRun {
                job_id: "j".into(),
                title: "t".into(),
                agent_id: "workspace".into(),
                schedule: "every:1d".into(),
                task: "task".into(),
                fired_at: "2026-07-11T11:00:00+08:00".into(),
                trigger: "manual".into(),
                session_id: None,
            })
            .await
            .unwrap();
        assert!(db.delete(&id).await.unwrap());
        assert!(db.get(&id).await.unwrap().is_none());
        assert!(!db.delete(&id).await.unwrap());
    }

    #[tokio::test]
    async fn delete_for_job_scopes_to_one_job() {
        let dir = TempDir::new().unwrap();
        let db = CronRunDb::new(dir.path().join(DB_SPEC.filename))
            .await
            .unwrap();
        for (job_id, fired_at) in [
            ("job-a", "2026-07-11T11:00:00+08:00"),
            ("job-a", "2026-07-11T12:00:00+08:00"),
            ("job-b", "2026-07-11T13:00:00+08:00"),
        ] {
            db.insert_running(NewCronRun {
                job_id: job_id.into(),
                title: "t".into(),
                agent_id: "workspace".into(),
                schedule: "every:1d".into(),
                task: "task".into(),
                fired_at: fired_at.into(),
                trigger: "due".into(),
                session_id: None,
            })
            .await
            .unwrap();
        }

        assert_eq!(db.delete_for_job("job-a").await.unwrap(), 2);
        assert_eq!(db.delete_for_job("job-a").await.unwrap(), 0);
        let mut remaining = db.distinct_job_ids().await.unwrap();
        remaining.sort();
        assert_eq!(remaining, vec!["job-b".to_string()]);
    }

    #[tokio::test]
    async fn opens_the_exact_requested_filename() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("custom-cron.db");
        let db = CronRunDb::new(path.clone()).await.unwrap();

        assert_eq!(db.db_path(), path.as_path());
        assert!(path.is_file());
        assert!(!dir.path().join(DB_SPEC.filename).exists());
    }
}
