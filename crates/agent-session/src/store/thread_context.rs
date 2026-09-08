//! Thread-local working notes, separate from durable conversation facts and agent memory.

use agent_db::sqlx::{self, Row};
use anyhow::{ensure, Result};
use serde::Serialize;

use super::SessionStore;

pub const MAX_THREAD_NOTES_CHARS: usize = 8_000;

#[derive(Debug, Default, Clone, Serialize)]
pub struct ThreadContext {
    pub notes: String,
    pub revision: i64,
    pub notes_stale: bool,
    pub compaction_turn: Option<String>,
    pub compaction_reason: Option<String>,
    pub compaction_status: String,
}

impl ThreadContext {
    /// A pending/running request from a prior turn is never replayed implicitly.
    pub fn for_turn(mut self, turn_id: Option<&str>) -> Self {
        if turn_id.is_some()
            && self.compaction_turn.as_deref() != turn_id
            && matches!(self.compaction_status.as_str(), "pending" | "running")
        {
            self.compaction_status =
                "interrupted: earlier-turn request will not be replayed".into();
        }
        self
    }
}

impl SessionStore {
    pub async fn thread_context(&self, session_id: &str) -> Result<ThreadContext> {
        let row = sqlx::query("SELECT notes, revision, compaction_turn, compaction_reason, compaction_status, notes_stale FROM thread_context WHERE session_id = ?1")
            .bind(session_id).fetch_optional(&self.pool).await?;
        Ok(row
            .map(|r| ThreadContext {
                notes: r.get(0),
                revision: r.get(1),
                compaction_turn: r.get(2),
                compaction_reason: r.get(3),
                compaction_status: r.get(4),
                notes_stale: r.get(5),
            })
            .unwrap_or_else(|| ThreadContext {
                compaction_status: "idle".into(),
                ..Default::default()
            }))
    }

    /// Compare-and-swap avoids silent lost updates from simultaneous calls.
    pub async fn write_thread_notes(
        &self,
        session_id: &str,
        notes: &str,
        expected_revision: i64,
    ) -> Result<i64> {
        ensure!(
            notes.chars().count() <= MAX_THREAD_NOTES_CHARS,
            "notes exceed {MAX_THREAD_NOTES_CHARS} characters"
        );
        ensure!(expected_revision >= 0, "revision must be nonnegative");
        self.assert_session_writable(session_id).await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT OR IGNORE INTO thread_context(session_id) VALUES (?1)")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
        let row: Option<(i64,)> = sqlx::query_as("UPDATE thread_context SET notes = ?1, revision = revision + 1, notes_stale = 0 WHERE session_id = ?2 AND revision = ?3 RETURNING revision")
            .bind(notes).bind(session_id).bind(expected_revision).fetch_optional(&mut *tx).await?;
        let revision = row
            .ok_or_else(|| anyhow::anyhow!("notes revision conflict; read notes and retry"))?
            .0;
        tx.commit().await?;
        Ok(revision)
    }

    pub async fn request_context_compaction(
        &self,
        session_id: &str,
        turn_id: &str,
        reason: &str,
    ) -> Result<()> {
        ensure!(!turn_id.is_empty(), "compaction requires an active turn");
        ensure!(
            reason.chars().count() <= 1_000,
            "compaction reason exceeds 1000 characters"
        );
        self.assert_session_writable(session_id).await?;
        sqlx::query("INSERT INTO thread_context(session_id, compaction_turn, compaction_reason, compaction_status) VALUES (?1, ?2, ?3, 'pending') ON CONFLICT(session_id) DO UPDATE SET compaction_turn = excluded.compaction_turn, compaction_reason = excluded.compaction_reason, compaction_status = 'pending'")
            .bind(session_id).bind(turn_id).bind(reason).execute(&self.pool).await?;
        Ok(())
    }

    /// Keep evidence of external side effects, but never treat a pre-rollback plan as current.
    pub async fn invalidate_thread_notes(&self, session_id: &str) -> Result<()> {
        sqlx::query("UPDATE thread_context SET notes_stale = 1, revision = revision + 1, compaction_status = 'invalidated by rollback' WHERE session_id = ?1")
            .bind(session_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn take_context_compaction(&self, session_id: &str, turn_id: &str) -> Result<bool> {
        let result = sqlx::query("UPDATE thread_context SET compaction_status = 'running' WHERE session_id = ?1 AND compaction_turn = ?2 AND compaction_status = 'pending'")
            .bind(session_id).bind(turn_id).execute(&self.pool).await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn finish_context_compaction(
        &self,
        session_id: &str,
        turn_id: &str,
        status: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE thread_context SET compaction_status = ?1 WHERE session_id = ?2 AND compaction_turn = ?3 AND compaction_status = 'running'")
            .bind(status).bind(session_id).bind(turn_id).execute(&self.pool).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn simultaneous_checkpoint_writes_do_not_lose_updates() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::open(&dir.path().join("state.db"))
            .await
            .unwrap();
        store.ensure_session("one", "test").await.unwrap();
        let (a, b) = tokio::join!(
            store.write_thread_notes("one", "a", 0),
            store.write_thread_notes("one", "b", 0)
        );
        assert_ne!(a.is_ok(), b.is_ok());
        assert_eq!(store.thread_context("one").await.unwrap().revision, 1);
    }

    #[tokio::test]
    async fn notes_are_isolated_revisioned_and_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::open(&dir.path().join("state.db"))
            .await
            .unwrap();
        store.ensure_session("one", "test").await.unwrap();
        store.ensure_session("two", "test").await.unwrap();
        assert_eq!(
            store
                .write_thread_notes("one", "完成：已发送邮件；勿重复发送", 0)
                .await
                .unwrap(),
            1
        );
        assert!(store.write_thread_notes("one", "stale", 0).await.is_err());
        assert!(store
            .write_thread_notes("one", &"字".repeat(MAX_THREAD_NOTES_CHARS + 1), 1)
            .await
            .is_err());
        assert!(store.thread_context("two").await.unwrap().notes.is_empty());
        let reopened = SessionStore::open(&dir.path().join("state.db"))
            .await
            .unwrap();
        assert!(reopened
            .thread_context("one")
            .await
            .unwrap()
            .notes
            .contains("已发送"));
        reopened.invalidate_thread_notes("one").await.unwrap();
        assert!(reopened.thread_context("one").await.unwrap().notes_stale);
        assert!(reopened.write_thread_notes("one", "", 1).await.is_err());
        reopened.write_thread_notes("one", "", 2).await.unwrap();
        assert_eq!(reopened.thread_context("one").await.unwrap().revision, 3);
        assert!(!reopened.thread_context("one").await.unwrap().notes_stale);
        reopened.delete_session_permanently("one").await.unwrap();
        assert_eq!(reopened.thread_context("one").await.unwrap().revision, 0);
    }

    #[tokio::test]
    async fn compaction_requests_are_turn_scoped_and_consumed_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::open(&dir.path().join("state.db"))
            .await
            .unwrap();
        store.ensure_session("one", "test").await.unwrap();
        store
            .request_context_compaction("one", "turn-1", "near limit")
            .await
            .unwrap();
        assert!(store
            .thread_context("one")
            .await
            .unwrap()
            .for_turn(Some("turn-2"))
            .compaction_status
            .starts_with("interrupted:"));
        assert!(!store
            .take_context_compaction("one", "turn-2")
            .await
            .unwrap());
        assert!(store
            .take_context_compaction("one", "turn-1")
            .await
            .unwrap());
        assert!(!store
            .take_context_compaction("one", "turn-1")
            .await
            .unwrap());
        store
            .finish_context_compaction("one", "turn-1", "failed: unavailable")
            .await
            .unwrap();
        assert!(store
            .thread_context("one")
            .await
            .unwrap()
            .compaction_status
            .starts_with("failed"));
    }
}
