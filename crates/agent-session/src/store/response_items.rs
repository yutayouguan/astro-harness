//! 原生 Responses item 持久化、分支与裁剪。

use agent_db::sqlx::{self, Row};
use agent_protocol::{ContentItem, ResponseItem};
use anyhow::{Context, Result};
use serde_json::Value;

use super::branches::{write_fork_metadata_in_transaction, write_inferred_metadata_in_transaction};
use super::{
    is_unique_constraint, now_epoch_secs, truncate_chars, BranchKind, NewResponseItem,
    SessionStore, StoredResponseItem,
};

pub(crate) fn response_item_role(item: &ResponseItem) -> Option<&str> {
    match item {
        ResponseItem::Message { role, .. } => Some(role.as_str()),
        ResponseItem::FunctionCallOutput { .. }
        | ResponseItem::CustomToolCallOutput { .. }
        | ResponseItem::ToolSearchOutput { .. } => Some("tool"),
        ResponseItem::FunctionCall { .. }
        | ResponseItem::CustomToolCall { .. }
        | ResponseItem::ToolSearchCall { .. }
        | ResponseItem::Reasoning { .. }
        | ResponseItem::LocalShellCall { .. }
        | ResponseItem::WebSearchCall { .. }
        | ResponseItem::ImageGenerationCall { .. }
        | ResponseItem::AgentMessage { .. } => Some("assistant"),
        _ => None,
    }
}

fn response_item_message_role(item: &ResponseItem) -> Option<&str> {
    match item {
        ResponseItem::Message { role, .. } => Some(role.as_str()),
        _ => None,
    }
}

pub(crate) fn response_item_text(item: &ResponseItem) -> String {
    item.text()
}

pub(crate) fn response_item_tool_name(item: &ResponseItem) -> Option<String> {
    item.qualified_tool_name()
}

pub(crate) fn response_item_is_tool_output(item: &ResponseItem) -> bool {
    item.is_tool_output()
}

pub(crate) async fn insert_response_item_row(
    executor: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    entry: NewResponseItem<'_>,
    timestamp: f64,
) -> Result<i64> {
    let item_json = serde_json::to_string(entry.item).context("serialize response item")?;
    let search_text = response_item_text(entry.item);
    let search_text_seg = types::search_text::segment_for_index(&search_text);
    let result = sqlx::query(
        "INSERT INTO response_items (
            session_id, item_json, role, search_text, search_text_seg, tool_name,
            timestamp, token_count, finish_reason
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(entry.session_id)
    .bind(item_json)
    .bind(response_item_role(entry.item))
    .bind(search_text)
    .bind(search_text_seg)
    .bind(response_item_tool_name(entry.item))
    .bind(timestamp)
    .bind(entry.token_count)
    .bind(entry.finish_reason)
    .execute(executor)
    .await?;
    Ok(result.last_insert_rowid())
}

impl SessionStore {
    pub async fn append_response_item(&self, entry: NewResponseItem<'_>) -> Result<i64> {
        self.assert_session_writable(entry.session_id).await?;
        let session_id = entry.session_id;
        let is_tool = response_item_is_tool_output(entry.item);
        let mut tx = self.pool.begin().await?;
        let id = insert_response_item_row(&mut *tx, entry, now_epoch_secs()?).await?;
        sqlx::query(
            "UPDATE sessions SET message_count = message_count + 1,
             tool_call_count = tool_call_count + ?1 WHERE id = ?2",
        )
        .bind(i64::from(is_tool))
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn append_response_items(
        &self,
        session_id: &str,
        items: &[ResponseItem],
    ) -> Result<Vec<i64>> {
        self.assert_session_writable(session_id).await?;
        let mut tx = self.pool.begin().await?;
        let mut ids = Vec::with_capacity(items.len());
        let base = now_epoch_secs()?;
        let mut tool_count = 0i64;
        for (index, item) in items.iter().enumerate() {
            tool_count += i64::from(response_item_is_tool_output(item));
            ids.push(
                insert_response_item_row(
                    &mut *tx,
                    NewResponseItem::new(session_id, item),
                    base + index as f64 * 0.000_001,
                )
                .await?,
            );
        }
        sqlx::query(
            "UPDATE sessions SET message_count = message_count + ?1,
             tool_call_count = tool_call_count + ?2 WHERE id = ?3",
        )
        .bind(i64::try_from(items.len()).context("response item count overflow")?)
        .bind(tool_count)
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ids)
    }

    pub async fn media_response_items(&self) -> Result<Vec<(String, i64, String)>> {
        let rows = sqlx::query("SELECT session_id, id, item_json FROM response_items ORDER BY id")
            .fetch_all(&self.pool)
            .await?;
        let mut out = Vec::new();
        for row in rows {
            let value: Value = serde_json::from_str(&row.get::<String, _>(2))?;
            if let Some(media) = value
                .get("internal_chat_message_metadata_passthrough")
                .and_then(|metadata| metadata.get("astro_media"))
            {
                out.push((row.get(0), row.get(1), serde_json::to_string(media)?));
            }
        }
        Ok(out)
    }

    pub async fn get_response_items(&self, session_id: &str) -> Result<Vec<StoredResponseItem>> {
        fetch_response_items(&self.pool, session_id).await
    }

    pub async fn update_response_item_compressed_content(
        &self,
        item_id: i64,
        compressed: Option<&str>,
    ) -> Result<()> {
        self.patch_response_item_metadata(
            item_id,
            "astro_compressed_output",
            compressed.map(Value::from),
        )
        .await
    }

    async fn patch_response_item_metadata(
        &self,
        item_id: i64,
        key: &str,
        value: Option<Value>,
    ) -> Result<()> {
        let raw: String = sqlx::query("SELECT item_json FROM response_items WHERE id = ?1")
            .bind(item_id)
            .fetch_one(&self.pool)
            .await?
            .get(0);
        let mut item: Value = serde_json::from_str(&raw)?;
        let object = item
            .as_object_mut()
            .context("stored response item must be an object")?;
        let metadata = object
            .entry("internal_chat_message_metadata_passthrough")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let metadata = metadata
            .as_object_mut()
            .context("response item metadata must be an object")?;
        match value {
            Some(value) => {
                metadata.insert(key.to_string(), value);
            }
            None => {
                metadata.remove(key);
            }
        }
        sqlx::query("UPDATE response_items SET item_json = ?1 WHERE id = ?2")
            .bind(serde_json::to_string(&item)?)
            .bind(item_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn fork_session(
        &self,
        source_id: &str,
        new_id: &str,
        keep_chat_bubbles: usize,
    ) -> Result<()> {
        if source_id == new_id {
            anyhow::bail!("fork_session: source and target session ids must differ");
        }
        let parent = self
            .get_session(source_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("fork_session: source session not found"))?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_target_absent(&mut tx, new_id, "fork_session").await?;
        let items = fetch_response_items(&mut *tx, source_id).await?;
        let selected = if keep_chat_bubbles == 0 {
            &items[..0]
        } else if let Some(end) = end_inclusive_for_bubbles(&items, keep_chat_bubbles) {
            &items[..=end]
        } else {
            items.as_slice()
        };
        insert_child_session(
            &mut tx,
            new_id,
            "tauri",
            parent.model.as_deref(),
            source_id,
            BranchKind::Fork,
            now_epoch_secs()?,
        )
        .await?;
        insert_response_items_into(&mut tx, new_id, selected, None).await?;
        refresh_counts(&mut *tx, new_id).await?;
        if let Some(title) = parent.title.filter(|title| !title.trim().is_empty()) {
            set_title_in_transaction(&mut tx, new_id, &format!("{title} · branch")).await?;
        }
        write_fork_metadata_in_transaction(&mut tx, source_id, new_id, selected, BranchKind::Fork)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn fork_session_recent_turns(
        &self,
        source_id: &str,
        new_id: &str,
        recent_turns: Option<usize>,
    ) -> Result<()> {
        if source_id == new_id {
            anyhow::bail!("fork_session_recent_turns: source and target session ids must differ");
        }
        let parent = self
            .get_session(source_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("source session not found: {source_id:?}"))?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_target_absent(&mut tx, new_id, "fork_session_recent_turns").await?;
        let items = fetch_response_items(&mut *tx, source_id).await?;
        let selected = match recent_turns {
            Some(0) => &items[..0],
            None => items.as_slice(),
            Some(turns) => start_inclusive_for_recent_turns(&items, turns)
                .map(|start| &items[start..])
                .unwrap_or(items.as_slice()),
        };
        insert_child_session(
            &mut tx,
            new_id,
            "tauri",
            parent.model.as_deref(),
            source_id,
            BranchKind::Agent,
            now_epoch_secs()?,
        )
        .await?;
        insert_response_items_into(&mut tx, new_id, selected, None).await?;
        refresh_counts(&mut *tx, new_id).await?;
        if let Some(title) = parent.title.filter(|title| !title.trim().is_empty()) {
            set_title_in_transaction(&mut tx, new_id, &format!("{title} · branch")).await?;
        }
        write_inferred_metadata_in_transaction(
            &mut tx,
            source_id,
            new_id,
            &items,
            selected,
            BranchKind::Agent,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn truncate_session_to_bubbles(
        &self,
        session_id: &str,
        keep_chat_bubbles: usize,
    ) -> Result<()> {
        if self.get_session(session_id).await?.is_none() {
            anyhow::bail!("truncate_session_to_bubbles: session not found");
        }
        let items = self.get_response_items(session_id).await?;
        let keep_through = end_inclusive_for_bubbles(&items, keep_chat_bubbles);
        let mut tx = self.pool.begin().await?;
        match keep_through {
            Some(end) if end + 1 < items.len() => {
                sqlx::query("DELETE FROM response_items WHERE session_id = ?1 AND id > ?2")
                    .bind(session_id)
                    .bind(items[end].id)
                    .execute(&mut *tx)
                    .await?;
            }
            None => {
                sqlx::query("DELETE FROM response_items WHERE session_id = ?1")
                    .bind(session_id)
                    .execute(&mut *tx)
                    .await?;
            }
            _ => {}
        }
        refresh_counts(&mut *tx, session_id).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Replace the SQLite projection with an exact canonical history.
    /// Matching prefixes keep their row ids and metadata; divergent projections
    /// are rebuilt transactionally.
    pub async fn replace_response_items(
        &self,
        session_id: &str,
        replacement: &[ResponseItem],
    ) -> Result<()> {
        self.ensure_session(session_id, "runtime").await?;
        let mut tx = self.pool.begin().await?;
        let current = fetch_response_items(&mut *tx, session_id).await?;
        let prefix_matches = current.len() >= replacement.len()
            && current
                .iter()
                .zip(replacement)
                .all(|(stored, expected)| &stored.item == expected);

        if prefix_matches {
            match replacement.last() {
                Some(_) if current.len() > replacement.len() => {
                    let first_removed_id = current[replacement.len()].id;
                    sqlx::query("DELETE FROM response_items WHERE session_id = ?1 AND id >= ?2")
                        .bind(session_id)
                        .bind(first_removed_id)
                        .execute(&mut *tx)
                        .await?;
                }
                None => {
                    sqlx::query("DELETE FROM response_items WHERE session_id = ?1")
                        .bind(session_id)
                        .execute(&mut *tx)
                        .await?;
                }
                _ => {}
            }
        } else {
            sqlx::query("DELETE FROM response_items WHERE session_id = ?1")
                .bind(session_id)
                .execute(&mut *tx)
                .await?;
            let now = now_epoch_secs()?;
            for (index, item) in replacement.iter().enumerate() {
                insert_response_item_row(
                    &mut *tx,
                    NewResponseItem::new(session_id, item),
                    now + index as f64 * 0.000_001,
                )
                .await?;
            }
        }
        refresh_counts(&mut *tx, session_id).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn compact_and_split(
        &self,
        old_id: &str,
        new_id: &str,
        summary_text: &str,
        keep_tail_bubbles: usize,
    ) -> Result<()> {
        self.compact_and_split_if_unchanged(old_id, new_id, summary_text, keep_tail_bubbles, None)
            .await
    }

    pub async fn compact_and_split_if_unchanged(
        &self,
        old_id: &str,
        new_id: &str,
        summary_text: &str,
        keep_tail_bubbles: usize,
        expected_last_item_id: Option<i64>,
    ) -> Result<()> {
        if old_id == new_id {
            anyhow::bail!("compact_and_split: session ids must differ");
        }
        let parent = self
            .get_session(old_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("compact_and_split: source session not found"))?;
        if parent.ended_at.is_some() {
            anyhow::bail!("compact_and_split: source session already ended");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_target_absent(&mut tx, new_id, "compact_and_split").await?;
        let items = fetch_response_items(&mut *tx, old_id).await?;
        if expected_last_item_id.is_some()
            && items.last().map(|item| item.id) != expected_last_item_id
        {
            anyhow::bail!("compact_and_split: source session changed while summarizing");
        }
        let now = now_epoch_secs()?;
        let changed = sqlx::query(
            "UPDATE sessions SET ended_at = ?1, end_reason = 'compacted'
             WHERE id = ?2 AND ended_at IS NULL",
        )
        .bind(now)
        .bind(old_id)
        .execute(&mut *tx)
        .await?;
        if changed.rows_affected() != 1 {
            anyhow::bail!("compact_and_split: source session already ended");
        }
        insert_child_session(
            &mut tx,
            new_id,
            &parent.source,
            parent.model.as_deref(),
            old_id,
            BranchKind::Fork,
            now,
        )
        .await?;
        let summary = ResponseItem::Message {
            id: None,
            role: "user".into(),
            content: vec![ContentItem::InputText {
                text: summary_text.into(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        };
        insert_response_item_row(&mut *tx, NewResponseItem::new(new_id, &summary), now).await?;
        if keep_tail_bubbles > 0 {
            if let Some(start) = start_inclusive_for_tail_bubbles(&items, keep_tail_bubbles) {
                insert_response_items_into(&mut tx, new_id, &items[start..], Some(now)).await?;
            }
        }
        refresh_counts(&mut *tx, new_id).await?;
        if let Some(title) = parent.title.filter(|title| !title.trim().is_empty()) {
            let continued = format!("{title} · continued");
            set_title_in_transaction(&mut tx, new_id, &truncate_chars(&continued, 80)).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_response_items_as_json(&self, session_id: &str) -> Result<Vec<Value>> {
        self.get_response_items(session_id)
            .await?
            .into_iter()
            .map(|entry| serde_json::to_value(entry.item).map_err(Into::into))
            .collect()
    }
}

async fn fetch_response_items<'e>(
    executor: impl sqlx::Executor<'e, Database = sqlx::Sqlite>,
    session_id: &str,
) -> Result<Vec<StoredResponseItem>> {
    let rows = sqlx::query(
        "SELECT id, session_id, item_json, timestamp, token_count, finish_reason
         FROM response_items WHERE session_id = ?1 ORDER BY timestamp, id",
    )
    .bind(session_id)
    .fetch_all(executor)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(StoredResponseItem {
                id: row.get(0),
                session_id: row.get(1),
                item: serde_json::from_str(&row.get::<String, _>(2))
                    .context("deserialize stored response item")?,
                timestamp: row.get(3),
                token_count: row.get(4),
                finish_reason: row.get(5),
            })
        })
        .collect()
}

async fn ensure_target_absent(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    session_id: &str,
    operation: &str,
) -> Result<()> {
    let exists: bool = sqlx::query("SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)")
        .bind(session_id)
        .fetch_one(&mut **tx)
        .await?
        .get(0);
    if exists {
        anyhow::bail!("{operation}: target session already exists");
    }
    Ok(())
}

async fn insert_child_session(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    source: &str,
    model: Option<&str>,
    parent_session_id: &str,
    kind: BranchKind,
    started_at: f64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO sessions (
            id, source, model, parent_session_id, started_at,
            branch_kind, branch_inherited_turn_count, branch_created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?5)",
    )
    .bind(id)
    .bind(source)
    .bind(model)
    .bind(parent_session_id)
    .bind(started_at)
    .bind(kind.as_str())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_response_items_into(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    new_id: &str,
    items: &[StoredResponseItem],
    base_timestamp: Option<f64>,
) -> Result<()> {
    for (index, item) in items.iter().enumerate() {
        insert_response_item_row(
            &mut **tx,
            NewResponseItem {
                session_id: new_id,
                item: &item.item,
                token_count: item.token_count,
                finish_reason: item.finish_reason.as_deref(),
            },
            base_timestamp
                .map(|base| base + (index + 1) as f64 * 0.000_001)
                .unwrap_or(item.timestamp),
        )
        .await?;
    }
    Ok(())
}

async fn set_title_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    session_id: &str,
    title: &str,
) -> Result<()> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let result = sqlx::query("UPDATE sessions SET title = ?1 WHERE id = ?2")
        .bind(trimmed)
        .bind(session_id)
        .execute(&mut **tx)
        .await;
    match result {
        Ok(_) => Ok(()),
        Err(error) if is_unique_constraint(&error) => {
            let suffix: String = session_id.chars().take(8).collect();
            let unique = format!("{} · {}", truncate_chars(trimmed, 60), suffix);
            sqlx::query("UPDATE sessions SET title = ?1 WHERE id = ?2")
                .bind(unique)
                .bind(session_id)
                .execute(&mut **tx)
                .await?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

async fn refresh_counts(
    executor: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    session_id: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE sessions SET
         message_count = (SELECT COUNT(*) FROM response_items WHERE session_id = ?1),
         tool_call_count = (SELECT COUNT(*) FROM response_items
             WHERE session_id = ?1 AND json_extract(item_json, '$.type') IN
             ('function_call_output', 'custom_tool_call_output', 'tool_search_output'))
         WHERE id = ?1",
    )
    .bind(session_id)
    .execute(executor)
    .await?;
    Ok(())
}

fn start_inclusive_for_tail_bubbles(items: &[StoredResponseItem], keep: usize) -> Option<usize> {
    if keep == 0 || items.is_empty() {
        return None;
    }
    let starts = response_item_bubble_starts(items);
    (!starts.is_empty()).then(|| starts[starts.len().saturating_sub(keep)])
}

fn start_inclusive_for_recent_turns(items: &[StoredResponseItem], keep: usize) -> Option<usize> {
    if keep == 0 || items.is_empty() {
        return None;
    }
    let starts = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            (response_item_message_role(&item.item) == Some("user")).then_some(index)
        })
        .collect::<Vec<_>>();
    (!starts.is_empty()).then(|| starts[starts.len().saturating_sub(keep)])
}

fn end_inclusive_for_bubbles(items: &[StoredResponseItem], keep: usize) -> Option<usize> {
    if keep == 0 || items.is_empty() {
        return None;
    }
    let starts = response_item_bubble_starts(items);
    if starts.is_empty() || keep >= starts.len() {
        return Some(items.len() - 1);
    }
    Some(starts[keep] - 1)
}

fn response_item_bubble_starts(items: &[StoredResponseItem]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut assistant_bubble_open = false;
    for (index, item) in items.iter().enumerate() {
        match response_item_message_role(&item.item) {
            Some("user") => {
                starts.push(index);
                assistant_bubble_open = false;
            }
            _ if response_item_role(&item.item) == Some("assistant") && !assistant_bubble_open => {
                starts.push(index);
                assistant_bubble_open = true;
            }
            _ => {}
        }
    }
    starts
}
