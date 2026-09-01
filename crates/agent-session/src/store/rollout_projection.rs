//! 从持久 rollout 确定性重建 SQLite 的原生 Responses item 索引。

use agent_db::sqlx;
use anyhow::{Context, Result};

use super::response_items::{insert_response_item_row, response_item_is_tool_output};
use super::{now_epoch_secs, NewResponseItem, SessionStore};

/// 用 rollout 中的原生 `ResponseItem` 替换单个会话的 SQLite 索引。
///
/// schema v22 不迁移旧 `messages` 行；rollout 是唯一事实源，索引可随时重建。
pub async fn rebuild_response_items_from_rollout(
    store: &SessionStore,
    session_id: &str,
    items: &[agent_rollout::RolloutItem],
) -> Result<()> {
    anyhow::ensure!(!session_id.trim().is_empty(), "rollout session id is empty");
    let response_items = agent_rollout::effective_response_history(items);
    let item_count = i64::try_from(response_items.len()).context("response item count overflow")?;
    let tool_call_count = i64::try_from(
        response_items
            .iter()
            .filter(|item| response_item_is_tool_output(item))
            .count(),
    )
    .context("tool output count overflow")?;
    let now = now_epoch_secs()?;

    let mut tx = store.pool.begin().await?;
    sqlx::query(
        "INSERT INTO sessions (id, source, started_at)
         VALUES (?1, 'rollout', ?2)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(session_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM response_items WHERE session_id = ?1")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    for (index, item) in response_items.iter().enumerate() {
        insert_response_item_row(
            &mut *tx,
            NewResponseItem::new(session_id, item),
            now + index as f64 * 0.000_001,
        )
        .await?;
    }
    sqlx::query("UPDATE sessions SET message_count = ?1, tool_call_count = ?2 WHERE id = ?3")
        .bind(item_count)
        .bind(tool_call_count)
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
