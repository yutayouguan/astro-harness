use std::io;
use std::path::Path;

use tokio::io::{AsyncBufReadExt, BufReader};

use agent_protocol::ResponseItem;

use crate::{RolloutItem, RolloutLine};

#[derive(Debug, Clone, PartialEq)]
pub struct RolloutRead {
    pub items: Vec<RolloutItem>,
    pub parse_errors: usize,
}

pub async fn read_rollout(path: &Path) -> io::Result<Vec<RolloutItem>> {
    Ok(read_rollout_with_diagnostics(path).await?.items)
}

pub async fn read_rollout_with_diagnostics(path: &Path) -> io::Result<RolloutRead> {
    let file = tokio::fs::File::open(path).await?;
    let mut reader = BufReader::new(file);
    let mut items = Vec::new();
    let mut parse_errors = 0;
    let mut record = Vec::new();

    while reader.read_until(b'\n', &mut record).await? != 0 {
        if record.last() == Some(&b'\n') {
            record.pop();
        }
        if record.last() == Some(&b'\r') {
            record.pop();
        }
        if record.iter().all(u8::is_ascii_whitespace) {
            record.clear();
            continue;
        }
        match serde_json::from_slice::<RolloutLine>(&record) {
            Ok(line) => items.push(line.item),
            Err(_) => parse_errors += 1,
        }
        record.clear();
    }

    Ok(RolloutRead {
        items,
        parse_errors,
    })
}

/// Rebuild the effective model history after applying append-only compaction and rollback markers.
pub fn effective_response_history(items: &[RolloutItem]) -> Vec<ResponseItem> {
    let mut history = Vec::new();
    for item in items {
        match item {
            RolloutItem::ResponseItem(item) => history.push(item.clone()),
            RolloutItem::Compacted(payload) => {
                if let Some(replacement) = payload.get("replacement_history") {
                    if let Ok(replacement) =
                        serde_json::from_value::<Vec<ResponseItem>>(replacement.clone())
                    {
                        history = replacement;
                    }
                }
            }
            RolloutItem::EventMsg(agent_protocol::EventMsg::ThreadRolledBack(event)) => {
                drop_last_n_user_turns(&mut history, event.num_turns);
            }
            _ => {}
        }
    }
    history
}

pub fn drop_last_n_user_turns(history: &mut Vec<ResponseItem>, num_turns: u32) {
    if num_turns == 0 {
        return;
    }
    let positions = history
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            matches!(item, ResponseItem::Message { role, .. } if role == "user").then_some(index)
        })
        .collect::<Vec<_>>();
    let remove_from = positions
        .len()
        .checked_sub(usize::try_from(num_turns).unwrap_or(usize::MAX))
        .and_then(|index| positions.get(index).copied())
        .or_else(|| positions.first().copied());
    if let Some(remove_from) = remove_from {
        history.truncate(remove_from);
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{
        drop_last_n_user_turns, effective_response_history, read_rollout_with_diagnostics,
    };
    use crate::RolloutItem;

    #[tokio::test]
    async fn retains_valid_items_across_a_malformed_middle_line() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-08-31T00:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"index\":1}}\n",
                "not json\n",
                "{\"timestamp\":\"2026-08-31T00:00:01.000Z\",\"type\":\"session_meta\",\"payload\":{\"index\":2}}\n"
            ),
        )
        .unwrap();

        let rollout = read_rollout_with_diagnostics(&path).await.unwrap();
        assert_eq!(rollout.parse_errors, 1);
        assert_eq!(rollout.items.len(), 2);
        assert!(matches!(rollout.items[0], RolloutItem::SessionMeta(_)));
        assert!(matches!(rollout.items[1], RolloutItem::SessionMeta(_)));
    }

    #[tokio::test]
    async fn retains_valid_prefix_before_a_truncated_final_line() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-08-31T00:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"index\":1}}\n",
                "{\"timestamp\":\"2026-08-31T00:00:01.000Z\",\"type\":\"session_meta\",\"payload\":"
            ),
        )
        .unwrap();

        let rollout = read_rollout_with_diagnostics(&path).await.unwrap();
        assert_eq!(rollout.parse_errors, 1);
        assert_eq!(rollout.items.len(), 1);
    }

    #[tokio::test]
    async fn retains_valid_prefix_before_a_truncated_utf8_final_record() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("rollout.jsonl");
        let mut bytes = b"{\"timestamp\":\"2026-08-31T00:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"index\":1}}\n".to_vec();
        bytes.extend_from_slice(&[0xe4, 0xb8]);
        std::fs::write(&path, bytes).unwrap();

        let rollout = read_rollout_with_diagnostics(&path).await.unwrap();
        assert_eq!(rollout.parse_errors, 1);
        assert_eq!(rollout.items.len(), 1);
    }

    #[test]
    fn effective_history_applies_compaction_and_cumulative_rollbacks() {
        let message = |role: &str, text: &str| agent_protocol::ResponseItem::Message {
            id: None,
            role: role.into(),
            content: vec![agent_protocol::ContentItem::InputText { text: text.into() }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        };
        let compacted = vec![message("developer", "summary")];
        let items = vec![
            RolloutItem::ResponseItem(message("user", "old")),
            RolloutItem::Compacted(serde_json::json!({
                "replacement_history": compacted
            })),
            RolloutItem::ResponseItem(message("user", "one")),
            RolloutItem::ResponseItem(message("assistant", "answer one")),
            RolloutItem::ResponseItem(message("user", "two")),
            RolloutItem::ResponseItem(message("assistant", "answer two")),
            RolloutItem::EventMsg(agent_protocol::EventMsg::ThreadRolledBack(
                agent_protocol::ThreadRolledBackEvent { num_turns: 1 },
            )),
        ];

        assert_eq!(
            effective_response_history(&items),
            vec![
                message("developer", "summary"),
                message("user", "one"),
                message("assistant", "answer one")
            ]
        );

        let mut history = vec![message("developer", "summary"), message("user", "one")];
        drop_last_n_user_turns(&mut history, 99);
        assert_eq!(history, vec![message("developer", "summary")]);
    }
}
