use agent_db::sqlx::{self, Row};
use agent_protocol::{
    ThreadAttachment, ThreadAttachmentAddOutcome, ThreadAttachmentAddResult, ThreadAttachmentPage,
};
use anyhow::{Context, Result};
use serde_json::Value;

use super::{now_epoch_secs, SessionStore};

pub const MAX_THREAD_ATTACHMENTS: i64 = 256;
pub const MAX_THREAD_ATTACHMENT_TYPE_BYTES: usize = 128;
pub const MAX_THREAD_ATTACHMENT_IDENTITY_KEY_BYTES: usize = 1024;
pub const MAX_THREAD_ATTACHMENT_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_THREAD_ATTACHMENT_PAGE_SIZE: usize = 200;

fn validate_identity(attachment_type: &str, identity_key: &str) -> Result<()> {
    anyhow::ensure!(
        !attachment_type.trim().is_empty(),
        "attachment type is required"
    );
    anyhow::ensure!(
        attachment_type.len() <= MAX_THREAD_ATTACHMENT_TYPE_BYTES,
        "attachment type exceeds {MAX_THREAD_ATTACHMENT_TYPE_BYTES} bytes"
    );
    anyhow::ensure!(
        !identity_key.trim().is_empty(),
        "attachment identity is required"
    );
    anyhow::ensure!(
        identity_key.len() <= MAX_THREAD_ATTACHMENT_IDENTITY_KEY_BYTES,
        "attachment identity exceeds {MAX_THREAD_ATTACHMENT_IDENTITY_KEY_BYTES} bytes"
    );
    Ok(())
}

fn encode_cursor(attachment: &ThreadAttachment) -> String {
    format!("{}:{}", attachment.created_at.to_bits(), attachment.id)
}

fn decode_cursor(cursor: &str) -> Result<(f64, &str)> {
    let (timestamp, id) = cursor
        .split_once(':')
        .context("invalid thread attachment cursor")?;
    let timestamp = timestamp
        .parse::<u64>()
        .context("invalid thread attachment cursor timestamp")?;
    anyhow::ensure!(!id.is_empty(), "invalid thread attachment cursor id");
    Ok((f64::from_bits(timestamp), id))
}

fn attachment_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ThreadAttachment> {
    Ok(ThreadAttachment {
        id: row.get("id"),
        thread_id: row.get("session_id"),
        attachment_type: row.get("attachment_type"),
        identity_key: row.get("identity_key"),
        payload: serde_json::from_str::<Value>(&row.get::<String, _>("payload_json"))
            .context("decode thread attachment payload")?,
        created_at: row.get("created_at"),
    })
}

impl SessionStore {
    pub async fn add_thread_attachment(
        &self,
        thread_id: &str,
        attachment_type: &str,
        identity_key: &str,
        payload: &Value,
    ) -> Result<ThreadAttachmentAddResult> {
        validate_identity(attachment_type, identity_key)?;
        let payload_json = serde_json::to_string(payload)?;
        anyhow::ensure!(
            payload_json.len() <= MAX_THREAD_ATTACHMENT_PAYLOAD_BYTES,
            "attachment payload exceeds {MAX_THREAD_ATTACHMENT_PAYLOAD_BYTES} bytes"
        );
        anyhow::ensure!(
            self.get_session(thread_id).await?.is_some(),
            "thread not found: {thread_id}"
        );

        let id = uuid::Uuid::new_v4().to_string();
        let created_at = now_epoch_secs()?;
        let inserted = sqlx::query(
            "INSERT INTO thread_attachments (
                id, session_id, attachment_type, identity_key, payload_json, created_at
             )
             SELECT ?1, ?2, ?3, ?4, ?5, ?6
             WHERE (SELECT COUNT(*) FROM thread_attachments WHERE session_id = ?2) < ?7
             ON CONFLICT(session_id, attachment_type, identity_key) DO NOTHING",
        )
        .bind(&id)
        .bind(thread_id)
        .bind(attachment_type)
        .bind(identity_key)
        .bind(&payload_json)
        .bind(created_at)
        .bind(MAX_THREAD_ATTACHMENTS)
        .execute(&self.pool)
        .await?;

        let attachment = self
            .get_thread_attachment(thread_id, attachment_type, identity_key)
            .await?
            .context("thread attachment limit reached")?;
        Ok(ThreadAttachmentAddResult {
            outcome: if inserted.rows_affected() == 1 {
                ThreadAttachmentAddOutcome::Created
            } else {
                ThreadAttachmentAddOutcome::Existing
            },
            attachment,
        })
    }

    pub async fn get_thread_attachment(
        &self,
        thread_id: &str,
        attachment_type: &str,
        identity_key: &str,
    ) -> Result<Option<ThreadAttachment>> {
        let row = sqlx::query(
            "SELECT id, session_id, attachment_type, identity_key, payload_json, created_at
             FROM thread_attachments
             WHERE session_id = ?1 AND attachment_type = ?2 AND identity_key = ?3",
        )
        .bind(thread_id)
        .bind(attachment_type)
        .bind(identity_key)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(attachment_from_row).transpose()
    }

    pub async fn list_thread_attachments(
        &self,
        thread_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ThreadAttachmentPage> {
        let limit = limit.clamp(1, MAX_THREAD_ATTACHMENT_PAGE_SIZE);
        let cursor = cursor.map(decode_cursor).transpose()?;
        let (cursor_time, cursor_id) = cursor.unwrap_or((f64::MAX, ""));
        let rows = sqlx::query(
            "SELECT id, session_id, attachment_type, identity_key, payload_json, created_at
             FROM thread_attachments
             WHERE session_id = ?1
               AND (?2 = '' OR created_at < ?3 OR (created_at = ?3 AND id < ?2))
             ORDER BY created_at DESC, id DESC
             LIMIT ?4",
        )
        .bind(thread_id)
        .bind(cursor_id)
        .bind(cursor_time)
        .bind((limit + 1) as i64)
        .fetch_all(&self.pool)
        .await?;
        let mut data = rows
            .iter()
            .map(attachment_from_row)
            .collect::<Result<Vec<_>>>()?;
        let next_cursor = (data.len() > limit).then(|| encode_cursor(&data[limit - 1]));
        data.truncate(limit);
        Ok(ThreadAttachmentPage { data, next_cursor })
    }

    pub async fn remove_thread_attachment(
        &self,
        thread_id: &str,
        attachment_type: &str,
        identity_key: &str,
    ) -> Result<Option<ThreadAttachment>> {
        validate_identity(attachment_type, identity_key)?;
        let row = sqlx::query(
            "DELETE FROM thread_attachments
             WHERE session_id = ?1 AND attachment_type = ?2 AND identity_key = ?3
             RETURNING id, session_id, attachment_type, identity_key, payload_json, created_at",
        )
        .bind(thread_id)
        .bind(attachment_type)
        .bind(identity_key)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(attachment_from_row).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn attachment_crud_is_idempotent_paginated_and_cascades() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::open(&dir.path().join("state.db"))
            .await
            .unwrap();
        store
            .create_session("thread-1", "test", None, None, None)
            .await
            .unwrap();

        let first = store
            .add_thread_attachment("thread-1", "file", "a", &serde_json::json!({"path":"a"}))
            .await
            .unwrap();
        assert_eq!(first.outcome, ThreadAttachmentAddOutcome::Created);
        let duplicate = store
            .add_thread_attachment(
                "thread-1",
                "file",
                "a",
                &serde_json::json!({"path":"changed"}),
            )
            .await
            .unwrap();
        assert_eq!(duplicate.outcome, ThreadAttachmentAddOutcome::Existing);
        assert_eq!(
            duplicate.attachment.payload,
            serde_json::json!({"path":"a"})
        );
        store
            .add_thread_attachment("thread-1", "file", "b", &serde_json::json!({"path":"b"}))
            .await
            .unwrap();

        let first_page = store
            .list_thread_attachments("thread-1", None, 1)
            .await
            .unwrap();
        assert_eq!(first_page.data.len(), 1);
        let second_page = store
            .list_thread_attachments("thread-1", first_page.next_cursor.as_deref(), 1)
            .await
            .unwrap();
        assert_eq!(second_page.data.len(), 1);
        assert_ne!(
            first_page.data[0].identity_key,
            second_page.data[0].identity_key
        );

        let removed = store
            .remove_thread_attachment("thread-1", "file", "a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(removed.identity_key, "a");
        store.delete_session_permanently("thread-1").await.unwrap();
        assert!(store
            .list_thread_attachments("thread-1", None, 10)
            .await
            .unwrap()
            .data
            .is_empty());
    }

    #[tokio::test]
    async fn attachment_rejects_oversized_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::open(&dir.path().join("state.db"))
            .await
            .unwrap();
        store
            .create_session("thread-1", "test", None, None, None)
            .await
            .unwrap();
        let error = store
            .add_thread_attachment(
                "thread-1",
                "file",
                "large",
                &serde_json::json!({"value":"x".repeat(MAX_THREAD_ATTACHMENT_PAYLOAD_BYTES)}),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("payload exceeds"));
    }
}
