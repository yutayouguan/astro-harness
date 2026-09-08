//! Read-only, append-stable evidence references. References use physical lines, not SQLite IDs.

use crate::{RolloutItem, RolloutLine};
use serde::Serialize;
use std::{io, path::Path};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

#[derive(Default)]
pub struct HistoryQuery<'a> {
    pub after_line: usize,
    pub query: Option<&'a str>,
    pub item_ref: Option<&'a str>,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub item_ref: String,
    pub role: String,
    pub text: String,
    pub total_chars: usize,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct HistoryPage {
    pub entries: Vec<HistoryEntry>,
    pub next_after_line: Option<usize>,
    pub parse_errors: usize,
}

pub fn history_reference(path: &Path, line: usize) -> String {
    format!(
        "{}#{line}",
        path.file_name().unwrap_or_default().to_string_lossy()
    )
}

fn entry_text(item: &RolloutItem) -> Option<(String, String)> {
    match item {
        RolloutItem::ResponseItem(item) => {
            // Reasoning/encrypted payloads are not a working-notes archive.
            if matches!(item, agent_protocol::ResponseItem::Reasoning { .. }) {
                return None;
            }
            let role = item.role().unwrap_or(if item.is_tool_output() {
                "tool"
            } else {
                "assistant"
            });
            Some((role.into(), item.text()))
        }
        RolloutItem::Compacted(value) => Some(("compaction".into(), value.to_string())),
        _ => None,
    }
}

/// Pages contain bounded text; malformed lines retain their physical position.
/// No item is claimed to be complete unless next_offset is None.
pub async fn read_history_page(path: &Path, query: HistoryQuery<'_>) -> io::Result<HistoryPage> {
    let target_line = if let Some(reference) = query.item_ref {
        let (file, line) = reference
            .rsplit_once('#')
            .ok_or_else(|| io::Error::other("invalid history reference"))?;
        if Some(file) != path.file_name().and_then(|n| n.to_str()) {
            return Err(io::Error::other(
                "reference does not belong to this thread rollout",
            ));
        }
        Some(
            line.parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| io::Error::other("invalid history line"))?,
        )
    } else {
        None
    };
    let limit = query.limit.clamp(1, 20);
    let needle = query.query.map(str::to_lowercase);
    let mut reader = BufReader::new(tokio::fs::File::open(path).await?);
    let mut bytes = Vec::new();
    let mut line_no = 0;
    let mut scanned = 0;
    let mut page = HistoryPage {
        entries: vec![],
        next_after_line: None,
        parse_errors: 0,
    };
    loop {
        bytes.clear();
        if (&mut reader)
            .take(16 * 1024 * 1024)
            .read_until(b'\n', &mut bytes)
            .await?
            == 0
        {
            break;
        }
        if bytes.len() == 16 * 1024 * 1024 {
            return Err(io::Error::other(
                "history record exceeds 16 MiB safety limit",
            ));
        }
        line_no += 1;
        if line_no <= query.after_line && target_line.is_none() {
            continue;
        }
        if target_line.is_some_and(|n| n != line_no) {
            continue;
        }
        // A writer may currently be appending this last record. Retry it next time.
        if bytes.last() != Some(&b'\n') {
            page.next_after_line = Some(line_no - 1);
            break;
        }
        scanned += 1;
        match serde_json::from_slice::<RolloutLine>(&bytes) {
            Ok(line) => {
                if let Some((role, text)) = entry_text(&line.item) {
                    if needle
                        .as_ref()
                        .is_none_or(|q| text.to_lowercase().contains(q))
                    {
                        let total_chars = text.chars().count();
                        let offset = if target_line.is_some() {
                            query.offset
                        } else {
                            0
                        };
                        if offset > total_chars {
                            return Err(io::Error::other("offset exceeds item length"));
                        }
                        let chunk: String = text
                            .chars()
                            .skip(offset)
                            .take(if target_line.is_some() { 8_000 } else { 400 })
                            .collect();
                        let end = offset + chunk.chars().count();
                        page.entries.push(HistoryEntry {
                            item_ref: history_reference(path, line_no),
                            role,
                            text: chunk,
                            total_chars,
                            next_offset: (end < total_chars).then_some(end),
                        });
                    }
                }
            }
            Err(_) => page.parse_errors += 1,
        }
        if target_line.is_some() {
            break;
        }
        if page.entries.len() >= limit || scanned >= 2_048 {
            page.next_after_line = Some(line_no);
            break;
        }
    }
    if target_line.is_some() && page.entries.is_empty() {
        return Err(io::Error::other(
            "history item unavailable, incomplete, or not a readable conversation item",
        ));
    }
    Ok(page)
}

type EvidenceKey = (Option<String>, Option<String>, Option<String>, String);

fn evidence_key(item: &agent_protocol::ResponseItem) -> EvidenceKey {
    (
        item.role().map(str::to_owned),
        item.qualified_tool_name(),
        item.call_id().map(str::to_owned),
        item.text(),
    )
}

/// Stream the archive, retaining only references requested by the active history.
/// Ambiguous identical records are deliberately not assigned a guessed reference.
pub async fn response_history_references(
    path: &Path,
    wanted: &[agent_protocol::ResponseItem],
) -> io::Result<Vec<Option<String>>> {
    let mut matches = std::collections::HashMap::new();
    for item in wanted {
        matches.entry(evidence_key(item)).or_insert((0usize, None));
    }
    let mut reader = BufReader::new(tokio::fs::File::open(path).await?);
    let mut bytes = Vec::new();
    let mut line_no = 0;
    loop {
        bytes.clear();
        if (&mut reader)
            .take(16 * 1024 * 1024)
            .read_until(b'\n', &mut bytes)
            .await?
            == 0
        {
            break;
        }
        if bytes.len() == 16 * 1024 * 1024 {
            return Err(io::Error::other(
                "history record exceeds 16 MiB safety limit",
            ));
        }
        line_no += 1;
        if bytes.last() != Some(&b'\n') {
            break;
        }
        if let Ok(RolloutLine {
            item: RolloutItem::ResponseItem(item),
            ..
        }) = serde_json::from_slice(&bytes)
        {
            if let Some((count, reference)) = matches.get_mut(&evidence_key(&item)) {
                *count += 1;
                *reference = (*count == 1).then(|| history_reference(path, line_no));
            }
        }
    }
    Ok(wanted
        .iter()
        .map(|item| {
            matches
                .get(&evidence_key(item))
                .and_then(|(_, reference)| reference.clone())
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RolloutRecorder;

    #[tokio::test]
    async fn references_only_resolve_unique_requested_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thread.jsonl");
        let writer = RolloutRecorder::open(path.clone()).await.unwrap();
        let repeated = agent_protocol::ResponseItem::user_text("same request");
        let unique = agent_protocol::ResponseItem::user_text("unique request");
        writer
            .record(vec![
                RolloutItem::ResponseItem(repeated.clone()),
                RolloutItem::ResponseItem(unique.clone()),
                RolloutItem::ResponseItem(repeated.clone()),
            ])
            .await
            .unwrap();
        writer.flush().await.unwrap();
        let references = response_history_references(
            &path,
            &[
                repeated,
                unique,
                agent_protocol::ResponseItem::user_text("absent"),
            ],
        )
        .await
        .unwrap();
        assert_eq!(references, vec![None, Some("thread.jsonl#2".into()), None]);
    }

    #[tokio::test]
    async fn malformed_lines_do_not_shift_references_and_partial_tail_is_retried() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thread.jsonl");
        let line = serde_json::to_string(&RolloutLine {
            timestamp: "test".into(),
            ordinal: None,
            item: RolloutItem::ResponseItem(agent_protocol::ResponseItem::user_text("evidence")),
        })
        .unwrap();
        tokio::fs::write(&path, format!("broken\n{line}\n{{\"type\":"))
            .await
            .unwrap();
        let page = read_history_page(
            &path,
            HistoryQuery {
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(page.parse_errors, 1);
        assert_eq!(page.entries[0].item_ref, "thread.jsonl#2");
        assert_eq!(page.next_after_line, Some(2));
        let direct = read_history_page(
            &path,
            HistoryQuery {
                item_ref: Some("thread.jsonl#2"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(direct.entries[0].text, "evidence");
    }

    #[tokio::test]
    async fn history_references_survive_append_and_support_unicode_paging() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thread.jsonl");
        let writer = RolloutRecorder::open(path.clone()).await.unwrap();
        writer
            .record(vec![RolloutItem::ResponseItem(
                agent_protocol::ResponseItem::user_text("证据".repeat(5_000)),
            )])
            .await
            .unwrap();
        writer.flush().await.unwrap();
        let page = read_history_page(
            &path,
            HistoryQuery {
                limit: 1,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let reference = page.entries[0].item_ref.clone();
        writer
            .record(vec![RolloutItem::Compacted(
                serde_json::json!({"reason":"test"}),
            )])
            .await
            .unwrap();
        writer.flush().await.unwrap();
        let full = read_history_page(
            &path,
            HistoryQuery {
                item_ref: Some(&reference),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(full.entries[0].next_offset, Some(8_000));
        let tail = read_history_page(
            &path,
            HistoryQuery {
                item_ref: Some(&reference),
                offset: 8_000,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(tail.entries[0].text.chars().count(), 2_000);
        assert!(tail.entries[0].next_offset.is_none());
        assert!(read_history_page(
            &path,
            HistoryQuery {
                item_ref: Some("other.jsonl#1"),
                ..Default::default()
            }
        )
        .await
        .is_err());
        let next = read_history_page(
            &path,
            HistoryQuery {
                after_line: 1,
                query: Some("test"),
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(next.entries[0].role, "compaction");
    }
}
