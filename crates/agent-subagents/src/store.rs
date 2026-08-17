use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::{AgentThread, AgentThreadMessage, AgentThreadStatus, SpawnAgentRequest};

const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS agent_threads (
    id TEXT PRIMARY KEY,
    parent_session_id TEXT NOT NULL,
    parent_agent_id TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    task TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT,
    error TEXT,
    model TEXT,
    model_reasoning_effort TEXT,
    sandbox_mode TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    finished_at TEXT,
    closed_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_agent_threads_parent_updated
    ON agent_threads(parent_session_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_threads_status_updated
    ON agent_threads(status, updated_at DESC);

CREATE TABLE IF NOT EXISTS agent_thread_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(thread_id) REFERENCES agent_threads(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_agent_thread_messages_thread_id
    ON agent_thread_messages(thread_id, id);
"#;

#[derive(Debug, Clone)]
pub struct AgentThreadStore {
    path: PathBuf,
}

impl AgentThreadStore {
    pub fn open_default() -> anyhow::Result<Self> {
        Self::new(home::default_memory_dir().join("subagents.db"))
    }

    pub fn new(path: PathBuf) -> anyhow::Result<Self> {
        let store = Self { path };
        store.connect()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connect(&self) -> anyhow::Result<Connection> {
        let conn = types::open_wal(&self.path)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(DDL)?;
        Ok(conn)
    }

    pub fn create(&self, request: &SpawnAgentRequest) -> anyhow::Result<AgentThread> {
        let id = Uuid::new_v4().to_string();
        let now = now();
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO agent_threads (
                id, parent_session_id, parent_agent_id, agent_name, task, status,
                summary, error, model, model_reasoning_effort, sandbox_mode,
                created_at, updated_at, finished_at, closed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, ?7, ?8, ?9, ?10, ?10, NULL, NULL)",
            params![
                id,
                request.parent_session_id,
                request.parent_agent_id,
                request.agent_name,
                request.task,
                AgentThreadStatus::Pending.as_str(),
                request.model,
                request.model_reasoning_effort,
                request.sandbox_mode,
                now,
            ],
        )?;
        self.append_message(&id, "user", &request.task)?;
        self.get(&id)?
            .ok_or_else(|| anyhow::anyhow!("created thread missing: {id}"))
    }

    pub fn get(&self, id: &str) -> anyhow::Result<Option<AgentThread>> {
        let conn = self.connect()?;
        conn.query_row(
            "SELECT id, parent_session_id, parent_agent_id, agent_name, task, status,
                    summary, error, model, model_reasoning_effort, sandbox_mode,
                    created_at, updated_at, finished_at, closed_at
             FROM agent_threads WHERE id = ?1",
            [id],
            thread_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list(
        &self,
        parent_session_id: Option<&str>,
        include_closed: bool,
    ) -> anyhow::Result<Vec<AgentThread>> {
        let conn = self.connect()?;
        let mut sql = String::from(
            "SELECT id, parent_session_id, parent_agent_id, agent_name, task, status,
                    summary, error, model, model_reasoning_effort, sandbox_mode,
                    created_at, updated_at, finished_at, closed_at
             FROM agent_threads WHERE 1=1",
        );
        if parent_session_id.is_some() {
            sql.push_str(" AND parent_session_id = ?1");
        }
        if !include_closed {
            sql.push_str(" AND status != 'closed'");
        }
        sql.push_str(" ORDER BY updated_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = if let Some(parent) = parent_session_id {
            stmt.query_map([parent], thread_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([], thread_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    }

    pub fn messages(&self, thread_id: &str) -> anyhow::Result<Vec<AgentThreadMessage>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT id, thread_id, role, content, created_at
             FROM agent_thread_messages WHERE thread_id = ?1 ORDER BY id",
        )?;
        let messages = stmt
            .query_map([thread_id], |row| {
                Ok(AgentThreadMessage {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(messages)
    }

    pub fn append_message(&self, thread_id: &str, role: &str, content: &str) -> anyhow::Result<()> {
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO agent_thread_messages(thread_id, role, content, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![thread_id, role, content, now()],
        )?;
        conn.execute(
            "UPDATE agent_threads SET updated_at = ?2 WHERE id = ?1",
            params![thread_id, now()],
        )?;
        Ok(())
    }

    pub fn set_status(
        &self,
        thread_id: &str,
        status: AgentThreadStatus,
        summary: Option<&str>,
        error: Option<&str>,
    ) -> anyhow::Result<()> {
        let now = now();
        let finished = status.is_wait_complete().then_some(now.as_str());
        let closed = (status == AgentThreadStatus::Closed).then_some(now.as_str());
        let conn = self.connect()?;
        conn.execute(
            "UPDATE agent_threads
             SET status = ?2,
                 summary = COALESCE(?3, summary),
                 error = ?4,
                 updated_at = ?5,
                 finished_at = ?6,
                 closed_at = COALESCE(?7, closed_at)
             WHERE id = ?1",
            params![
                thread_id,
                status.as_str(),
                summary,
                error,
                now,
                finished,
                closed,
            ],
        )?;
        Ok(())
    }

    pub fn count_active(&self, parent_session_id: &str) -> anyhow::Result<usize> {
        let conn = self.connect()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_threads
             WHERE parent_session_id = ?1 AND status IN ('pending', 'running')",
            [parent_session_id],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as usize)
    }

    pub fn interrupt_stale_running(&self) -> anyhow::Result<usize> {
        let conn = self.connect()?;
        let now = now();
        Ok(conn.execute(
            "UPDATE agent_threads
             SET status = 'interrupted', error = 'app restarted while agent was running',
                 updated_at = ?1, finished_at = ?1
             WHERE status IN ('pending', 'running')",
            [now],
        )?)
    }

    pub async fn wait(
        &self,
        thread_ids: &[String],
        timeout: Duration,
    ) -> anyhow::Result<Vec<AgentThread>> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut threads = Vec::with_capacity(thread_ids.len());
            let mut all_finished = true;
            for id in thread_ids {
                let thread = self
                    .get(id)?
                    .ok_or_else(|| anyhow::anyhow!("unknown agent thread: {id}"))?;
                all_finished &= thread.status.is_wait_complete();
                threads.push(thread);
            }
            if all_finished || Instant::now() >= deadline {
                return Ok(threads);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

fn thread_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentThread> {
    let status: String = row.get(5)?;
    Ok(AgentThread {
        id: row.get(0)?,
        parent_session_id: row.get(1)?,
        parent_agent_id: row.get(2)?,
        agent_name: row.get(3)?,
        task: row.get(4)?,
        status: AgentThreadStatus::parse(&status),
        summary: row.get(6)?,
        error: row.get(7)?,
        model: row.get(8)?,
        model_reasoning_effort: row.get(9)?,
        sandbox_mode: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        finished_at: row.get(13)?,
        closed_at: row.get(14)?,
    })
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> SpawnAgentRequest {
        SpawnAgentRequest {
            parent_session_id: "parent".into(),
            parent_agent_id: "astro".into(),
            task: "inspect".into(),
            agent_name: "explorer".into(),
            developer_instructions: "read only".into(),
            context_snapshot: String::new(),
            model: None,
            model_reasoning_effort: None,
            sandbox_mode: Some("read-only".into()),
            mcp_servers: Default::default(),
            skills_config: Vec::new(),
            chat_targets: vec![],
            project_root: None,
            hook_bus: None,
            interrupt_message: true,
        }
    }

    #[tokio::test]
    async fn thread_lifecycle_and_wait() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentThreadStore::new(dir.path().join("subagents.db")).unwrap();
        let thread = store.create(&request()).unwrap();
        store
            .set_status(&thread.id, AgentThreadStatus::Running, None, None)
            .unwrap();
        store
            .append_message(&thread.id, "assistant", "done")
            .unwrap();
        store
            .set_status(&thread.id, AgentThreadStatus::Completed, Some("done"), None)
            .unwrap();
        let waited = store
            .wait(std::slice::from_ref(&thread.id), Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(waited[0].status, AgentThreadStatus::Completed);
        assert_eq!(store.messages(&thread.id).unwrap().len(), 2);
    }
}
