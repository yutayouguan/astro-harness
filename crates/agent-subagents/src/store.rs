use std::path::{Path, PathBuf};

use agent_db::sqlx::{self, Row};
use agent_db::{AstroDb, DbSpec, SqlitePool};
use anyhow::{bail, Context};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::mailbox::{self, MailboxMessage, NewMailboxMessage};
use crate::migration;
use crate::{
    AgentPath, AgentRuntimeDescriptorV2, AgentStatusKind, AgentStatusV2, AgentThreadV2,
    AgentTreeSnapshotV2, MailboxKind, RunnerEvent, ThreadReservation,
};

const V2_THREAD_BY_ID_SQL: &str =
    "SELECT thread_id, root_thread_id, parent_thread_id, canonical_path, task_name,
     agent_type, session_id, status_kind, status_payload, created_at, updated_at
     FROM agent_threads WHERE thread_id = ?1";
const V2_THREAD_BY_ROOT_PATH_SQL: &str =
    "SELECT thread_id, root_thread_id, parent_thread_id, canonical_path, task_name,
     agent_type, session_id, status_kind, status_payload, created_at, updated_at
     FROM agent_threads WHERE root_thread_id = ?1 AND canonical_path = ?2";
const V2_THREADS_BY_ROOT_SQL: &str =
    "SELECT thread_id, root_thread_id, parent_thread_id, canonical_path, task_name,
     agent_type, session_id, status_kind, status_payload, created_at, updated_at
     FROM agent_threads WHERE root_thread_id = ?1 ORDER BY canonical_path";
const V2_THREAD_ROOT_SELECT_SQL: &str =
    "SELECT thread_id, root_thread_id, parent_thread_id, canonical_path, task_name,
     agent_type, session_id, status_kind, status_payload, created_at, updated_at
     FROM agent_threads WHERE root_thread_id = ?1 AND canonical_path = '/root'";
const ERROR_MAX_TOKENS: usize = 900;
const APPROX_BYTES_PER_TOKEN: usize = 4;

fn v2_default_db_path() -> PathBuf {
    home::subagents_db_path(&home::default_memory_dir())
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
    pool: SqlitePool,
    path: PathBuf,
}

impl AgentGraphStore {
    pub async fn open(path: PathBuf) -> anyhow::Result<Self> {
        let pool = open_pool(&path).await?;
        migration::initialize(&pool)
            .await
            .with_context(|| format!("initialize agent graph at {}", path.display()))?;
        Ok(Self { pool, path })
    }

    pub async fn open_default_v2() -> anyhow::Result<Self> {
        home::ensure_default_workspace_dirs()?;
        Self::open(v2_default_db_path()).await
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn schema_version(&self) -> anyhow::Result<i32> {
        migration::schema_version(&self.pool).await
    }

    pub async fn ensure_root_thread(&self, root_thread_id: &str) -> anyhow::Result<AgentThreadV2> {
        require_non_empty("root_thread_id", root_thread_id)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let timestamp = now();
        let status = AgentStatusV2::Running;
        sqlx::query(
            "INSERT INTO agent_threads (
                thread_id, root_thread_id, parent_thread_id, canonical_path,
                task_name, agent_type, session_id, status_kind, status_payload,
                last_status_sequence, created_at, updated_at
             ) VALUES (?1, ?1, NULL, '/root', 'root', 'root', ?1, ?2, ?3, 0, ?4, ?4)
             ON CONFLICT DO NOTHING",
        )
        .bind(root_thread_id)
        .bind(status_kind_str(status.kind()))
        .bind(serde_json::to_string(&status)?)
        .bind(&timestamp)
        .execute(&mut *tx)
        .await?;
        let root = sqlx::query(V2_THREAD_ROOT_SELECT_SQL)
            .bind(root_thread_id)
            .fetch_optional(&mut *tx)
            .await?
            .as_ref()
            .map(v2_thread_from_row)
            .transpose()?
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
        tx.commit().await?;
        Ok(root)
    }

    pub async fn reserve_thread(
        &self,
        reservation: &ThreadReservation,
    ) -> anyhow::Result<AgentThreadV2> {
        validate_reservation(reservation)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let timestamp = now();
        let status = AgentStatusV2::PendingInit;
        sqlx::query(
            "INSERT INTO agent_threads (
                thread_id, root_thread_id, parent_thread_id, canonical_path,
                task_name, agent_type, session_id, status_kind, status_payload,
                last_status_sequence, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?10)",
        )
        .bind(&reservation.thread_id)
        .bind(&reservation.root_thread_id)
        .bind(&reservation.parent_thread_id)
        .bind(reservation.canonical_path.as_str())
        .bind(&reservation.task_name)
        .bind(&reservation.agent_type)
        .bind(&reservation.session_id)
        .bind(status_kind_str(status.kind()))
        .bind(serde_json::to_string(&status)?)
        .bind(&timestamp)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO agent_spawn_edges (
                parent_thread_id, child_thread_id, edge_state, created_at, closed_at
             ) VALUES (?1, ?2, 'open', ?3, NULL)",
        )
        .bind(&reservation.parent_thread_id)
        .bind(&reservation.thread_id)
        .bind(&timestamp)
        .execute(&mut *tx)
        .await?;
        let thread = query_v2_thread_by_id(&mut *tx, &reservation.thread_id)
            .await?
            .context("reserved agent thread is missing")?;
        tx.commit().await?;
        Ok(thread)
    }

    pub async fn record_runtime_descriptor(
        &self,
        descriptor: &AgentRuntimeDescriptorV2,
    ) -> anyhow::Result<()> {
        require_non_empty("thread_id", &descriptor.thread_id)?;
        sqlx::query(
            "INSERT INTO agent_runtime_descriptors (
                 thread_id, model, reasoning_effort
             ) VALUES (?1, ?2, ?3)",
        )
        .bind(&descriptor.thread_id)
        .bind(&descriptor.model)
        .bind(&descriptor.reasoning_effort)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn runtime_descriptor(
        &self,
        thread_id: &str,
    ) -> anyhow::Result<Option<AgentRuntimeDescriptorV2>> {
        require_non_empty("thread_id", thread_id)?;
        let row = sqlx::query(
            "SELECT thread_id, model, reasoning_effort
             FROM agent_runtime_descriptors WHERE thread_id = ?1",
        )
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(|r| AgentRuntimeDescriptorV2 {
            thread_id: r.get("thread_id"),
            model: r.get("model"),
            reasoning_effort: r.get("reasoning_effort"),
        }))
    }

    pub async fn rollback_pending_thread(&self, thread_id: &str) -> anyhow::Result<()> {
        require_non_empty("thread_id", thread_id)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row: Option<(String,)> =
            sqlx::query_as("SELECT status_kind FROM agent_threads WHERE thread_id = ?1")
                .bind(thread_id)
                .fetch_optional(&mut *tx)
                .await?;
        let status_kind = row
            .map(|(s,)| s)
            .with_context(|| format!("unknown agent thread {thread_id:?}"))?;
        if status_kind != "pending_init" {
            bail!(
                "cannot roll back agent thread {thread_id:?}: expected pending_init, found {status_kind}"
            );
        }
        sqlx::query("DELETE FROM agent_spawn_edges WHERE child_thread_id = ?1")
            .bind(thread_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM agent_threads WHERE thread_id = ?1")
            .bind(thread_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn rollback_unaccepted_started_thread(
        &self,
        thread_id: &str,
        turn_id: &str,
    ) -> anyhow::Result<()> {
        require_non_empty("thread_id", thread_id)?;
        require_non_empty("turn_id", turn_id)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row: Option<(String,)> =
            sqlx::query_as("SELECT status_kind FROM agent_threads WHERE thread_id = ?1")
                .bind(thread_id)
                .fetch_optional(&mut *tx)
                .await?;
        let status_kind = row
            .map(|(s,)| s)
            .with_context(|| format!("unknown agent thread {thread_id:?}"))?;
        anyhow::ensure!(
            status_kind == "running",
            "cannot roll back unaccepted agent thread {thread_id:?}: expected running, found {status_kind}"
        );
        let events = sqlx::query(
            "SELECT event_kind, source_turn_id
             FROM agent_status_events
             WHERE thread_id = ?1
             ORDER BY sequence",
        )
        .bind(thread_id)
        .fetch_all(&mut *tx)
        .await?;
        let event_pairs: Vec<(String, Option<String>)> = events
            .iter()
            .map(|r| (r.get("event_kind"), r.get("source_turn_id")))
            .collect();
        anyhow::ensure!(
            event_pairs.as_slice()
                == [("turn_started".to_string(), Some(turn_id.to_string()))],
            "cannot roll back unaccepted agent thread {thread_id:?}: durable event history advanced"
        );
        sqlx::query("DELETE FROM agent_status_events WHERE thread_id = ?1")
            .bind(thread_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM agent_spawn_edges WHERE child_thread_id = ?1")
            .bind(thread_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM agent_threads WHERE thread_id = ?1")
            .bind(thread_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn cleanup_pending_reservations(
        &self,
        root_thread_id: &str,
    ) -> anyhow::Result<usize> {
        require_non_empty("root_thread_id", root_thread_id)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "DELETE FROM agent_spawn_edges
             WHERE child_thread_id IN (
                 SELECT thread_id FROM agent_threads
                 WHERE root_thread_id = ?1 AND status_kind = 'pending_init'
             )",
        )
        .bind(root_thread_id)
        .execute(&mut *tx)
        .await?;
        let result = sqlx::query(
            "DELETE FROM agent_threads
             WHERE root_thread_id = ?1 AND status_kind = 'pending_init'",
        )
        .bind(root_thread_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result.rows_affected() as usize)
    }

    pub async fn recover_running_as_interrupted(
        &self,
        root_thread_id: &str,
    ) -> anyhow::Result<usize> {
        require_non_empty("root_thread_id", root_thread_id)?;
        let rows = sqlx::query(
            "SELECT thread_id FROM agent_threads
             WHERE root_thread_id = ?1
               AND parent_thread_id IS NOT NULL
               AND status_kind = 'running'
             ORDER BY canonical_path",
        )
        .bind(root_thread_id)
        .fetch_all(&self.pool)
        .await?;
        let running: Vec<String> = rows.iter().map(|r| r.get("thread_id")).collect();

        for thread_id in &running {
            let turn_id = self
                .status_events(thread_id)
                .await?
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
            )
            .await?;
        }
        Ok(running.len())
    }

    pub async fn validate_pending_reservation(
        &self,
        expected: &AgentThreadV2,
    ) -> anyhow::Result<()> {
        require_non_empty("thread_id", &expected.thread_id)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let durable = query_v2_thread_by_id(&mut *tx, &expected.thread_id)
            .await?
            .with_context(|| {
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
        let edge: Option<(String, String)> = sqlx::query_as(
            "SELECT parent_thread_id, edge_state
             FROM agent_spawn_edges WHERE child_thread_id = ?1",
        )
        .bind(&expected.thread_id)
        .fetch_optional(&mut *tx)
        .await?;
        if edge.as_ref().is_none_or(|(parent_thread_id, state)| {
            Some(parent_thread_id.as_str()) != expected.parent_thread_id.as_deref()
                || state != "open"
        }) {
            bail!(
                "durable pending reservation {:?} requires its matching open spawn edge",
                expected.thread_id
            );
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_thread(&self, thread_id: &str) -> anyhow::Result<Option<AgentThreadV2>> {
        require_non_empty("thread_id", thread_id)?;
        query_v2_thread_by_id(&self.pool, thread_id).await
    }

    pub async fn get_by_path(
        &self,
        root_thread_id: &str,
        path: &AgentPath,
    ) -> anyhow::Result<Option<AgentThreadV2>> {
        require_non_empty("root_thread_id", root_thread_id)?;
        let row = sqlx::query(V2_THREAD_BY_ROOT_PATH_SQL)
            .bind(root_thread_id)
            .bind(path.as_str())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(v2_thread_from_row).transpose()
    }

    pub async fn apply_status_event(
        &self,
        thread_id: &str,
        event: RunnerEvent,
    ) -> anyhow::Result<AgentThreadV2> {
        self.apply_status_event_with_after_read(thread_id, event, std::future::ready(()))
            .await
    }

    async fn apply_status_event_with_after_read<Fut: std::future::Future<Output = ()>>(
        &self,
        thread_id: &str,
        event: RunnerEvent,
        after_read: Fut,
    ) -> anyhow::Result<AgentThreadV2> {
        require_non_empty("thread_id", thread_id)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing = query_v2_thread_by_id(&mut *tx, thread_id)
            .await?
            .with_context(|| format!("unknown agent thread {thread_id:?}"))?;
        after_read.await;

        if existing.status == AgentStatusV2::Shutdown
            && matches!(event, RunnerEvent::RuntimeTerminated)
        {
            tx.commit().await?;
            return Ok(existing);
        }

        let status = status_for_event(&event);
        let parent_notification = final_parent_notification(&existing, &event);
        let event_kind = event_kind(&event);
        let source_turn_id = source_turn_id(&event);
        let timestamp = now();
        let result = sqlx::query(
            "INSERT INTO agent_status_events (
                thread_id, event_kind, payload, source_turn_id, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(thread_id)
        .bind(event_kind)
        .bind(serde_json::to_string(&event)?)
        .bind(source_turn_id)
        .bind(&timestamp)
        .execute(&mut *tx)
        .await?;
        let sequence = result.last_insert_rowid();
        sqlx::query(
            "UPDATE agent_threads
             SET status_kind = ?2,
                 status_payload = ?3,
                 last_status_sequence = ?4,
                 updated_at = ?5
             WHERE thread_id = ?1",
        )
        .bind(thread_id)
        .bind(status_kind_str(status.kind()))
        .bind(serde_json::to_string(&status)?)
        .bind(sequence)
        .bind(&timestamp)
        .execute(&mut *tx)
        .await?;
        if matches!(event, RunnerEvent::RuntimeTerminated) && existing.parent_thread_id.is_some() {
            let edge_result = sqlx::query(
                "UPDATE agent_spawn_edges
                 SET edge_state = 'closed', closed_at = COALESCE(closed_at, ?2)
                 WHERE child_thread_id = ?1",
            )
            .bind(thread_id)
            .bind(&timestamp)
            .execute(&mut *tx)
            .await?;
            if edge_result.rows_affected() != 1 {
                bail!("missing spawn edge for terminated agent thread {thread_id:?}");
            }
        }
        if let Some(notification) = parent_notification {
            mailbox::enqueue_in_transaction(&mut tx, &notification).await?;
        }
        let thread = query_v2_thread_by_id(&mut *tx, thread_id)
            .await?
            .context("updated agent thread is missing")?;
        tx.commit().await?;
        Ok(thread)
    }

    pub async fn status_events(&self, thread_id: &str) -> anyhow::Result<Vec<StoredStatusEvent>> {
        require_non_empty("thread_id", thread_id)?;
        let rows = sqlx::query(
            "SELECT sequence, thread_id, event_kind, payload, source_turn_id, created_at
             FROM agent_status_events
             WHERE thread_id = ?1
             ORDER BY sequence",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(stored_status_event_from_row).collect()
    }

    pub async fn enqueue(&self, message: &NewMailboxMessage) -> anyhow::Result<MailboxMessage> {
        mailbox::enqueue(&self.pool, message).await
    }

    pub async fn pending_for(
        &self,
        recipient: &str,
        after: i64,
    ) -> anyhow::Result<Vec<MailboxMessage>> {
        mailbox::pending_for(&self.pool, recipient, after).await
    }

    pub async fn mark_delivered(
        &self,
        recipient: &str,
        through_sequence: i64,
    ) -> anyhow::Result<()> {
        mailbox::mark_delivered(&self.pool, recipient, through_sequence).await
    }

    pub(crate) async fn delete_pending_mailbox_message(
        &self,
        message_id: &str,
    ) -> anyhow::Result<()> {
        mailbox::delete_pending(&self.pool, message_id).await
    }

    pub async fn snapshot(&self, root_thread_id: &str) -> anyhow::Result<AgentTreeSnapshotV2> {
        self.snapshot_with_after_threads(root_thread_id, std::future::ready(()))
            .await
    }

    async fn snapshot_with_after_threads<Fut: std::future::Future<Output = ()>>(
        &self,
        root_thread_id: &str,
        after_threads: Fut,
    ) -> anyhow::Result<AgentTreeSnapshotV2> {
        require_non_empty("root_thread_id", root_thread_id)?;
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query(V2_THREADS_BY_ROOT_SQL)
            .bind(root_thread_id)
            .fetch_all(&mut *tx)
            .await?;
        let threads = rows
            .iter()
            .map(v2_thread_from_row)
            .collect::<anyhow::Result<Vec<_>>>()?;
        after_threads.await;
        let (activity_sequence,): (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(last_status_sequence), 0)
             FROM agent_threads WHERE root_thread_id = ?1",
        )
        .bind(root_thread_id)
        .fetch_one(&mut *tx)
        .await?;
        let snapshot = AgentTreeSnapshotV2 {
            root_thread_id: root_thread_id.to_string(),
            threads,
            activity_sequence: activity_sequence.try_into().with_context(|| {
                format!("invalid negative status activity sequence {activity_sequence}")
            })?,
            root_service_tier: None,
        };
        tx.commit().await?;
        Ok(snapshot)
    }

    pub async fn close_edge(&self, child_thread_id: &str) -> anyhow::Result<()> {
        require_non_empty("child_thread_id", child_thread_id)?;
        sqlx::query(
            "UPDATE agent_spawn_edges
             SET edge_state = 'closed', closed_at = COALESCE(closed_at, ?2)
             WHERE child_thread_id = ?1",
        )
        .bind(child_thread_id)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn edge_state(&self, child_thread_id: &str) -> anyhow::Result<Option<String>> {
        require_non_empty("child_thread_id", child_thread_id)?;
        let row: Option<(String,)> =
            sqlx::query_as("SELECT edge_state FROM agent_spawn_edges WHERE child_thread_id = ?1")
                .bind(child_thread_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(s,)| s))
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

async fn query_v2_thread_by_id<'e, E>(
    executor: E,
    thread_id: &str,
) -> anyhow::Result<Option<AgentThreadV2>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let row = sqlx::query(V2_THREAD_BY_ID_SQL)
        .bind(thread_id)
        .fetch_optional(executor)
        .await?;
    row.as_ref().map(v2_thread_from_row).transpose()
}

fn v2_thread_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<AgentThreadV2> {
    let canonical_path: String = row.get("canonical_path");
    let status_kind: String = row.get("status_kind");
    let status_payload: String = row.get("status_payload");
    let status: AgentStatusV2 = serde_json::from_str(&status_payload)
        .with_context(|| format!("invalid status_payload: {status_payload}"))?;
    if status_kind != status_kind_str(status.kind()) {
        bail!(
            "status kind {status_kind:?} does not match payload kind {:?}",
            status.kind()
        );
    }
    Ok(AgentThreadV2 {
        thread_id: row.get("thread_id"),
        root_thread_id: row.get("root_thread_id"),
        parent_thread_id: row.get("parent_thread_id"),
        canonical_path: AgentPath::parse(&canonical_path)
            .map_err(|e| anyhow::anyhow!("invalid canonical_path: {e}"))?,
        task_name: row.get("task_name"),
        agent_type: row.get("agent_type"),
        session_id: row.get("session_id"),
        status,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn stored_status_event_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> anyhow::Result<StoredStatusEvent> {
    let stored_event_kind: String = row.get("event_kind");
    let payload: String = row.get("payload");
    let event: RunnerEvent = serde_json::from_str(&payload)
        .with_context(|| format!("invalid event payload: {payload}"))?;
    if stored_event_kind != event_kind(&event) {
        bail!(
            "event kind {stored_event_kind:?} does not match payload kind {:?}",
            event_kind(&event)
        );
    }
    Ok(StoredStatusEvent {
        sequence: row.get("sequence"),
        thread_id: row.get("thread_id"),
        event_kind: stored_event_kind,
        event,
        source_turn_id: row.get("source_turn_id"),
        created_at: row.get("created_at"),
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

fn final_parent_notification(
    child: &AgentThreadV2,
    event: &RunnerEvent,
) -> Option<NewMailboxMessage> {
    let parent_thread_id = child.parent_thread_id.as_ref()?;
    let parent_path = child.canonical_path.parent()?;
    let (notification_id, final_payload) = match event {
        RunnerEvent::TurnCompleted {
            turn_id,
            last_message,
        } => (turn_id.as_str(), last_message.clone()),
        RunnerEvent::TurnErrored {
            turn_id, message, ..
        } => {
            let message = truncate_terminal_error(message);
            (
                turn_id.as_str(),
                format!(
                    "Agent errored: {message}\n\nThis agent's turn failed. If you still need this agent, use the available collaboration tools to give it another task."
                ),
            )
        }
        RunnerEvent::RuntimeTerminated
            if !matches!(
                child.status,
                AgentStatusV2::Completed { .. } | AgentStatusV2::Errored { .. }
            ) =>
        {
            ("shutdown", "Agent shut down.".to_string())
        }
        RunnerEvent::TurnStarted { .. }
        | RunnerEvent::TurnInterrupted { .. }
        | RunnerEvent::RuntimeTerminated => return None,
    };
    let message_id = format!("agent-final:{}:{notification_id}", child.thread_id);
    Some(NewMailboxMessage {
        idempotency_key: message_id.clone(),
        message_id,
        sender_thread_id: child.thread_id.clone(),
        recipient_thread_id: parent_thread_id.clone(),
        kind: MailboxKind::Result,
        payload: format!(
            "Message Type: FINAL_ANSWER\nTask name: {parent_path}\nSender: {}\nPayload:\n{final_payload}",
            child.canonical_path
        ),
        trigger_turn: false,
    })
}

fn truncate_terminal_error(message: &str) -> String {
    let max_bytes = ERROR_MAX_TOKENS.saturating_mul(APPROX_BYTES_PER_TOKEN);
    if message.len() <= max_bytes {
        return message.to_string();
    }

    let left_budget = max_bytes / 2;
    let right_budget = max_bytes - left_budget;
    let tail_start_target = message.len().saturating_sub(right_budget);
    let mut prefix_end = 0;
    let mut suffix_start = message.len();
    for (index, character) in message.char_indices() {
        let character_end = index + character.len_utf8();
        if character_end <= left_budget {
            prefix_end = character_end;
        } else if index >= tail_start_target && suffix_start == message.len() {
            suffix_start = index;
        }
    }
    if suffix_start < prefix_end {
        suffix_start = prefix_end;
    }

    let removed_tokens = message
        .len()
        .saturating_sub(max_bytes)
        .div_ceil(APPROX_BYTES_PER_TOKEN);
    format!(
        "{}…{removed_tokens} tokens truncated…{}",
        &message[..prefix_end],
        &message[suffix_start..]
    )
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

async fn open_pool(path: &Path) -> anyhow::Result<SqlitePool> {
    const SPEC: DbSpec = DbSpec::new("subagents", "subagents-v2.db").with_max_connections(4);
    // File-level PRAGMAs belong to the shared once-per-path initializer, not
    // connection options: replacement connections must not contend with writers.
    let db = AstroDb::new(path.parent().unwrap_or(Path::new(".")));
    Ok(db.open_pool_at_path(&SPEC, path).await?)
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{AgentPath, AgentStatusV2, RunnerEvent, ThreadReservation};

    #[tokio::test]
    async fn pool_open_does_not_wait_for_an_active_writer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("custom-agent-graph.db");
        let store = AgentGraphStore::open(path.clone()).await.unwrap();
        store.ensure_root_thread("root-thread").await.unwrap();
        let writer = store.pool().begin_with("BEGIN IMMEDIATE").await.unwrap();

        // WAL permits readers while a writer holds its transaction. Opening
        // their connections must not replay file-level, write-locking PRAGMAs.
        let opened = tokio::time::timeout(Duration::from_secs(2), async {
            let reader = AgentGraphStore::open(path.clone()).await?;
            let roots: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_threads")
                .fetch_one(reader.pool())
                .await?;
            Ok::<_, anyhow::Error>((reader, roots))
        })
        .await;
        writer.rollback().await.unwrap();
        let (reader, roots) = opened
            .expect("opening a reader pool must not wait for the writer")
            .unwrap();
        assert_eq!(reader.path(), path);
        assert!(!dir.path().join("subagents-v2.db").exists());
        assert_eq!(roots, 1);
        reader.pool().close().await;
        store.pool().close().await;
    }

    #[tokio::test]
    async fn replacement_connection_can_read_while_a_writer_is_active() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents-v2.db");
        let store = AgentGraphStore::open(path).await.unwrap();
        store.ensure_root_thread("root-thread").await.unwrap();
        let pool = store.pool();
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(pool.acquire().await.unwrap());
        }
        // Force a physical replacement, regardless of eager/lazy pool setup.
        held.pop().unwrap().close().await.unwrap();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *held[0])
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            let mut reader = pool.acquire().await?;
            let roots: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_threads")
                .fetch_one(&mut *reader)
                .await?;
            let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
                .fetch_one(&mut *reader)
                .await?;
            Ok::<_, sqlx::Error>((roots, foreign_keys))
        })
        .await;
        sqlx::query("ROLLBACK")
            .execute(&mut *held[0])
            .await
            .unwrap();
        drop(held);
        pool.close().await;
        assert_eq!(
            result
                .expect("replacement reader must not wait for the writer")
                .unwrap(),
            (1, 1)
        );
    }

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
        let path = v2_default_db_path();
        assert_eq!(path.file_name().unwrap(), "subagents-v2.db");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "subagents");
    }

    #[tokio::test]
    async fn runtime_descriptor_round_trips_without_credentials_and_cascades_on_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store.ensure_root_thread("root-thread").await.unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();
        let descriptor = AgentRuntimeDescriptorV2 {
            thread_id: "child".into(),
            model: Some("openai:gpt-5.6".into()),
            reasoning_effort: Some("high".into()),
        };

        store.record_runtime_descriptor(&descriptor).await.unwrap();
        assert_eq!(
            store.runtime_descriptor("child").await.unwrap(),
            Some(descriptor)
        );

        store.rollback_pending_thread("child").await.unwrap();
        assert!(store.runtime_descriptor("child").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn runner_events_atomically_update_projection_and_append_status_activity() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        let thread = store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();
        assert_eq!(thread.status, AgentStatusV2::PendingInit);

        let started = RunnerEvent::TurnStarted {
            turn_id: "turn-1".into(),
        };
        let running = store
            .apply_status_event("child", started.clone())
            .await
            .unwrap();
        assert_eq!(running.status, AgentStatusV2::Running);
        let events = store.status_events("child").await.unwrap();
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
            let projected = store.apply_status_event("child", event).await.unwrap();
            assert_eq!(projected.status, expected);
        }
        assert_eq!(store.status_events("child").await.unwrap().len(), 5);
    }

    #[tokio::test]
    async fn each_terminal_turn_enqueues_one_idempotent_parent_result() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();

        store
            .apply_status_event(
                "child",
                RunnerEvent::TurnInterrupted {
                    turn_id: "turn-0".into(),
                    reason: "paused".into(),
                },
            )
            .await
            .unwrap();
        assert!(store
            .pending_for("root-thread", 0)
            .await
            .unwrap()
            .is_empty());

        store
            .apply_status_event(
                "child",
                RunnerEvent::TurnCompleted {
                    turn_id: "turn-1".into(),
                    last_message: "first answer".into(),
                },
            )
            .await
            .unwrap();
        let first = store.pending_for("root-thread", 0).await.unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].kind, MailboxKind::Result);
        assert!(first[0].payload.ends_with("Payload:\nfirst answer"));
        store
            .mark_delivered("root-thread", first[0].sequence)
            .await
            .unwrap();

        store
            .apply_status_event(
                "child",
                RunnerEvent::TurnStarted {
                    turn_id: "turn-2".into(),
                },
            )
            .await
            .unwrap();
        store
            .apply_status_event(
                "child",
                RunnerEvent::TurnErrored {
                    turn_id: "turn-2".into(),
                    message: "later failure".into(),
                },
            )
            .await
            .unwrap();
        store
            .apply_status_event(
                "child",
                RunnerEvent::TurnErrored {
                    turn_id: "turn-2".into(),
                    message: "later failure".into(),
                },
            )
            .await
            .unwrap();
        let second = store.pending_for("root-thread", 0).await.unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].message_id, "agent-final:child:turn-2");
        assert!(second[0].payload.ends_with(
            "Payload:\nAgent errored: later failure\n\nThis agent's turn failed. If you still need this agent, use the available collaboration tools to give it another task."
        ));
        store
            .apply_status_event("child", RunnerEvent::RuntimeTerminated)
            .await
            .unwrap();

        assert_eq!(store.pending_for("root-thread", 0).await.unwrap(), second);
    }

    #[tokio::test]
    async fn final_notification_failure_rolls_back_status_and_event_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();
        store
            .enqueue(&NewMailboxMessage {
                message_id: "agent-final:child:turn-1".into(),
                idempotency_key: "agent-final:child:turn-1".into(),
                sender_thread_id: "poison".into(),
                recipient_thread_id: "root-thread".into(),
                kind: MailboxKind::Message,
                payload: "conflict".into(),
                trigger_turn: false,
            })
            .await
            .unwrap();

        let error = store
            .apply_status_event(
                "child",
                RunnerEvent::TurnCompleted {
                    turn_id: "turn-1".into(),
                    last_message: "must be atomic".into(),
                },
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("immutable contents"));
        assert_eq!(
            store.get_thread("child").await.unwrap().unwrap().status,
            AgentStatusV2::PendingInit
        );
        assert!(store.status_events("child").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn final_notification_payloads_cover_error_and_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        let child = store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();

        let errored = final_parent_notification(
            &child,
            &RunnerEvent::TurnErrored {
                turn_id: "turn-error".into(),
                message: "provider unavailable".into(),
            },
        )
        .unwrap();
        assert!(errored.payload.ends_with(
            "Payload:\nAgent errored: provider unavailable\n\nThis agent's turn failed. If you still need this agent, use the available collaboration tools to give it another task."
        ));

        let shutdown = final_parent_notification(&child, &RunnerEvent::RuntimeTerminated).unwrap();
        assert!(shutdown.payload.ends_with("Payload:\nAgent shut down."));
    }

    #[tokio::test]
    async fn direct_shutdown_after_interrupt_notifies_parent_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();
        store
            .apply_status_event(
                "child",
                RunnerEvent::TurnInterrupted {
                    turn_id: "turn-1".into(),
                    reason: "cancelled".into(),
                },
            )
            .await
            .unwrap();
        store
            .apply_status_event("child", RunnerEvent::RuntimeTerminated)
            .await
            .unwrap();

        let pending = store.pending_for("root-thread", 0).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].message_id, "agent-final:child:shutdown");
        assert!(pending[0].payload.ends_with("Payload:\nAgent shut down."));
    }

    #[tokio::test]
    async fn final_error_payload_is_safely_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        let child = store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();
        let head = "HEAD-中文-🚀";
        let tail = "TAIL-DIAGNOSTIC-尾部-🚨";
        let long_error = format!("{head}{}{tail}", "🙂".repeat(1_000));
        let removed_tokens = long_error.len().saturating_sub(3_600).div_ceil(4);
        let errored = final_parent_notification(
            &child,
            &RunnerEvent::TurnErrored {
                turn_id: "turn-error".into(),
                message: long_error,
            },
        )
        .unwrap();
        let truncated = errored
            .payload
            .strip_prefix("Message Type: FINAL_ANSWER\nTask name: /root\nSender: /root/child\nPayload:\nAgent errored: ")
            .unwrap()
            .split_once("\n\nThis agent's turn failed.")
            .unwrap()
            .0;
        assert!(truncated.starts_with(head));
        assert!(truncated.ends_with(tail));
        assert!(truncated.contains(&format!("…{removed_tokens} tokens truncated…")));
    }

    #[tokio::test]
    async fn runtime_terminated_atomically_projects_shutdown_and_closes_edge() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();

        let terminated = store
            .apply_status_event("child", RunnerEvent::RuntimeTerminated)
            .await
            .unwrap();

        assert_eq!(terminated.status, AgentStatusV2::Shutdown);
        assert_eq!(
            store.edge_state("child").await.unwrap().as_deref(),
            Some("closed")
        );
    }

    #[tokio::test]
    async fn restart_recovery_preserves_turn_identity_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store.ensure_root_thread("root-thread").await.unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();
        store
            .apply_status_event(
                "child",
                RunnerEvent::TurnStarted {
                    turn_id: "durable-turn".into(),
                },
            )
            .await
            .unwrap();

        assert_eq!(
            store
                .recover_running_as_interrupted("root-thread")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .recover_running_as_interrupted("root-thread")
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            store.get_thread("child").await.unwrap().unwrap().status,
            AgentStatusV2::Interrupted
        );
        assert_eq!(
            store
                .status_events("child")
                .await
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

    #[tokio::test]
    async fn runtime_terminated_is_idempotent_after_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store.ensure_root_thread("root-thread").await.unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();

        store
            .apply_status_event("child", RunnerEvent::RuntimeTerminated)
            .await
            .unwrap();
        store
            .apply_status_event("child", RunnerEvent::RuntimeTerminated)
            .await
            .unwrap();

        assert_eq!(store.status_events("child").await.unwrap().len(), 1);
        assert_eq!(
            store.edge_state("child").await.unwrap().as_deref(),
            Some("closed")
        );
    }

    #[tokio::test]
    async fn runtime_terminated_missing_child_edge_rolls_back_status_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();
        sqlx::query("DELETE FROM agent_spawn_edges WHERE child_thread_id = 'child'")
            .execute(store.pool())
            .await
            .unwrap();

        let error = store
            .apply_status_event("child", RunnerEvent::RuntimeTerminated)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("spawn edge"));
        assert_eq!(
            store.get_thread("child").await.unwrap().unwrap().status,
            AgentStatusV2::PendingInit
        );
        assert!(store.status_events("child").await.unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_status_writers_produce_correct_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();

        let store2 = store.clone();
        let second_writer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            store2
                .apply_status_event(
                    "child",
                    RunnerEvent::TurnStarted {
                        turn_id: "second-turn".into(),
                    },
                )
                .await
        });
        let first_result = store
            .apply_status_event(
                "child",
                RunnerEvent::TurnStarted {
                    turn_id: "first-turn".into(),
                },
            )
            .await;
        first_result.unwrap();
        second_writer.await.unwrap().unwrap();

        let source_turn_ids = store
            .status_events("child")
            .await
            .unwrap()
            .into_iter()
            .map(|event| event.source_turn_id.unwrap())
            .collect::<Vec<_>>();
        assert_eq!(source_turn_ids, vec!["first-turn", "second-turn"]);
    }

    #[tokio::test]
    async fn rollback_only_removes_pending_reservation_and_spawn_edge() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("pending", "/root/pending"))
            .await
            .unwrap();
        store.rollback_pending_thread("pending").await.unwrap();
        assert!(store.get_thread("pending").await.unwrap().is_none());
        let (pending_edges,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_spawn_edges WHERE child_thread_id = 'pending'",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(pending_edges, 0);

        store
            .reserve_thread(&reservation("running", "/root/running"))
            .await
            .unwrap();
        store
            .apply_status_event(
                "running",
                RunnerEvent::TurnStarted {
                    turn_id: "turn".into(),
                },
            )
            .await
            .unwrap();
        let error = store.rollback_pending_thread("running").await.unwrap_err();
        assert!(error.to_string().contains("pending_init"));
        assert_eq!(
            store.get_thread("running").await.unwrap().unwrap().status,
            AgentStatusV2::Running
        );
    }

    #[tokio::test]
    async fn rollback_unaccepted_start_requires_exact_matching_started_generation() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("accepted", "/root/accepted"))
            .await
            .unwrap();
        store
            .apply_status_event(
                "accepted",
                RunnerEvent::TurnStarted {
                    turn_id: "turn-1".into(),
                },
            )
            .await
            .unwrap();
        store
            .rollback_unaccepted_started_thread("accepted", "turn-1")
            .await
            .unwrap();
        assert!(store.get_thread("accepted").await.unwrap().is_none());
        assert!(store.status_events("accepted").await.unwrap().is_empty());

        store
            .reserve_thread(&reservation("advanced", "/root/advanced"))
            .await
            .unwrap();
        store
            .apply_status_event(
                "advanced",
                RunnerEvent::TurnStarted {
                    turn_id: "turn-2".into(),
                },
            )
            .await
            .unwrap();
        store
            .apply_status_event(
                "advanced",
                RunnerEvent::TurnCompleted {
                    turn_id: "turn-2".into(),
                    last_message: "done".into(),
                },
            )
            .await
            .unwrap();
        let error = store
            .rollback_unaccepted_started_thread("advanced", "turn-2")
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("expected running")
                || error.to_string().contains("advanced")
        );
        assert!(store.get_thread("advanced").await.unwrap().is_some());
        assert_eq!(store.status_events("advanced").await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn cleanup_pending_reservations_is_root_scoped_and_preserves_started_threads() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("pending", "/root/pending"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("running", "/root/running"))
            .await
            .unwrap();
        store
            .apply_status_event(
                "running",
                RunnerEvent::TurnStarted {
                    turn_id: "turn".into(),
                },
            )
            .await
            .unwrap();
        let mut other_root = reservation("other-pending", "/root/pending");
        other_root.root_thread_id = "other-root".into();
        other_root.parent_thread_id = "other-root".into();
        store.reserve_thread(&other_root).await.unwrap();

        assert_eq!(
            store
                .cleanup_pending_reservations("root-thread")
                .await
                .unwrap(),
            1
        );
        assert!(store.get_thread("pending").await.unwrap().is_none());
        assert_eq!(
            store.get_thread("running").await.unwrap().unwrap().status,
            AgentStatusV2::Running
        );
        assert!(store.get_thread("other-pending").await.unwrap().is_some());
        let (pending_edge_count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_spawn_edges WHERE child_thread_id = 'pending'",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(pending_edge_count, 0);
    }

    #[tokio::test]
    async fn pending_reservation_validation_requires_matching_row_and_open_edge() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        let expected = store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();

        store.validate_pending_reservation(&expected).await.unwrap();

        let mut wrong_root = expected.clone();
        wrong_root.root_thread_id = "wrong-root".into();
        assert!(store
            .validate_pending_reservation(&wrong_root)
            .await
            .unwrap_err()
            .to_string()
            .contains("does not match"));

        sqlx::query("DELETE FROM agent_spawn_edges WHERE child_thread_id = 'child'")
            .execute(store.pool())
            .await
            .unwrap();
        assert!(store
            .validate_pending_reservation(&expected)
            .await
            .unwrap_err()
            .to_string()
            .contains("open spawn edge"));
    }

    #[tokio::test]
    async fn snapshot_sorts_canonical_paths_and_has_stable_status_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents-v2.db");
        let store = AgentGraphStore::open(path.clone()).await.unwrap();
        store
            .reserve_thread(&reservation("z-thread", "/root/z_task"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("a-thread", "/root/a_task"))
            .await
            .unwrap();
        store
            .apply_status_event(
                "z-thread",
                RunnerEvent::TurnStarted {
                    turn_id: "turn-z".into(),
                },
            )
            .await
            .unwrap();
        store
            .apply_status_event(
                "a-thread",
                RunnerEvent::TurnStarted {
                    turn_id: "turn-a".into(),
                },
            )
            .await
            .unwrap();

        let snapshot = store.snapshot("root-thread").await.unwrap();
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
                .await
                .unwrap()
                .unwrap()
                .thread_id,
            "a-thread"
        );
        drop(store);

        let reopened = AgentGraphStore::open(path).await.unwrap();
        assert_eq!(
            reopened
                .snapshot("root-thread")
                .await
                .unwrap()
                .activity_sequence,
            snapshot.activity_sequence
        );
    }

    #[tokio::test]
    async fn snapshot_projection_and_cursor_share_one_read_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("child", "/root/child"))
            .await
            .unwrap();

        let store2 = store.clone();
        let snapshot = store
            .snapshot_with_after_threads("root-thread", async move {
                store2
                    .apply_status_event(
                        "child",
                        RunnerEvent::TurnStarted {
                            turn_id: "concurrent-turn".into(),
                        },
                    )
                    .await
                    .unwrap();
            })
            .await
            .unwrap();

        assert_eq!(snapshot.threads[0].status, AgentStatusV2::PendingInit);
        assert_eq!(snapshot.activity_sequence, 0);
        let current = store.snapshot("root-thread").await.unwrap();
        assert_eq!(current.threads[0].status, AgentStatusV2::Running);
        assert_eq!(current.activity_sequence, 1);
    }

    #[tokio::test]
    async fn root_path_is_unique_and_close_edge_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db"))
            .await
            .unwrap();
        store
            .reserve_thread(&reservation("first", "/root/task"))
            .await
            .unwrap();
        assert!(store
            .reserve_thread(&reservation("second", "/root/task"))
            .await
            .is_err());
        store.close_edge("first").await.unwrap();
        store.close_edge("first").await.unwrap();
        assert_eq!(
            store.get_thread("first").await.unwrap().unwrap().status,
            AgentStatusV2::PendingInit
        );
    }
}
