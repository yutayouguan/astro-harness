//! Canonical `ResponseItem` append boundary.
//!
//! A bound rollout is authoritative and is always written before the SQLite
//! search/UI projection. Sessions without runtime I/O (for example isolated
//! review sessions) intentionally use their injected conversation store only.

use agent_protocol::ResponseItem;
use agent_rollout::{RolloutItem, RolloutRecorder};
use session::ConversationStore;

pub(super) async fn append_canonical(
    rollout: Option<&RolloutRecorder>,
    items: &[ResponseItem],
) -> anyhow::Result<()> {
    if let Some(rollout) = rollout {
        rollout
            .record(
                items
                    .iter()
                    .cloned()
                    .map(RolloutItem::ResponseItem)
                    .collect(),
            )
            .await?;
    }
    Ok(())
}

pub(super) async fn project(
    store: &dyn ConversationStore,
    session_id: &str,
    items: &[ResponseItem],
) -> anyhow::Result<Vec<i64>> {
    store.ensure_session(session_id, "tauri").await?;
    store.append_response_items(session_id, items).await
}
