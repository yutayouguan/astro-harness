use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::Connection;

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
    output TEXT
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

/// 工作流运行记录行
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
}

/// 工作流步骤日志行
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
    conn: Connection,
}

impl WorkflowRunDb {
    pub fn new(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path).context("打开 workflow.db 失败")?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(DDL).context("workflow.db DDL 失败")?;
        Ok(Self { conn })
    }

    pub fn open_default() -> Result<Self> {
        let path = home::default_memory_dir().join("workflows").join("workflow.db");
        Self::new(path)
    }

    pub fn insert_run(
        &self,
        id: &str,
        workflow_id: &str,
        workflow_name: &str,
        trigger_type: &str,
        started_at: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO workflow_runs (id, workflow_id, workflow_name, trigger_type, started_at, status) VALUES (?1,?2,?3,?4,?5,'running')",
            rusqlite::params![id, workflow_id, workflow_name, trigger_type, started_at],
        )?;
        Ok(())
    }

    pub fn finish_run(
        &self,
        id: &str,
        status: &str,
        finished_at: &str,
        error: Option<&str>,
        output: Option<&str>,
        node_count: i64,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE workflow_runs SET status=?2, finished_at=?3, error=?4, output=?5, node_count=?6 WHERE id=?1",
            rusqlite::params![id, status, finished_at, error, output, node_count],
        )?;
        Ok(())
    }

    pub fn get_run(&self, id: &str) -> Result<Option<WorkflowRunRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, workflow_id, workflow_name, trigger_type, started_at, finished_at, status, error, node_count, output FROM workflow_runs WHERE id=?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![id], |row| {
            Ok(WorkflowRunRow {
                id: row.get(0)?,
                workflow_id: row.get(1)?,
                workflow_name: row.get(2)?,
                trigger_type: row.get(3)?,
                started_at: row.get(4)?,
                finished_at: row.get(5)?,
                status: row.get(6)?,
                error: row.get(7)?,
                node_count: row.get(8)?,
                output: row.get(9)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn list_runs(&self, workflow_id: Option<&str>, limit: i64) -> Result<Vec<WorkflowRunRow>> {
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(wid) =
            workflow_id
        {
            (
                "SELECT id, workflow_id, workflow_name, trigger_type, started_at, finished_at, status, error, node_count, output FROM workflow_runs WHERE workflow_id=?1 ORDER BY started_at DESC LIMIT ?2",
                vec![Box::new(wid.to_string()), Box::new(limit)],
            )
        } else {
            (
                "SELECT id, workflow_id, workflow_name, trigger_type, started_at, finished_at, status, error, node_count, output FROM workflow_runs ORDER BY started_at DESC LIMIT ?1",
                vec![Box::new(limit)],
            )
        };
        let mut stmt = self.conn.prepare(sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            Ok(WorkflowRunRow {
                id: row.get(0)?,
                workflow_id: row.get(1)?,
                workflow_name: row.get(2)?,
                trigger_type: row.get(3)?,
                started_at: row.get(4)?,
                finished_at: row.get(5)?,
                status: row.get(6)?,
                error: row.get(7)?,
                node_count: row.get(8)?,
                output: row.get(9)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn delete_run(&self, id: &str) -> Result<bool> {
        self.conn.execute_batch("BEGIN")?;
        self.conn
            .execute("DELETE FROM workflow_step_logs WHERE run_id=?1", rusqlite::params![id])?;
        let n = self
            .conn
            .execute("DELETE FROM workflow_runs WHERE id=?1", rusqlite::params![id])?;
        self.conn.execute_batch("COMMIT")?;
        Ok(n > 0)
    }

    pub fn insert_step_log(
        &self,
        id: &str,
        run_id: &str,
        node_id: &str,
        node_type: &str,
        node_label: &str,
        started_at: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO workflow_step_logs (id, run_id, node_id, node_type, node_label, started_at, status) VALUES (?1,?2,?3,?4,?5,?6,'running')",
            rusqlite::params![id, run_id, node_id, node_type, node_label, started_at],
        )?;
        Ok(())
    }

    pub fn finish_step_log(
        &self,
        id: &str,
        status: &str,
        finished_at: &str,
        output: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE workflow_step_logs SET status=?2, finished_at=?3, output=?4, error=?5 WHERE id=?1",
            rusqlite::params![id, status, finished_at, output, error],
        )?;
        Ok(())
    }

    pub fn list_step_logs(&self, run_id: &str) -> Result<Vec<WorkflowStepLogRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, run_id, node_id, node_type, node_label, started_at, finished_at, status, input, output, error FROM workflow_step_logs WHERE run_id=?1 ORDER BY started_at",
        )?;
        let rows = stmt.query_map(rusqlite::params![run_id], |row| {
            Ok(WorkflowStepLogRow {
                id: row.get(0)?,
                run_id: row.get(1)?,
                node_id: row.get(2)?,
                node_type: row.get(3)?,
                node_label: row.get(4)?,
                started_at: row.get(5)?,
                finished_at: row.get(6)?,
                status: row.get(7)?,
                input: row.get(8)?,
                output: row.get(9)?,
                error: row.get(10)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// 清理旧运行记录，保留最近 max_keep 条
    pub fn prune_old_runs(&self, max_keep: i64) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM workflow_runs", [], |r| r.get(0),
        )?;
        if count <= max_keep {
            return Ok(0);
        }
        let to_delete = count - max_keep;
        self.conn.execute_batch("BEGIN")?;
        self.conn.execute(
            "DELETE FROM workflow_step_logs WHERE run_id IN (SELECT id FROM workflow_runs ORDER BY started_at ASC LIMIT ?1)",
            rusqlite::params![to_delete],
        )?;
        let n = self.conn.execute(
            "DELETE FROM workflow_runs WHERE id IN (SELECT id FROM workflow_runs ORDER BY started_at ASC LIMIT ?1)",
            rusqlite::params![to_delete],
        )?;
        self.conn.execute_batch("COMMIT")?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let db = WorkflowRunDb::new(dir.path().join("workflow.db")).unwrap();

        db.insert_run("r1", "wf1", "测试流程", "manual", "2026-01-01T00:00:00+08:00")
            .unwrap();
        let run = db.get_run("r1").unwrap().unwrap();
        assert_eq!(run.status, "running");

        db.finish_run("r1", "success", "2026-01-01T00:01:00+08:00", None, Some("ok"), 3)
            .unwrap();
        let run = db.get_run("r1").unwrap().unwrap();
        assert_eq!(run.status, "success");
        assert_eq!(run.node_count, 3);

        let list = db.list_runs(Some("wf1"), 100).unwrap();
        assert_eq!(list.len(), 1);

        db.delete_run("r1").unwrap();
        assert!(db.get_run("r1").unwrap().is_none());
    }
}
