use std::collections::HashSet;
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

/// Returns the durable realtime timeline in append order.
pub fn realtime_history(items: &[RolloutItem]) -> Vec<agent_protocol::RealtimeItem> {
    items
        .iter()
        .filter_map(|item| match item {
            RolloutItem::RealtimeItem(item) => Some(item.clone()),
            _ => None,
        })
        .collect()
}

/// Rebuild the effective model history after applying append-only compaction and rollback markers.
pub fn effective_response_history(items: &[RolloutItem]) -> Vec<ResponseItem> {
    let mut history = Vec::new();
    let mut pending_triggered_communications = Vec::new();
    let mut seen_triggered_communication_ids = HashSet::new();
    for item in items {
        match item {
            RolloutItem::ResponseItem(item) => {
                if let ResponseItem::AgentMessage { id: Some(id), .. } = item {
                    if let Some(index) = pending_triggered_communications
                        .iter()
                        .position(|(pending_id, _)| pending_id == id.as_str())
                    {
                        pending_triggered_communications.remove(index);
                    }
                }
                history.push(item.clone());
            }
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
                history.extend(
                    pending_triggered_communications
                        .drain(..)
                        .map(|(_, item)| item),
                );
                drop_last_n_user_turns(&mut history, event.num_turns);
            }
            RolloutItem::EventMsg(agent_protocol::EventMsg::UserInputCommitted(event)) => {
                if let Some(index) = pending_triggered_communications
                    .iter()
                    .position(|(pending_id, _)| pending_id == &event.client_message_id)
                {
                    pending_triggered_communications.remove(index);
                }
            }
            RolloutItem::InterAgentCommunication(payload) => {
                if let Ok(communication) = serde_json::from_value::<
                    agent_protocol::InterAgentCommunication,
                >(payload.clone())
                {
                    if communication.trigger_turn {
                        if let Some(id) = communication.id.as_ref() {
                            let id = id.to_string();
                            if seen_triggered_communication_ids.insert(id.clone()) {
                                pending_triggered_communications
                                    .push((id, communication.to_model_input_item()));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    history.extend(
        pending_triggered_communications
            .into_iter()
            .map(|(_, item)| item),
    );
    history
}

/// Drop instruction boundaries: ordinary user messages and structured agent messages.
pub fn drop_last_n_user_turns(history: &mut Vec<ResponseItem>, num_turns: u32) {
    if num_turns == 0 {
        return;
    }
    let positions = history
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            matches!(item, ResponseItem::Message { role, .. } if role == "user")
                .then_some(index)
                .or_else(|| matches!(item, ResponseItem::AgentMessage { .. }).then_some(index))
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
        realtime_history,
    };
    use crate::RolloutItem;

    fn message(role: &str, text: &str) -> agent_protocol::ResponseItem {
        agent_protocol::ResponseItem::Message {
            id: None,
            role: role.into(),
            content: vec![agent_protocol::ContentItem::InputText { text: text.into() }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
    }

    #[test]
    fn realtime_history_keeps_only_durable_realtime_items() {
        let realtime = agent_protocol::RealtimeItem {
            id: "rt-item-1".into(),
            realtime_session_id: "rt-1".into(),
            content: agent_protocol::RealtimeItemContent::TranscriptSegment {
                role: agent_protocol::RealtimeTranscriptRole::User,
                text: "hello".into(),
            },
        };
        let items = vec![
            RolloutItem::SessionMeta(serde_json::json!({"thread_id": "thread-1"})),
            RolloutItem::RealtimeItem(realtime.clone()),
        ];
        assert_eq!(realtime_history(&items), vec![realtime]);
    }

    #[test]
    fn realtime_history_preserves_multiple_session_boundaries() {
        let contents = [
            agent_protocol::RealtimeItemContent::RealtimeSessionStarted,
            agent_protocol::RealtimeItemContent::RealtimeSessionClosed {
                outcome: agent_protocol::RealtimeSessionOutcome::Ended,
            },
            agent_protocol::RealtimeItemContent::RealtimeSessionStarted,
        ];
        let items = contents
            .into_iter()
            .enumerate()
            .map(|(index, content)| {
                RolloutItem::RealtimeItem(agent_protocol::RealtimeItem {
                    id: format!("rt-{index}"),
                    realtime_session_id: if index < 2 { "one" } else { "two" }.into(),
                    content,
                })
            })
            .collect::<Vec<_>>();

        let restored = realtime_history(&items);
        assert_eq!(restored.len(), 3);
        assert_eq!(restored[0].realtime_session_id, "one");
        assert_eq!(restored[1].realtime_session_id, "one");
        assert_eq!(restored[2].realtime_session_id, "two");
    }

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

    #[test]
    fn effective_history_recovers_only_uncommitted_triggered_agent_messages() {
        let communication = agent_protocol::InterAgentCommunication {
            id: Some(agent_protocol::ResponseItemId::with_suffix("mail", "1")),
            author: "/root/worker".into(),
            recipient: "/root".into(),
            other_recipients: Vec::new(),
            content: "continue".into(),
            encrypted_content: None,
            internal_chat_message_metadata_passthrough: None,
            trigger_turn: true,
        };
        let marker =
            RolloutItem::InterAgentCommunication(serde_json::to_value(&communication).unwrap());
        assert_eq!(
            effective_response_history(std::slice::from_ref(&marker)),
            vec![communication.to_model_input_item()]
        );

        let consumed = vec![
            marker.clone(),
            RolloutItem::ResponseItem(message("user", "continue")),
            RolloutItem::EventMsg(agent_protocol::EventMsg::UserInputCommitted(
                agent_protocol::UserInputCommittedEvent {
                    turn_id: "turn-1".into(),
                    client_message_id: "mail_1".into(),
                },
            )),
            marker,
        ];
        assert_eq!(
            effective_response_history(&consumed),
            vec![message("user", "continue")]
        );

        let rolled_back = vec![
            RolloutItem::InterAgentCommunication(serde_json::to_value(&communication).unwrap()),
            RolloutItem::EventMsg(agent_protocol::EventMsg::ThreadRolledBack(
                agent_protocol::ThreadRolledBackEvent { num_turns: 1 },
            )),
        ];
        assert!(effective_response_history(&rolled_back).is_empty());
    }
}
