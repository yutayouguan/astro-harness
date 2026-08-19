use std::path::{Path, PathBuf};
#[cfg(test)]
use std::time::Duration;

use anyhow::{bail, Context};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, types::Type, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::mailbox::{self, MailboxMessage, NewMailboxMessage};
use crate::migration::{self, HistoricalAgentMessage, HistoricalAgentThread};
use crate::{
    AgentPath, AgentRuntimeDescriptorV2, AgentStatusKind, AgentStatusV2, AgentThreadV2,
    AgentTreeSnapshotV2, RunnerEvent, ThreadReservation,
};

const V2_THREAD_SELECT: &str =
    "thread_id, root_thread_id, parent_thread_id, canonical_path, task_name,
     agent_type, session_id, status_kind, status_payload, created_at, updated_at";

fn v2_default_db_path() -> PathBuf {
    home::default_memory_dir().join("subagents-v2.db")
}

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
        Self::open(v2_default_db_path())
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

    pub fn ensure_root_thread(&self, root_thread_id: &str) -> anyhow::Result<AgentThreadV2> {
        require_non_empty("root_thread_id", root_thread_id)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let timestamp = now();
        let status = AgentStatusV2::Running;
        tx.execute(
            "INSERT INTO agent_threads (
                thread_id, root_thread_id, parent_thread_id, canonical_path,
                task_name, agent_type, session_id, status_kind, status_payload,
                last_status_sequence, created_at, updated_at
             ) VALUES (?1, ?1, NULL, '/root', 'root', 'root', ?1, ?2, ?3, 0, ?4, ?4)
             ON CONFLICT DO NOTHING",
            params![
                root_thread_id,
                status_kind_str(status.kind()),
                serde_json::to_string(&status)?,
                timestamp,
            ],
        )?;
        let root = tx
            .query_row(
                &format!(
                    "SELECT {V2_THREAD_SELECT} FROM agent_threads
                     WHERE root_thread_id = ?1 AND canonical_path = '/root'"
                ),
                [root_thread_id],
                v2_thread_from_row,
            )
            .optional()?
            .with_context(|| {
                format!("root agent row for thread {root_thread_id:?} could not be ensured")
            })?;
        if root.thread_id != root_thread_id
            || root.root_thread_id != root_thread_id
            || root.parent_thread_id.is_some()
            || root.session_id != root_thread_id
        {
            bail!("existing root agent row does not match root thread {root_thread_id:?}");
        }
        tx.commit()?;
        Ok(root)
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

    pub fn record_runtime_descriptor(
        &self,
        descriptor: &AgentRuntimeDescriptorV2,
    ) -> anyhow::Result<()> {
        require_non_empty("thread_id", &descriptor.thread_id)?;
        self.connect()?.execute(
            "INSERT INTO agent_runtime_descriptors (thread_id, model, reasoning_effort)
             VALUES (?1, ?2, ?3)",
            params![
                descriptor.thread_id,
                descriptor.model,
                descriptor.reasoning_effort,
            ],
        )?;
        Ok(())
    }

    pub fn runtime_descriptor(
        &self,
        thread_id: &str,
    ) -> anyhow::Result<Option<AgentRuntimeDescriptorV2>> {
        require_non_empty("thread_id", thread_id)?;
        Ok(self
            .connect()?
            .query_row(
                "SELECT thread_id, model, reasoning_effort
                 FROM agent_runtime_descriptors WHERE thread_id = ?1",
                [thread_id],
                |row| {
                    Ok(AgentRuntimeDescriptorV2 {
                        thread_id: row.get(0)?,
                        model: row.get(1)?,
                        reasoning_effort: row.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn rollback_pending_thread(&self, thread_id: &str) -> anyhow::Result<()> {
        require_non_empty("thread_id", thread_id)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
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

    /// Roll back the single durable TurnStarted written before a spawn caller
    /// accepted ownership. No terminal or later-generation event may exist.
    pub(crate) fn rollback_unaccepted_started_thread(
        &self,
        thread_id: &str,
        turn_id: &str,
    ) -> anyhow::Result<()> {
        require_non_empty("thread_id", thread_id)?;
        require_non_empty("turn_id", turn_id)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let status_kind = tx
            .query_row(
                "SELECT status_kind FROM agent_threads WHERE thread_id = ?1",
                [thread_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .with_context(|| format!("unknown agent thread {thread_id:?}"))?;
        anyhow::ensure!(
            status_kind == "running",
            "cannot roll back unaccepted agent thread {thread_id:?}: expected running, found {status_kind}"
        );
        let events = {
            let mut stmt = tx.prepare(
                "SELECT event_kind, source_turn_id
                 FROM agent_status_events
                 WHERE thread_id = ?1
                 ORDER BY sequence",
            )?;
            let rows = stmt
                .query_map([thread_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        anyhow::ensure!(
            events.as_slice() == [("turn_started".to_string(), Some(turn_id.to_string()))],
            "cannot roll back unaccepted agent thread {thread_id:?}: durable event history advanced"
        );
        tx.execute(
            "DELETE FROM agent_status_events WHERE thread_id = ?1",
            [thread_id],
        )?;
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

    pub fn cleanup_pending_reservations(&self, root_thread_id: &str) -> anyhow::Result<usize> {
        require_non_empty("root_thread_id", root_thread_id)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM agent_spawn_edges
             WHERE child_thread_id IN (
                 SELECT thread_id FROM agent_threads
                 WHERE root_thread_id = ?1 AND status_kind = 'pending_init'
             )",
            [root_thread_id],
        )?;
        let removed = tx.execute(
            "DELETE FROM agent_threads
             WHERE root_thread_id = ?1 AND status_kind = 'pending_init'",
            [root_thread_id],
        )?;
        tx.commit()?;
        Ok(removed)
    }

    /// Recover child turns that were durably Running when their process died.
    /// Recovery records a terminal interruption for the last started turn and
    /// never attempts to replay provider or tool side effects.
    pub fn recover_running_as_interrupted(&self, root_thread_id: &str) -> anyhow::Result<usize> {
        require_non_empty("root_thread_id", root_thread_id)?;
        let running = {
            let conn = self.connect()?;
            let mut stmt = conn.prepare(
                "SELECT thread_id FROM agent_threads
                 WHERE root_thread_id = ?1
                   AND parent_thread_id IS NOT NULL
                   AND status_kind = 'running'
                 ORDER BY canonical_path",
            )?;
            let rows = stmt
                .query_map([root_thread_id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };

        for thread_id in &running {
            let turn_id = self
                .status_events(thread_id)?
                .into_iter()
                .rev()
                .find_map(|event| match event.event {
                    RunnerEvent::TurnStarted { turn_id } => Some(turn_id),
                    _ => None,
                })
                .unwrap_or_else(|| format!("recovered:{thread_id}"));
            self.apply_status_event(
                thread_id,
                RunnerEvent::TurnInterrupted {
                    turn_id,
                    reason: "runtime recovered after process interruption".into(),
                },
            )?;
        }
        Ok(running.len())
    }

    pub fn validate_pending_reservation(&self, expected: &AgentThreadV2) -> anyhow::Result<()> {
        require_non_empty("thread_id", &expected.thread_id)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let durable = query_v2_thread_by_id(&tx, &expected.thread_id)?.with_context(|| {
            format!(
                "durable pending reservation {:?} is missing",
                expected.thread_id
            )
        })?;
        if durable.root_thread_id != expected.root_thread_id
            || durable.parent_thread_id != expected.parent_thread_id
            || durable.canonical_path != expected.canonical_path
            || durable.task_name != expected.task_name
            || durable.agent_type != expected.agent_type
            || durable.session_id != expected.session_id
            || durable.status != AgentStatusV2::PendingInit
        {
            bail!(
                "durable pending reservation {:?} does not match the reserved identity",
                expected.thread_id
            );
        }
        let edge = tx
            .query_row(
                "SELECT parent_thread_id, edge_state
                 FROM agent_spawn_edges WHERE child_thread_id = ?1",
                [&expected.thread_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        if edge.as_ref().is_none_or(|(parent_thread_id, state)| {
            Some(parent_thread_id.as_str()) != expected.parent_thread_id.as_deref()
                || state != "open"
        }) {
            bail!(
                "durable pending reservation {:?} requires its matching open spawn edge",
                expected.thread_id
            );
        }
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
        self.apply_status_event_with_after_read(thread_id, event, || {})
    }

    fn apply_status_event_with_after_read<F>(
        &self,
        thread_id: &str,
        event: RunnerEvent,
        after_read: F,
    ) -> anyhow::Result<AgentThreadV2>
    where
        F: FnOnce(),
    {
        require_non_empty("thread_id", thread_id)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = query_v2_thread_by_id(&tx, thread_id)?
            .with_context(|| format!("unknown agent thread {thread_id:?}"))?;
        after_read();

        if existing.status == AgentStatusV2::Shutdown
            && matches!(event, RunnerEvent::RuntimeTerminated)
        {
            tx.commit()?;
            return Ok(existing);
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
        if matches!(event, RunnerEvent::RuntimeTerminated) && existing.parent_thread_id.is_some() {
            let updated_edges = tx.execute(
                "UPDATE agent_spawn_edges
                 SET edge_state = 'closed', closed_at = COALESCE(closed_at, ?2)
                 WHERE child_thread_id = ?1",
                params![thread_id, timestamp],
            )?;
            if updated_edges != 1 {
                bail!("missing spawn edge for terminated agent thread {thread_id:?}");
            }
        }
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

    pub(crate) fn delete_pending_mailbox_message(&self, message_id: &str) -> anyhow::Result<()> {
        mailbox::delete_pending(&self.connect()?, message_id)
    }

    pub fn snapshot(&self, root_thread_id: &str) -> anyhow::Result<AgentTreeSnapshotV2> {
        self.snapshot_with_after_threads(root_thread_id, || {})
    }

    fn snapshot_with_after_threads<F>(
        &self,
        root_thread_id: &str,
        after_threads: F,
    ) -> anyhow::Result<AgentTreeSnapshotV2>
    where
        F: FnOnce(),
    {
        require_non_empty("root_thread_id", root_thread_id)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let threads = {
            let mut stmt = tx.prepare(&format!(
                "SELECT {V2_THREAD_SELECT} FROM agent_threads
                 WHERE root_thread_id = ?1
                 ORDER BY canonical_path"
            ))?;
            let rows = stmt
                .query_map([root_thread_id], v2_thread_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        after_threads();
        let activity_sequence: i64 = tx.query_row(
            "SELECT COALESCE(MAX(last_status_sequence), 0)
             FROM agent_threads WHERE root_thread_id = ?1",
            [root_thread_id],
            |row| row.get(0),
        )?;
        let snapshot = AgentTreeSnapshotV2 {
            root_thread_id: root_thread_id.to_string(),
            threads,
            activity_sequence: activity_sequence.try_into().with_context(|| {
                format!("invalid negative status activity sequence {activity_sequence}")
            })?,
        };
        tx.commit()?;
        Ok(snapshot)
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

    pub fn edge_state(&self, child_thread_id: &str) -> anyhow::Result<Option<String>> {
        require_non_empty("child_thread_id", child_thread_id)?;
        self.connect()?
            .query_row(
                "SELECT edge_state FROM agent_spawn_edges WHERE child_thread_id = ?1",
                [child_thread_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
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

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;

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

    #[test]
    fn v2_default_database_path_is_canonical() {
        assert_eq!(v2_default_db_path().file_name().unwrap(), "subagents-v2.db");
    }

    #[test]
    fn runtime_descriptor_round_trips_without_credentials_and_cascades_on_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store.ensure_root_thread("root-thread").unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .unwrap();
        let descriptor = AgentRuntimeDescriptorV2 {
            thread_id: "child".into(),
            model: Some("openai:gpt-5.6".into()),
            reasoning_effort: Some("high".into()),
        };

        store.record_runtime_descriptor(&descriptor).unwrap();
        assert_eq!(store.runtime_descriptor("child").unwrap(), Some(descriptor));

        store.rollback_pending_thread("child").unwrap();
        assert!(store.runtime_descriptor("child").unwrap().is_none());
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
    fn runtime_terminated_atomically_projects_shutdown_and_closes_edge() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .unwrap();

        let terminated = store
            .apply_status_event("child", RunnerEvent::RuntimeTerminated)
            .unwrap();

        assert_eq!(terminated.status, AgentStatusV2::Shutdown);
        assert_eq!(
            store.edge_state("child").unwrap().as_deref(),
            Some("closed")
        );
    }

    #[test]
    fn restart_recovery_preserves_turn_identity_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store.ensure_root_thread("root-thread").unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .unwrap();
        store
            .apply_status_event(
                "child",
                RunnerEvent::TurnStarted {
                    turn_id: "durable-turn".into(),
                },
            )
            .unwrap();

        assert_eq!(
            store.recover_running_as_interrupted("root-thread").unwrap(),
            1
        );
        assert_eq!(
            store.recover_running_as_interrupted("root-thread").unwrap(),
            0
        );
        assert_eq!(
            store.get_thread("child").unwrap().unwrap().status,
            AgentStatusV2::Interrupted
        );
        assert_eq!(
            store
                .status_events("child")
                .unwrap()
                .into_iter()
                .map(|event| event.event)
                .collect::<Vec<_>>(),
            vec![
                RunnerEvent::TurnStarted {
                    turn_id: "durable-turn".into(),
                },
                RunnerEvent::TurnInterrupted {
                    turn_id: "durable-turn".into(),
                    reason: "runtime recovered after process interruption".into(),
                },
            ]
        );
    }

    #[test]
    fn runtime_terminated_is_idempotent_after_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store.ensure_root_thread("root-thread").unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .unwrap();

        store
            .apply_status_event("child", RunnerEvent::RuntimeTerminated)
            .unwrap();
        store
            .apply_status_event("child", RunnerEvent::RuntimeTerminated)
            .unwrap();

        assert_eq!(store.status_events("child").unwrap().len(), 1);
        assert_eq!(
            store.edge_state("child").unwrap().as_deref(),
            Some("closed")
        );
    }

    #[test]
    fn runtime_terminated_missing_child_edge_rolls_back_status_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .unwrap();
        store
            .connect()
            .unwrap()
            .execute(
                "DELETE FROM agent_spawn_edges WHERE child_thread_id = 'child'",
                [],
            )
            .unwrap();

        let error = store
            .apply_status_event("child", RunnerEvent::RuntimeTerminated)
            .unwrap_err();

        assert!(error.to_string().contains("spawn edge"));
        assert_eq!(
            store.get_thread("child").unwrap().unwrap().status,
            AgentStatusV2::PendingInit
        );
        assert!(store.status_events("child").unwrap().is_empty());
    }

    #[test]
    fn concurrent_status_writer_does_not_invalidate_first_transaction_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .unwrap();

        let (writer_result_tx, writer_result_rx) = mpsc::channel();
        let mut writer_handle = None;
        let mut early_writer_result = None;
        let first_result = store.apply_status_event_with_after_read(
            "child",
            RunnerEvent::TurnStarted {
                turn_id: "first-turn".into(),
            },
            || {
                let writer = store.clone();
                let (writer_started_tx, writer_started_rx) = mpsc::channel();
                writer_handle = Some(thread::spawn(move || {
                    writer_started_tx.send(()).unwrap();
                    let result = writer.apply_status_event(
                        "child",
                        RunnerEvent::TurnStarted {
                            turn_id: "second-turn".into(),
                        },
                    );
                    writer_result_tx.send(result).unwrap();
                }));
                writer_started_rx
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap();
                early_writer_result = writer_result_rx
                    .recv_timeout(Duration::from_millis(100))
                    .ok();
            },
        );

        let second_result = early_writer_result.unwrap_or_else(|| {
            writer_result_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
        });
        writer_handle.unwrap().join().unwrap();
        first_result.unwrap();
        second_result.unwrap();

        let source_turn_ids = store
            .status_events("child")
            .unwrap()
            .into_iter()
            .map(|event| event.source_turn_id.unwrap())
            .collect::<Vec<_>>();
        assert_eq!(source_turn_ids, vec!["first-turn", "second-turn"]);
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
    fn rollback_unaccepted_start_requires_exact_matching_started_generation() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store
            .reserve_thread(&reservation("accepted", "/root/accepted"))
            .unwrap();
        store
            .apply_status_event(
                "accepted",
                RunnerEvent::TurnStarted {
                    turn_id: "turn-1".into(),
                },
            )
            .unwrap();
        store
            .rollback_unaccepted_started_thread("accepted", "turn-1")
            .unwrap();
        assert!(store.get_thread("accepted").unwrap().is_none());
        assert!(store.status_events("accepted").unwrap().is_empty());

        store
            .reserve_thread(&reservation("advanced", "/root/advanced"))
            .unwrap();
        store
            .apply_status_event(
                "advanced",
                RunnerEvent::TurnStarted {
                    turn_id: "turn-2".into(),
                },
            )
            .unwrap();
        store
            .apply_status_event(
                "advanced",
                RunnerEvent::TurnCompleted {
                    turn_id: "turn-2".into(),
                    last_message: "done".into(),
                },
            )
            .unwrap();
        let error = store
            .rollback_unaccepted_started_thread("advanced", "turn-2")
            .unwrap_err();
        assert!(
            error.to_string().contains("expected running")
                || error.to_string().contains("advanced")
        );
        assert!(store.get_thread("advanced").unwrap().is_some());
        assert_eq!(store.status_events("advanced").unwrap().len(), 2);
    }

    #[test]
    fn cleanup_pending_reservations_is_root_scoped_and_preserves_started_threads() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store
            .reserve_thread(&reservation("pending", "/root/pending"))
            .unwrap();
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
        let mut other_root = reservation("other-pending", "/root/pending");
        other_root.root_thread_id = "other-root".into();
        other_root.parent_thread_id = "other-root".into();
        store.reserve_thread(&other_root).unwrap();

        assert_eq!(
            store.cleanup_pending_reservations("root-thread").unwrap(),
            1
        );
        assert!(store.get_thread("pending").unwrap().is_none());
        assert_eq!(
            store.get_thread("running").unwrap().unwrap().status,
            AgentStatusV2::Running
        );
        assert!(store.get_thread("other-pending").unwrap().is_some());
        let pending_edge_count: i64 = store
            .connect()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM agent_spawn_edges WHERE child_thread_id = 'pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending_edge_count, 0);
    }

    #[test]
    fn pending_reservation_validation_requires_matching_row_and_open_edge() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        let expected = store
            .reserve_thread(&reservation("child", "/root/child"))
            .unwrap();

        store.validate_pending_reservation(&expected).unwrap();

        let mut wrong_root = expected.clone();
        wrong_root.root_thread_id = "wrong-root".into();
        assert!(store
            .validate_pending_reservation(&wrong_root)
            .unwrap_err()
            .to_string()
            .contains("does not match"));

        store
            .connect()
            .unwrap()
            .execute(
                "DELETE FROM agent_spawn_edges WHERE child_thread_id = 'child'",
                [],
            )
            .unwrap();
        assert!(store
            .validate_pending_reservation(&expected)
            .unwrap_err()
            .to_string()
            .contains("open spawn edge"));
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
    fn snapshot_projection_and_cursor_share_one_read_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .unwrap();

        let snapshot = store
            .snapshot_with_after_threads("root-thread", || {
                store
                    .apply_status_event(
                        "child",
                        RunnerEvent::TurnStarted {
                            turn_id: "concurrent-turn".into(),
                        },
                    )
                    .unwrap();
            })
            .unwrap();

        assert_eq!(snapshot.threads[0].status, AgentStatusV2::PendingInit);
        assert_eq!(snapshot.activity_sequence, 0);
        let current = store.snapshot("root-thread").unwrap();
        assert_eq!(current.threads[0].status, AgentStatusV2::Running);
        assert_eq!(current.activity_sequence, 1);
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
