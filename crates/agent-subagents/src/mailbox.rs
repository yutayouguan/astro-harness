use anyhow::{bail, Context};
use chrono::{SecondsFormat, Utc};
use agent_db::sqlx::{self, Row};
use agent_db::SqlitePool;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailboxKind {
    Message,
    Followup,
    Steer,
    Result,
    Status,
}

#[derive(Debug, Clone)]
pub struct NewMailboxMessage {
    pub message_id: String,
    pub idempotency_key: String,
    pub sender_thread_id: String,
    pub recipient_thread_id: String,
    pub kind: MailboxKind,
    pub payload: String,
    pub trigger_turn: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailboxMessage {
    pub sequence: i64,
    pub message_id: String,
    pub sender_thread_id: String,
    pub recipient_thread_id: String,
    pub kind: MailboxKind,
    pub payload: String,
    pub trigger_turn: bool,
}

#[derive(Debug)]
struct PersistedMailboxMessage {
    message: MailboxMessage,
    idempotency_key: String,
}

pub(crate) async fn enqueue(
    pool: &SqlitePool,
    message: &NewMailboxMessage,
) -> anyhow::Result<MailboxMessage> {
    let mut tx = pool.begin().await?;
    let stored = enqueue_in_transaction(&mut tx, message).await?;
    tx.commit().await?;
    Ok(stored)
}

pub(crate) async fn enqueue_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    message: &NewMailboxMessage,
) -> anyhow::Result<MailboxMessage> {
    validate(message)?;
    let result = sqlx::query(
        "INSERT INTO agent_mailbox (
            message_id, idempotency_key, sender_thread_id, recipient_thread_id,
            kind, payload, trigger_turn, delivery_state, created_at, delivered_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, NULL)
         ON CONFLICT(idempotency_key) DO NOTHING",
    )
    .bind(&message.message_id)
    .bind(&message.idempotency_key)
    .bind(&message.sender_thread_id)
    .bind(&message.recipient_thread_id)
    .bind(message.kind.as_str())
    .bind(&message.payload)
    .bind(message.trigger_turn)
    .bind(now())
    .execute(&mut **tx)
    .await?;

    let stored = if result.rows_affected() == 1 {
        let sequence = result.last_insert_rowid();
        load_by_sequence(&mut **tx, sequence)
            .await?
            .context("inserted mailbox row is missing")?
    } else {
        load_by_idempotency_key(&mut **tx, &message.idempotency_key)
            .await?
            .context("idempotent mailbox row is missing")?
    };
    ensure_same_immutable_contents(&stored, message)?;
    Ok(stored.message)
}

pub(crate) async fn pending_for(
    pool: &SqlitePool,
    recipient: &str,
    after: i64,
) -> anyhow::Result<Vec<MailboxMessage>> {
    if recipient.trim().is_empty() {
        bail!("mailbox recipient_thread_id must not be empty");
    }
    let rows = sqlx::query(
        "SELECT sequence, message_id, sender_thread_id, recipient_thread_id,
                CASE WHEN idempotency_key LIKE 'main-steer:%' THEN 'steer' ELSE kind END,
                payload, trigger_turn
         FROM agent_mailbox
         WHERE recipient_thread_id = ?1
           AND delivery_state = 'pending'
           AND sequence > ?2
         ORDER BY sequence",
    )
    .bind(recipient)
    .bind(after)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(mailbox_message_from_row)
        .collect()
}

pub(crate) async fn mark_delivered(
    pool: &SqlitePool,
    recipient: &str,
    through_sequence: i64,
) -> anyhow::Result<()> {
    if recipient.trim().is_empty() {
        bail!("mailbox recipient_thread_id must not be empty");
    }
    sqlx::query(
        "UPDATE agent_mailbox
         SET delivery_state = 'delivered', delivered_at = ?3
         WHERE recipient_thread_id = ?1
           AND sequence <= ?2
           AND delivery_state = 'pending'",
    )
    .bind(recipient)
    .bind(through_sequence)
    .bind(now())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn delete_pending(pool: &SqlitePool, message_id: &str) -> anyhow::Result<()> {
    if message_id.trim().is_empty() {
        bail!("mailbox message_id must not be empty");
    }
    let result = sqlx::query(
        "DELETE FROM agent_mailbox
         WHERE message_id = ?1 AND delivery_state = 'pending'",
    )
    .bind(message_id)
    .execute(pool)
    .await?;
    anyhow::ensure!(
        result.rows_affected() == 1,
        "pending mailbox message {message_id:?} could not be rolled back"
    );
    Ok(())
}

impl MailboxKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Followup => "followup",
            Self::Steer => "steer",
            Self::Result => "result",
            Self::Status => "status",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "message" => Some(Self::Message),
            "followup" => Some(Self::Followup),
            "steer" => Some(Self::Steer),
            "result" => Some(Self::Result),
            "status" => Some(Self::Status),
            _ => None,
        }
    }
}

fn validate(message: &NewMailboxMessage) -> anyhow::Result<()> {
    for (label, value) in [
        ("message_id", message.message_id.as_str()),
        ("idempotency_key", message.idempotency_key.as_str()),
        ("sender_thread_id", message.sender_thread_id.as_str()),
        ("recipient_thread_id", message.recipient_thread_id.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("mailbox {label} must not be empty");
        }
    }
    Ok(())
}

fn ensure_same_immutable_contents(
    stored: &PersistedMailboxMessage,
    requested: &NewMailboxMessage,
) -> anyhow::Result<()> {
    if stored.idempotency_key != requested.idempotency_key
        || stored.message.message_id != requested.message_id
        || stored.message.sender_thread_id != requested.sender_thread_id
        || stored.message.recipient_thread_id != requested.recipient_thread_id
        || stored.message.kind != requested.kind
        || stored.message.payload != requested.payload
        || stored.message.trigger_turn != requested.trigger_turn
    {
        bail!(
            "mailbox idempotency key {:?} was already used with different immutable contents",
            requested.idempotency_key
        );
    }
    Ok(())
}

async fn load_by_sequence(
    conn: &mut sqlx::SqliteConnection,
    sequence: i64,
) -> anyhow::Result<Option<PersistedMailboxMessage>> {
    let row = sqlx::query(
        "SELECT sequence, message_id, idempotency_key, sender_thread_id,
                recipient_thread_id,
                CASE WHEN idempotency_key LIKE 'main-steer:%' THEN 'steer' ELSE kind END,
                payload, trigger_turn
         FROM agent_mailbox WHERE sequence = ?1",
    )
    .bind(sequence)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(persisted_mailbox_message_from_row).transpose()
}

async fn load_by_idempotency_key(
    conn: &mut sqlx::SqliteConnection,
    key: &str,
) -> anyhow::Result<Option<PersistedMailboxMessage>> {
    let row = sqlx::query(
        "SELECT sequence, message_id, idempotency_key, sender_thread_id,
                recipient_thread_id,
                CASE WHEN idempotency_key LIKE 'main-steer:%' THEN 'steer' ELSE kind END,
                payload, trigger_turn
         FROM agent_mailbox WHERE idempotency_key = ?1",
    )
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(persisted_mailbox_message_from_row).transpose()
}

fn persisted_mailbox_message_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> anyhow::Result<PersistedMailboxMessage> {
    let kind_str: String = row.get(5);
    let kind = MailboxKind::parse(&kind_str)
        .with_context(|| format!("unknown mailbox kind {kind_str:?}"))?;
    Ok(PersistedMailboxMessage {
        message: MailboxMessage {
            sequence: row.get(0),
            message_id: row.get(1),
            sender_thread_id: row.get(3),
            recipient_thread_id: row.get(4),
            kind,
            payload: row.get(6),
            trigger_turn: row.get(7),
        },
        idempotency_key: row.get(2),
    })
}

fn mailbox_message_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<MailboxMessage> {
    let kind_str: String = row.get(4);
    let kind = MailboxKind::parse(&kind_str)
        .with_context(|| format!("unknown mailbox kind {kind_str:?}"))?;
    Ok(MailboxMessage {
        sequence: row.get(0),
        message_id: row.get(1),
        sender_thread_id: row.get(2),
        recipient_thread_id: row.get(3),
        kind,
        payload: row.get(5),
        trigger_turn: row.get(6),
    })
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use crate::{AgentGraphStore, MailboxKind, NewMailboxMessage};

    fn message(id: &str, key: &str, recipient: &str) -> NewMailboxMessage {
        NewMailboxMessage {
            message_id: id.into(),
            idempotency_key: key.into(),
            sender_thread_id: "sender".into(),
            recipient_thread_id: recipient.into(),
            kind: MailboxKind::Message,
            payload: format!("payload-{id}"),
            trigger_turn: false,
        }
    }

    #[tokio::test]
    async fn enqueue_persists_and_idempotent_retry_returns_original_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents.db");
        let store = AgentGraphStore::open(path.clone()).await.unwrap();

        let first = store
            .enqueue(&message("message-1", "key-1", "recipient"))
            .await
            .unwrap();
        let retry = store
            .enqueue(&message("message-1", "key-1", "recipient"))
            .await
            .unwrap();
        assert_eq!(retry, first);
        drop(store);

        let reopened = AgentGraphStore::open(path).await.unwrap();
        assert_eq!(
            reopened.pending_for("recipient", 0).await.unwrap(),
            vec![first]
        );
    }

    #[tokio::test]
    async fn idempotency_key_rejects_different_immutable_contents() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents.db"))
            .await
            .unwrap();
        store
            .enqueue(&message("message-1", "same-key", "recipient"))
            .await
            .unwrap();
        let mut changed = message("message-2", "same-key", "recipient");
        changed.payload = "different".into();

        let error = store.enqueue(&changed).await.unwrap_err();
        assert!(error.to_string().contains("immutable contents"));
        assert_eq!(store.pending_for("recipient", 0).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn pending_messages_are_sequence_ordered_and_delivery_is_recipient_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents.db"))
            .await
            .unwrap();
        let first = store
            .enqueue(&message("message-1", "key-1", "recipient"))
            .await
            .unwrap();
        let second = store
            .enqueue(&message("message-2", "key-2", "recipient"))
            .await
            .unwrap();
        let other = store
            .enqueue(&message("message-3", "key-3", "other"))
            .await
            .unwrap();

        assert_eq!(
            store.pending_for("recipient", 0).await.unwrap(),
            vec![first.clone(), second.clone()]
        );
        assert_eq!(
            store
                .pending_for("recipient", first.sequence)
                .await
                .unwrap(),
            vec![second.clone()]
        );
        store
            .mark_delivered("recipient", first.sequence)
            .await
            .unwrap();
        assert_eq!(
            store.pending_for("recipient", 0).await.unwrap(),
            vec![second]
        );
        assert_eq!(store.pending_for("other", 0).await.unwrap(), vec![other]);
    }

    #[tokio::test]
    async fn legacy_main_steer_followup_rows_project_as_steer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents.db");
        let store = AgentGraphStore::open(path.clone()).await.unwrap();
        let pool = store.pool();
        agent_db::sqlx::query(
            "INSERT INTO agent_mailbox (
                message_id, idempotency_key, sender_thread_id, recipient_thread_id,
                kind, payload, trigger_turn, delivery_state, created_at, delivered_at
             ) VALUES ('legacy-steer', 'main-steer:legacy-steer', 'root', 'root',
                       'followup', 'resume', 1, 'pending', 'now', NULL)",
        )
        .execute(pool)
        .await
        .unwrap();

        let pending = store.pending_for("root", 0).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, MailboxKind::Steer);
    }

    #[tokio::test]
    async fn enqueue_rejects_empty_identifiers_before_persisting() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents.db"))
            .await
            .unwrap();
        for invalid in [
            NewMailboxMessage {
                message_id: String::new(),
                ..message("message", "key-a", "recipient")
            },
            NewMailboxMessage {
                idempotency_key: String::new(),
                ..message("message", "key-b", "recipient")
            },
            NewMailboxMessage {
                sender_thread_id: String::new(),
                ..message("message", "key-c", "recipient")
            },
            NewMailboxMessage {
                recipient_thread_id: String::new(),
                ..message("message", "key-d", "recipient")
            },
        ] {
            assert!(store.enqueue(&invalid).await.is_err());
        }
    }
}
