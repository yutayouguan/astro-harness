use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, types::Type, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::mailbox::{self, MailboxMessage, NewMailboxMessage};
use crate::migration::{self, HistoricalAgentMessage, HistoricalAgentThread};
use crate::{
    AgentPath, AgentStatusKind, AgentStatusV2, AgentThread, AgentThreadMessage, AgentThreadStatus,
    AgentThreadV2, AgentTreeSnapshotV2, RunnerEvent, SpawnAgentRequest, ThreadReservation,
};

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

const V2_THREAD_SELECT: &str =
    "thread_id, root_thread_id, parent_thread_id, canonical_path, task_name,
     agent_type, session_id, status_kind, status_payload, created_at, updated_at";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredStatusEvent {
    pub sequence: i64,
    pub thread_id: String,
    pub event_kind: String,
    pub event: RunnerEvent,
    pub source_turn_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct AgentGraphStore {
    path: PathBuf,
}

impl AgentGraphStore {
    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        let store = Self { path };
        let mut conn = store.connect()?;
        migration::migrate(&mut conn)
            .with_context(|| format!("migrate agent graph at {}", store.path.display()))?;
        Ok(store)
    }

    pub fn open_default_v2() -> anyhow::Result<Self> {
        Self::open(home::default_memory_dir().join("subagents.db"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connect(&self) -> anyhow::Result<Connection> {
        let conn = types::open_wal(&self.path)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        Ok(conn)
    }

    pub fn schema_version(&self) -> anyhow::Result<i32> {
        migration::schema_version(&self.connect()?)
    }

    pub fn reserve_thread(&self, reservation: &ThreadReservation) -> anyhow::Result<AgentThreadV2> {
        validate_reservation(reservation)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let timestamp = now();
        let status = AgentStatusV2::PendingInit;
        tx.execute(
            "INSERT INTO agent_threads (
                thread_id, root_thread_id, parent_thread_id, canonical_path,
                task_name, agent_type, session_id, status_kind, status_payload,
                last_status_sequence, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?10)",
            params![
                reservation.thread_id,
                reservation.root_thread_id,
                reservation.parent_thread_id,
                reservation.canonical_path.as_str(),
                reservation.task_name,
                reservation.agent_type,
                reservation.session_id,
                status_kind_str(status.kind()),
                serde_json::to_string(&status)?,
                timestamp,
            ],
        )?;
        tx.execute(
            "INSERT INTO agent_spawn_edges (
                parent_thread_id, child_thread_id, edge_state, created_at, closed_at
             ) VALUES (?1, ?2, 'open', ?3, NULL)",
            params![
                reservation.parent_thread_id,
                reservation.thread_id,
                timestamp,
            ],
        )?;
        let thread = query_v2_thread_by_id(&tx, &reservation.thread_id)?
            .context("reserved agent thread is missing")?;
        tx.commit()?;
        Ok(thread)
    }

    pub fn rollback_pending_thread(&self, thread_id: &str) -> anyhow::Result<()> {
        require_non_empty("thread_id", thread_id)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let status_kind = tx
            .query_row(
                "SELECT status_kind FROM agent_threads WHERE thread_id = ?1",
                [thread_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .with_context(|| format!("unknown agent thread {thread_id:?}"))?;
        if status_kind != "pending_init" {
            bail!(
                "cannot roll back agent thread {thread_id:?}: expected pending_init, found {status_kind}"
            );
        }
        tx.execute(
            "DELETE FROM agent_spawn_edges WHERE child_thread_id = ?1",
            [thread_id],
        )?;
        tx.execute(
            "DELETE FROM agent_threads WHERE thread_id = ?1",
            [thread_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_thread(&self, thread_id: &str) -> anyhow::Result<Option<AgentThreadV2>> {
        require_non_empty("thread_id", thread_id)?;
        query_v2_thread_by_id(&self.connect()?, thread_id)
    }

    pub fn get_by_path(
        &self,
        root_thread_id: &str,
        path: &AgentPath,
    ) -> anyhow::Result<Option<AgentThreadV2>> {
        require_non_empty("root_thread_id", root_thread_id)?;
        let conn = self.connect()?;
        conn.query_row(
            &format!(
                "SELECT {V2_THREAD_SELECT} FROM agent_threads
                 WHERE root_thread_id = ?1 AND canonical_path = ?2"
            ),
            params![root_thread_id, path.as_str()],
            v2_thread_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn apply_status_event(
        &self,
        thread_id: &str,
        event: RunnerEvent,
    ) -> anyhow::Result<AgentThreadV2> {
        require_non_empty("thread_id", thread_id)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        if query_v2_thread_by_id(&tx, thread_id)?.is_none() {
            bail!("unknown agent thread {thread_id:?}");
        }

        let status = status_for_event(&event);
        let event_kind = event_kind(&event);
        let source_turn_id = source_turn_id(&event);
        let timestamp = now();
        tx.execute(
            "INSERT INTO agent_status_events (
                thread_id, event_kind, payload, source_turn_id, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                thread_id,
                event_kind,
                serde_json::to_string(&event)?,
                source_turn_id,
                timestamp,
            ],
        )?;
        let sequence = tx.last_insert_rowid();
        tx.execute(
            "UPDATE agent_threads
             SET status_kind = ?2,
                 status_payload = ?3,
                 last_status_sequence = ?4,
                 updated_at = ?5
             WHERE thread_id = ?1",
            params![
                thread_id,
                status_kind_str(status.kind()),
                serde_json::to_string(&status)?,
                sequence,
                timestamp,
            ],
        )?;
        let thread =
            query_v2_thread_by_id(&tx, thread_id)?.context("updated agent thread is missing")?;
        tx.commit()?;
        Ok(thread)
    }

    pub fn status_events(&self, thread_id: &str) -> anyhow::Result<Vec<StoredStatusEvent>> {
        require_non_empty("thread_id", thread_id)?;
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT sequence, thread_id, event_kind, payload, source_turn_id, created_at
             FROM agent_status_events
             WHERE thread_id = ?1
             ORDER BY sequence",
        )?;
        let events = stmt
            .query_map([thread_id], stored_status_event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(events)
    }

    pub fn enqueue(&self, message: &NewMailboxMessage) -> anyhow::Result<MailboxMessage> {
        mailbox::enqueue(&mut self.connect()?, message)
    }

    pub fn pending_for(&self, recipient: &str, after: i64) -> anyhow::Result<Vec<MailboxMessage>> {
        mailbox::pending_for(&self.connect()?, recipient, after)
    }

    pub fn mark_delivered(&self, recipient: &str, through_sequence: i64) -> anyhow::Result<()> {
        mailbox::mark_delivered(&mut self.connect()?, recipient, through_sequence)
    }

    pub fn snapshot(&self, root_thread_id: &str) -> anyhow::Result<AgentTreeSnapshotV2> {
        require_non_empty("root_thread_id", root_thread_id)?;
        let conn = self.connect()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {V2_THREAD_SELECT} FROM agent_threads
             WHERE root_thread_id = ?1
             ORDER BY canonical_path"
        ))?;
        let threads = stmt
            .query_map([root_thread_id], v2_thread_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let activity_sequence: i64 = conn.query_row(
            "SELECT COALESCE(MAX(last_status_sequence), 0)
             FROM agent_threads WHERE root_thread_id = ?1",
            [root_thread_id],
            |row| row.get(0),
        )?;
        Ok(AgentTreeSnapshotV2 {
            root_thread_id: root_thread_id.to_string(),
            threads,
            activity_sequence: activity_sequence.try_into().with_context(|| {
                format!("invalid negative status activity sequence {activity_sequence}")
            })?,
        })
    }

    pub fn close_edge(&self, child_thread_id: &str) -> anyhow::Result<()> {
        require_non_empty("child_thread_id", child_thread_id)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE agent_spawn_edges
             SET edge_state = 'closed', closed_at = COALESCE(closed_at, ?2)
             WHERE child_thread_id = ?1",
            params![child_thread_id, now()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_historical_threads(&self) -> anyhow::Result<Vec<HistoricalAgentThread>> {
        migration::list_historical_threads(&self.connect()?)
    }

    pub fn list_historical_messages(
        &self,
        legacy_thread_id: &str,
    ) -> anyhow::Result<Vec<HistoricalAgentMessage>> {
        require_non_empty("legacy_thread_id", legacy_thread_id)?;
        migration::list_historical_messages(&self.connect()?, legacy_thread_id)
    }
}

fn validate_reservation(reservation: &ThreadReservation) -> anyhow::Result<()> {
    for (label, value) in [
        ("thread_id", reservation.thread_id.as_str()),
        ("root_thread_id", reservation.root_thread_id.as_str()),
        ("parent_thread_id", reservation.parent_thread_id.as_str()),
        ("task_name", reservation.task_name.as_str()),
        ("agent_type", reservation.agent_type.as_str()),
        ("session_id", reservation.session_id.as_str()),
    ] {
        require_non_empty(label, value)?;
    }
    if reservation.canonical_path == AgentPath::root() {
        bail!("a child thread reservation cannot use the root path");
    }
    if reservation.canonical_path.name() != reservation.task_name {
        bail!(
            "reservation task_name {:?} does not match canonical path {:?}",
            reservation.task_name,
            reservation.canonical_path
        );
    }
    Ok(())
}

fn require_non_empty(label: &str, value: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(())
}

fn query_v2_thread_by_id(
    conn: &Connection,
    thread_id: &str,
) -> anyhow::Result<Option<AgentThreadV2>> {
    conn.query_row(
        &format!("SELECT {V2_THREAD_SELECT} FROM agent_threads WHERE thread_id = ?1"),
        [thread_id],
        v2_thread_from_row,
    )
    .optional()
    .map_err(Into::into)
}

fn v2_thread_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentThreadV2> {
    let canonical_path = row.get::<_, String>(3)?;
    let status_kind = row.get::<_, String>(7)?;
    let status_payload = row.get::<_, String>(8)?;
    let status = serde_json::from_str::<AgentStatusV2>(&status_payload)
        .map_err(|error| sql_conversion_error(8, Type::Text, error))?;
    if status_kind != status_kind_str(status.kind()) {
        return Err(sql_conversion_error(
            8,
            Type::Text,
            format!(
                "status kind {status_kind:?} does not match payload kind {:?}",
                status.kind()
            ),
        ));
    }
    Ok(AgentThreadV2 {
        thread_id: row.get(0)?,
        root_thread_id: row.get(1)?,
        parent_thread_id: row.get(2)?,
        canonical_path: AgentPath::parse(&canonical_path)
            .map_err(|error| sql_conversion_error(3, Type::Text, error))?,
        task_name: row.get(4)?,
        agent_type: row.get(5)?,
        session_id: row.get(6)?,
        status,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn stored_status_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredStatusEvent> {
    let stored_event_kind = row.get::<_, String>(2)?;
    let payload = row.get::<_, String>(3)?;
    let event = serde_json::from_str::<RunnerEvent>(&payload)
        .map_err(|error| sql_conversion_error(3, Type::Text, error))?;
    if stored_event_kind != event_kind(&event) {
        return Err(sql_conversion_error(
            3,
            Type::Text,
            format!(
                "event kind {stored_event_kind:?} does not match payload kind {:?}",
                event_kind(&event)
            ),
        ));
    }
    Ok(StoredStatusEvent {
        sequence: row.get(0)?,
        thread_id: row.get(1)?,
        event_kind: stored_event_kind,
        event,
        source_turn_id: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn status_for_event(event: &RunnerEvent) -> AgentStatusV2 {
    match event {
        RunnerEvent::TurnStarted { .. } => AgentStatusV2::Running,
        RunnerEvent::TurnCompleted { last_message, .. } => AgentStatusV2::Completed {
            last_message: last_message.clone(),
        },
        RunnerEvent::TurnInterrupted { .. } => AgentStatusV2::Interrupted,
        RunnerEvent::TurnErrored { message, .. } => AgentStatusV2::Errored {
            message: message.clone(),
        },
        RunnerEvent::RuntimeTerminated => AgentStatusV2::Shutdown,
    }
}

fn status_kind_str(kind: AgentStatusKind) -> &'static str {
    match kind {
        AgentStatusKind::PendingInit => "pending_init",
        AgentStatusKind::Running => "running",
        AgentStatusKind::Interrupted => "interrupted",
        AgentStatusKind::Completed => "completed",
        AgentStatusKind::Errored => "errored",
        AgentStatusKind::Shutdown => "shutdown",
    }
}

fn event_kind(event: &RunnerEvent) -> &'static str {
    match event {
        RunnerEvent::TurnStarted { .. } => "turn_started",
        RunnerEvent::TurnCompleted { .. } => "turn_completed",
        RunnerEvent::TurnInterrupted { .. } => "turn_interrupted",
        RunnerEvent::TurnErrored { .. } => "turn_errored",
        RunnerEvent::RuntimeTerminated => "runtime_terminated",
    }
}

fn source_turn_id(event: &RunnerEvent) -> Option<&str> {
    match event {
        RunnerEvent::TurnStarted { turn_id }
        | RunnerEvent::TurnCompleted { turn_id, .. }
        | RunnerEvent::TurnInterrupted { turn_id, .. }
        | RunnerEvent::TurnErrored { turn_id, .. } => Some(turn_id),
        RunnerEvent::RuntimeTerminated => None,
    }
}

fn sql_conversion_error(
    column: usize,
    value_type: Type,
    error: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, value_type, error.into())
}

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
    use crate::{AgentPath, AgentStatusV2, RunnerEvent, ThreadReservation};

    fn reservation(id: &str, path: &str) -> ThreadReservation {
        ThreadReservation {
            thread_id: id.into(),
            root_thread_id: "root-thread".into(),
            parent_thread_id: "root-thread".into(),
            canonical_path: AgentPath::parse(path).unwrap(),
            task_name: path.rsplit('/').next().unwrap().into(),
            agent_type: "explorer".into(),
            session_id: format!("session-{id}"),
        }
    }

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

    #[test]
    fn runner_events_atomically_update_projection_and_append_status_activity() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        let thread = store
            .reserve_thread(&reservation("child", "/root/child"))
            .unwrap();
        assert_eq!(thread.status, AgentStatusV2::PendingInit);

        let started = RunnerEvent::TurnStarted {
            turn_id: "turn-1".into(),
        };
        let running = store.apply_status_event("child", started.clone()).unwrap();
        assert_eq!(running.status, AgentStatusV2::Running);
        let events = store.status_events("child").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, started);

        let cases = [
            (
                RunnerEvent::TurnCompleted {
                    turn_id: "turn-2".into(),
                    last_message: "done".into(),
                },
                AgentStatusV2::Completed {
                    last_message: "done".into(),
                },
            ),
            (
                RunnerEvent::TurnInterrupted {
                    turn_id: "turn-3".into(),
                    reason: "cancelled".into(),
                },
                AgentStatusV2::Interrupted,
            ),
            (
                RunnerEvent::TurnErrored {
                    turn_id: "turn-4".into(),
                    message: "failed".into(),
                },
                AgentStatusV2::Errored {
                    message: "failed".into(),
                },
            ),
            (RunnerEvent::RuntimeTerminated, AgentStatusV2::Shutdown),
        ];
        for (event, expected) in cases {
            let projected = store.apply_status_event("child", event).unwrap();
            assert_eq!(projected.status, expected);
        }
        assert_eq!(store.status_events("child").unwrap().len(), 5);
    }

    #[test]
    fn rollback_only_removes_pending_reservation_and_spawn_edge() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store
            .reserve_thread(&reservation("pending", "/root/pending"))
            .unwrap();
        store.rollback_pending_thread("pending").unwrap();
        assert!(store.get_thread("pending").unwrap().is_none());
        let pending_edges: i64 = store
            .connect()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM agent_spawn_edges WHERE child_thread_id = 'pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending_edges, 0);

        store
            .reserve_thread(&reservation("running", "/root/running"))
            .unwrap();
        store
            .apply_status_event(
                "running",
                RunnerEvent::TurnStarted {
                    turn_id: "turn".into(),
                },
            )
            .unwrap();
        let error = store.rollback_pending_thread("running").unwrap_err();
        assert!(error.to_string().contains("pending_init"));
        assert_eq!(
            store.get_thread("running").unwrap().unwrap().status,
            AgentStatusV2::Running
        );
    }

    #[test]
    fn snapshot_sorts_canonical_paths_and_has_stable_status_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents-v2.db");
        let store = AgentGraphStore::open(path.clone()).unwrap();
        store
            .reserve_thread(&reservation("z-thread", "/root/z_task"))
            .unwrap();
        store
            .reserve_thread(&reservation("a-thread", "/root/a_task"))
            .unwrap();
        store
            .apply_status_event(
                "z-thread",
                RunnerEvent::TurnStarted {
                    turn_id: "turn-z".into(),
                },
            )
            .unwrap();
        store
            .apply_status_event(
                "a-thread",
                RunnerEvent::TurnStarted {
                    turn_id: "turn-a".into(),
                },
            )
            .unwrap();

        let snapshot = store.snapshot("root-thread").unwrap();
        let paths = snapshot
            .threads
            .iter()
            .map(|thread| thread.canonical_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, vec!["/root/a_task", "/root/z_task"]);
        assert_eq!(snapshot.activity_sequence, 2);
        assert_eq!(
            store
                .get_by_path("root-thread", &AgentPath::parse("/root/a_task").unwrap())
                .unwrap()
                .unwrap()
                .thread_id,
            "a-thread"
        );
        drop(store);

        let reopened = AgentGraphStore::open(path).unwrap();
        assert_eq!(
            reopened.snapshot("root-thread").unwrap().activity_sequence,
            snapshot.activity_sequence
        );
    }

    #[test]
    fn root_path_is_unique_and_close_edge_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store
            .reserve_thread(&reservation("first", "/root/task"))
            .unwrap();
        assert!(store
            .reserve_thread(&reservation("second", "/root/task"))
            .is_err());
        store.close_edge("first").unwrap();
        store.close_edge("first").unwrap();
        assert_eq!(
            store.get_thread("first").unwrap().unwrap().status,
            AgentStatusV2::PendingInit
        );
    }
}
