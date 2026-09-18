use std::path::{Path, PathBuf};

use agent_db::sqlx::{self, Row};
use agent_db::{AstroDb, DbSpec, SqlitePool};
use anyhow::Result;

const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS workflow_runs (
    id TEXT PRIMARY KEY,
    workflow_id TEXT NOT NULL,
    workflow_name TEXT NOT NULL DEFAULT '',
    trigger_type TEXT NOT NULL DEFAULT 'manual',
    started_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT NOT NULL DEFAULT 'running',
    error TEXT,
    node_count INTEGER DEFAULT 0,
    output TEXT,
    owner_session_id TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_wf_runs_wid ON workflow_runs(workflow_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_wf_runs_started ON workflow_runs(started_at DESC);

CREATE TABLE IF NOT EXISTS workflow_step_logs (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    node_type TEXT NOT NULL,
    node_label TEXT NOT NULL DEFAULT '',
    started_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT NOT NULL DEFAULT 'running',
    input TEXT,
    output TEXT,
    error TEXT,
    FOREIGN KEY (run_id) REFERENCES workflow_runs(id)
);
CREATE INDEX IF NOT EXISTS idx_wf_steps_run ON workflow_step_logs(run_id, node_id);
"#;

const DB_SPEC: DbSpec = DbSpec::new("workflow", "workflow.db");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowRunRow {
    pub id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub trigger_type: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub node_count: i64,
    pub output: Option<String>,
    #[serde(default, skip_serializing)]
    pub owner_session_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowStepLogRow {
    pub id: String,
    pub run_id: String,
    pub node_id: String,
    pub node_type: String,
    pub node_label: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub input: Option<String>,
    pub output: Option<String>,
    pub error: Option<String>,
}

pub struct WorkflowRunDb {
    pool: SqlitePool,
}

fn row_to_run(r: &sqlx::sqlite::SqliteRow) -> WorkflowRunRow {
    WorkflowRunRow {
        id: r.get("id"),
        workflow_id: r.get("workflow_id"),
        workflow_name: r.get("workflow_name"),
        trigger_type: r.get("trigger_type"),
        started_at: r.get("started_at"),
        finished_at: r.get("finished_at"),
        status: r.get("status"),
        error: r.get("error"),
        node_count: r.get("node_count"),
        output: r.get("output"),
        owner_session_id: r.get("owner_session_id"),
    }
}

fn row_to_step(r: &sqlx::sqlite::SqliteRow) -> WorkflowStepLogRow {
    WorkflowStepLogRow {
        id: r.get("id"),
        run_id: r.get("run_id"),
        node_id: r.get("node_id"),
        node_type: r.get("node_type"),
        node_label: r.get("node_label"),
        started_at: r.get("started_at"),
        finished_at: r.get("finished_at"),
        status: r.get("status"),
        input: r.get("input"),
        output: r.get("output"),
        error: r.get("error"),
    }
}

impl WorkflowRunDb {
    pub async fn new(path: PathBuf) -> Result<Self> {
        let db = AstroDb::new(path.parent().unwrap_or(Path::new(".")));
        let pool = db.open_pool_at_path(&DB_SPEC, &path).await?;
        sqlx::query(DDL).execute(&pool).await?;
        let columns = sqlx::query("PRAGMA table_info(workflow_runs)")
            .fetch_all(&pool)
            .await?;
        if !columns
            .iter()
            .any(|row| row.get::<String, _>("name") == "owner_session_id")
        {
            if let Err(error) = sqlx::query(
                "ALTER TABLE workflow_runs ADD COLUMN owner_session_id TEXT NOT NULL DEFAULT ''",
            )
            .execute(&pool)
            .await
            {
                let message = error.to_string();
                if !message.contains("duplicate column name") {
                    return Err(error.into());
                }
            }
        }
        Ok(Self { pool })
    }

    pub async fn open_default() -> Result<Self> {
        let path = home::workflow_db_path(&home::default_memory_dir());
        Self::new(path).await
    }

    pub async fn insert_run(
        &self,
        id: &str,
        workflow_id: &str,
        workflow_name: &str,
        trigger_type: &str,
        started_at: &str,
    ) -> Result<()> {
        self.insert_run_owned(id, workflow_id, workflow_name, trigger_type, started_at, "")
            .await
    }

    pub async fn insert_run_owned(
        &self,
        id: &str,
        workflow_id: &str,
        workflow_name: &str,
        trigger_type: &str,
        started_at: &str,
        owner_session_id: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO workflow_runs (id, workflow_id, workflow_name, trigger_type, started_at, status, owner_session_id) VALUES (?1,?2,?3,?4,?5,'running',?6)",
        )
        .bind(id)
        .bind(workflow_id)
        .bind(workflow_name)
        .bind(trigger_type)
        .bind(started_at)
        .bind(owner_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn finish_run(
        &self,
        id: &str,
        status: &str,
        finished_at: &str,
        error: Option<&str>,
        output: Option<&str>,
        node_count: i64,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE workflow_runs SET status=?2, finished_at=?3, error=?4, output=?5, node_count=?6 WHERE id=?1",
        )
        .bind(id)
        .bind(status)
        .bind(finished_at)
        .bind(error)
        .bind(output)
        .bind(node_count)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_run(&self, id: &str) -> Result<Option<WorkflowRunRow>> {
        let row = sqlx::query("SELECT * FROM workflow_runs WHERE id=?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_run(&r));
        Ok(row)
    }

    pub async fn list_runs(
        &self,
        workflow_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<WorkflowRunRow>> {
        let rows = sqlx::query(
            "SELECT * FROM workflow_runs
             WHERE (?1 IS NULL OR workflow_id = ?1)
             ORDER BY started_at DESC LIMIT ?2",
        )
        .bind(workflow_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(row_to_run)
        .collect();
        Ok(rows)
    }

    pub async fn delete_run(&self, id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM workflow_step_logs WHERE run_id=?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let result = sqlx::query("DELETE FROM workflow_runs WHERE id=?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn insert_step_log(
        &self,
        id: &str,
        run_id: &str,
        node_id: &str,
        node_type: &str,
        node_label: &str,
        started_at: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO workflow_step_logs (id, run_id, node_id, node_type, node_label, started_at, status) VALUES (?1,?2,?3,?4,?5,?6,'running')",
        )
        .bind(id)
        .bind(run_id)
        .bind(node_id)
        .bind(node_type)
        .bind(node_label)
        .bind(started_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn finish_step_log(
        &self,
        id: &str,
        status: &str,
        finished_at: &str,
        output: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE workflow_step_logs SET status=?2, finished_at=?3, output=?4, error=?5 WHERE id=?1",
        )
        .bind(id)
        .bind(status)
        .bind(finished_at)
        .bind(output)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_step_logs(&self, run_id: &str) -> Result<Vec<WorkflowStepLogRow>> {
        let rows =
            sqlx::query("SELECT * FROM workflow_step_logs WHERE run_id=?1 ORDER BY started_at")
                .bind(run_id)
                .fetch_all(&self.pool)
                .await?
                .iter()
                .map(row_to_step)
                .collect();
        Ok(rows)
    }

    pub async fn prune_old_runs(&self, max_keep: i64) -> Result<usize> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM workflow_runs")
            .fetch_one(&self.pool)
            .await?;
        if count <= max_keep {
            return Ok(0);
        }
        let to_delete = count - max_keep;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM workflow_step_logs WHERE run_id IN (SELECT id FROM workflow_runs ORDER BY started_at ASC LIMIT ?1)",
        )
        .bind(to_delete)
        .execute(&mut *tx)
        .await?;
        let result = sqlx::query(
            "DELETE FROM workflow_runs WHERE id IN (SELECT id FROM workflow_runs ORDER BY started_at ASC LIMIT ?1)",
        )
        .bind(to_delete)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result.rows_affected() as usize)
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // 测试直接开原始 pool，生产路径必须走 agent-db
mod tests {
    use super::*;
    use agent_db::sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    #[tokio::test]
    async fn run_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let db = WorkflowRunDb::new(dir.path().join("workflow.db"))
            .await
            .unwrap();

        db.insert_run(
            "r1",
            "wf1",
            "测试流程",
            "manual",
            "2026-01-01T00:00:00+08:00",
        )
        .await
        .unwrap();
        let run = db.get_run("r1").await.unwrap().unwrap();
        assert_eq!(run.status, "running");
        assert!(run.owner_session_id.is_empty());

        db.insert_run_owned(
            "r-owned",
            "wf1",
            "测试流程",
            "agent_tool",
            "2026-01-01T00:00:00+08:00",
            "session-1",
        )
        .await
        .unwrap();
        assert_eq!(
            db.get_run("r-owned")
                .await
                .unwrap()
                .unwrap()
                .owner_session_id,
            "session-1"
        );

        db.finish_run(
            "r1",
            "success",
            "2026-01-01T00:01:00+08:00",
            None,
            Some("ok"),
            3,
        )
        .await
        .unwrap();
        let run = db.get_run("r1").await.unwrap().unwrap();
        assert_eq!(run.status, "success");
        assert_eq!(run.node_count, 3);

        let list = db.list_runs(Some("wf1"), 100).await.unwrap();
        assert_eq!(list.len(), 2);

        db.delete_run("r1").await.unwrap();
        assert!(db.get_run("r1").await.unwrap().is_none());
        db.delete_run("r-owned").await.unwrap();
    }

    #[tokio::test]
    async fn opens_the_exact_requested_filename() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("custom-workflow.db");
        let _db = WorkflowRunDb::new(path.clone()).await.unwrap();

        assert!(path.is_file());
        assert!(!dir.path().join(DB_SPEC.filename).exists());
    }

    #[tokio::test]
    async fn existing_database_adds_owner_session_column() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("legacy-workflow.db");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true),
            )
            .await
            .expect("legacy pool");
        sqlx::query(
            "CREATE TABLE workflow_runs (
                id TEXT PRIMARY KEY,
                workflow_id TEXT NOT NULL,
                workflow_name TEXT NOT NULL DEFAULT '',
                trigger_type TEXT NOT NULL DEFAULT 'manual',
                started_at TEXT NOT NULL,
                finished_at TEXT,
                status TEXT NOT NULL DEFAULT 'running',
                error TEXT,
                node_count INTEGER DEFAULT 0,
                output TEXT
            )",
        )
        .execute(&pool)
        .await
        .expect("legacy schema");
        pool.close().await;

        let db = WorkflowRunDb::new(path).await.expect("migrated db");
        db.insert_run_owned(
            "owned",
            "workflow",
            "Workflow",
            "agent_tool",
            "now",
            "session-1",
        )
        .await
        .expect("insert owned run");
        assert_eq!(
            db.get_run("owned")
                .await
                .expect("read run")
                .expect("owned run")
                .owner_session_id,
            "session-1"
        );
    }
}
