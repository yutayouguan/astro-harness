use anyhow::{bail, Context};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, types::Type, Connection};
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

pub(crate) fn enqueue(
    conn: &mut Connection,
    message: &NewMailboxMessage,
) -> anyhow::Result<MailboxMessage> {
    validate(message)?;
    let tx = conn.transaction()?;
    let inserted = tx.execute(
        "INSERT INTO agent_mailbox (
            message_id, idempotency_key, sender_thread_id, recipient_thread_id,
            kind, payload, trigger_turn, delivery_state, created_at, delivered_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, NULL)
         ON CONFLICT(idempotency_key) DO NOTHING",
        params![
            message.message_id,
            message.idempotency_key,
            message.sender_thread_id,
            message.recipient_thread_id,
            message.kind.as_str(),
            message.payload,
            message.trigger_turn,
            now(),
        ],
    )?;

    let stored = if inserted == 1 {
        let sequence = tx.last_insert_rowid();
        load_by_sequence(&tx, sequence)?.context("inserted mailbox row is missing")?
    } else {
        load_by_idempotency_key(&tx, &message.idempotency_key)?
            .context("idempotent mailbox row is missing")?
    };
    ensure_same_immutable_contents(&stored, message)?;
    tx.commit()?;
    Ok(stored.message)
}

pub(crate) fn pending_for(
    conn: &Connection,
    recipient: &str,
    after: i64,
) -> anyhow::Result<Vec<MailboxMessage>> {
    if recipient.trim().is_empty() {
        bail!("mailbox recipient_thread_id must not be empty");
    }
    let mut stmt = conn.prepare(
        "SELECT sequence, message_id, sender_thread_id, recipient_thread_id,
                CASE WHEN idempotency_key LIKE 'main-steer:%' THEN 'steer' ELSE kind END,
                payload, trigger_turn
         FROM agent_mailbox
         WHERE recipient_thread_id = ?1
           AND delivery_state = 'pending'
           AND sequence > ?2
         ORDER BY sequence",
    )?;
    let messages = stmt
        .query_map(params![recipient, after], mailbox_message_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(messages)
}

pub(crate) fn mark_delivered(
    conn: &mut Connection,
    recipient: &str,
    through_sequence: i64,
) -> anyhow::Result<()> {
    if recipient.trim().is_empty() {
        bail!("mailbox recipient_thread_id must not be empty");
    }
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE agent_mailbox
         SET delivery_state = 'delivered', delivered_at = ?3
         WHERE recipient_thread_id = ?1
           AND sequence <= ?2
           AND delivery_state = 'pending'",
        params![recipient, through_sequence, now()],
    )?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn delete_pending(conn: &Connection, message_id: &str) -> anyhow::Result<()> {
    if message_id.trim().is_empty() {
        bail!("mailbox message_id must not be empty");
    }
    let deleted = conn.execute(
        "DELETE FROM agent_mailbox
         WHERE message_id = ?1 AND delivery_state = 'pending'",
        [message_id],
    )?;
    anyhow::ensure!(
        deleted == 1,
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

fn load_by_sequence(
    conn: &Connection,
    sequence: i64,
) -> rusqlite::Result<Option<PersistedMailboxMessage>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT sequence, message_id, idempotency_key, sender_thread_id,
                recipient_thread_id,
                CASE WHEN idempotency_key LIKE 'main-steer:%' THEN 'steer' ELSE kind END,
                payload, trigger_turn
         FROM agent_mailbox WHERE sequence = ?1",
        [sequence],
        persisted_mailbox_message_from_row,
    )
    .optional()
}

fn load_by_idempotency_key(
    conn: &Connection,
    key: &str,
) -> rusqlite::Result<Option<PersistedMailboxMessage>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT sequence, message_id, idempotency_key, sender_thread_id,
                recipient_thread_id,
                CASE WHEN idempotency_key LIKE 'main-steer:%' THEN 'steer' ELSE kind END,
                payload, trigger_turn
         FROM agent_mailbox WHERE idempotency_key = ?1",
        [key],
        persisted_mailbox_message_from_row,
    )
    .optional()
}

fn persisted_mailbox_message_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<PersistedMailboxMessage> {
    let kind = parse_kind(row.get::<_, String>(5)?, 5)?;
    Ok(PersistedMailboxMessage {
        message: MailboxMessage {
            sequence: row.get(0)?,
            message_id: row.get(1)?,
            sender_thread_id: row.get(3)?,
            recipient_thread_id: row.get(4)?,
            kind,
            payload: row.get(6)?,
            trigger_turn: row.get(7)?,
        },
        idempotency_key: row.get(2)?,
    })
}

fn mailbox_message_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MailboxMessage> {
    Ok(MailboxMessage {
        sequence: row.get(0)?,
        message_id: row.get(1)?,
        sender_thread_id: row.get(2)?,
        recipient_thread_id: row.get(3)?,
        kind: parse_kind(row.get::<_, String>(4)?, 4)?,
        payload: row.get(5)?,
        trigger_turn: row.get(6)?,
    })
}

fn parse_kind(value: String, column: usize) -> rusqlite::Result<MailboxKind> {
    MailboxKind::parse(&value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            Type::Text,
            format!("unknown mailbox kind {value:?}").into(),
        )
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

    #[test]
    fn enqueue_persists_and_idempotent_retry_returns_original_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents.db");
        let store = AgentGraphStore::open(path.clone()).unwrap();

        let first = store
            .enqueue(&message("message-1", "key-1", "recipient"))
            .unwrap();
        let retry = store
            .enqueue(&message("message-1", "key-1", "recipient"))
            .unwrap();
        assert_eq!(retry, first);
        drop(store);

        let reopened = AgentGraphStore::open(path).unwrap();
        assert_eq!(reopened.pending_for("recipient", 0).unwrap(), vec![first]);
    }

    #[test]
    fn idempotency_key_rejects_different_immutable_contents() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents.db")).unwrap();
        store
            .enqueue(&message("message-1", "same-key", "recipient"))
            .unwrap();
        let mut changed = message("message-2", "same-key", "recipient");
        changed.payload = "different".into();

        let error = store.enqueue(&changed).unwrap_err();
        assert!(error.to_string().contains("idempotency"));
        assert_eq!(store.pending_for("recipient", 0).unwrap().len(), 1);
    }

    #[test]
    fn pending_messages_are_sequence_ordered_and_delivery_is_recipient_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents.db")).unwrap();
        let first = store
            .enqueue(&message("message-1", "key-1", "recipient"))
            .unwrap();
        let second = store
            .enqueue(&message("message-2", "key-2", "recipient"))
            .unwrap();
        let other = store
            .enqueue(&message("message-3", "key-3", "other"))
            .unwrap();

        assert_eq!(
            store.pending_for("recipient", 0).unwrap(),
            vec![first.clone(), second.clone()]
        );
        assert_eq!(
            store.pending_for("recipient", first.sequence).unwrap(),
            vec![second.clone()]
        );
        store.mark_delivered("recipient", first.sequence).unwrap();
        assert_eq!(store.pending_for("recipient", 0).unwrap(), vec![second]);
        assert_eq!(store.pending_for("other", 0).unwrap(), vec![other]);
    }

    #[test]
    fn legacy_main_steer_followup_rows_project_as_steer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents.db");
        let store = AgentGraphStore::open(path.clone()).unwrap();
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute(
            "INSERT INTO agent_mailbox (
                message_id, idempotency_key, sender_thread_id, recipient_thread_id,
                kind, payload, trigger_turn, delivery_state, created_at, delivered_at
             ) VALUES ('legacy-steer', 'main-steer:legacy-steer', 'root', 'root',
                       'followup', 'resume', 1, 'pending', 'now', NULL)",
            [],
        )
        .unwrap();

        let pending = store.pending_for("root", 0).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, MailboxKind::Steer);
    }

    #[test]
    fn enqueue_rejects_empty_identifiers_before_persisting() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents.db")).unwrap();
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
            assert!(store.enqueue(&invalid).is_err());
        }
    }
}
