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

/// Returns the latest durable thread settings snapshot in submission order.
pub fn latest_thread_settings(
    items: &[RolloutItem],
) -> Option<agent_protocol::ThreadSettingsSnapshot> {
    items.iter().rev().find_map(|item| match item {
        RolloutItem::EventMsg(agent_protocol::EventMsg::ThreadSettingsApplied(event)) => {
            Some(event.thread_settings.clone())
        }
        _ => None,
    })
}

pub fn latest_token_usage(items: &[RolloutItem]) -> Option<agent_protocol::TokenUsageRecord> {
    items.iter().rev().find_map(|item| match item {
        RolloutItem::TokenUsage(record) => Some(record.clone()),
        _ => None,
    })
}

/// 线程最近一次上下文占用快照：切换会话 / 重开窗口后恢复占用视图的事实源。
pub fn latest_context_usage(items: &[RolloutItem]) -> Option<agent_protocol::ContextUsageEvent> {
    items.iter().rev().find_map(|item| match item {
        RolloutItem::EventMsg(agent_protocol::EventMsg::ContextUsage(event)) => Some(event.clone()),
        _ => None,
    })
}

/// 单行解析：只有上下文占用事件才返回快照。
fn context_usage_from_line(line: &[u8]) -> Option<agent_protocol::ContextUsageEvent> {
    match serde_json::from_slice::<RolloutLine>(line).ok()?.item {
        RolloutItem::EventMsg(agent_protocol::EventMsg::ContextUsage(event)) => Some(event),
        _ => None,
    }
}

async fn read_tail(path: &Path, bytes: u64) -> io::Result<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let mut file = tokio::fs::File::open(path).await?;
    let len = file.metadata().await?.len();
    let window = bytes.min(len);
    file.seek(std::io::SeekFrom::Start(len - window)).await?;
    let mut buffer = vec![0u8; window as usize];
    file.read_exact(&mut buffer).await?;
    Ok(buffer)
}

/// 读取线程最近一次上下文占用快照，优先只扫文件尾部窗口。
///
/// 长线程的 rollout 可以到 MB 级，切换会话时没必要整份解析；尾部窗口里找不到
/// （例如快照很旧）时再退回整份解析，结果与 [`latest_context_usage`] 一致。
pub async fn read_last_context_usage(
    path: &Path,
) -> io::Result<Option<agent_protocol::ContextUsageEvent>> {
    const TAIL_WINDOW_BYTES: u64 = 256 * 1024;

    let len = tokio::fs::metadata(path).await?.len();
    if len > TAIL_WINDOW_BYTES {
        let tail = read_tail(path, TAIL_WINDOW_BYTES).await?;
        let mut end = tail.len();
        while end > 0 {
            let start = tail[..end]
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |index| index + 1);
            if let Some(event) = context_usage_from_line(&tail[start..end]) {
                return Ok(Some(event));
            }
            if start == 0 {
                break;
            }
            end = start - 1;
        }
    }

    Ok(latest_context_usage(&read_rollout(path).await?))
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
                if let Some(keep) = event.keep_chat_bubbles {
                    truncate_to_chat_bubbles(&mut history, keep as usize);
                } else {
                    drop_last_n_user_turns(&mut history, event.num_turns);
                }
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

/// Count durable user-turn boundaries using the same contract as relative rollback.
pub fn user_turn_count(history: &[ResponseItem]) -> usize {
    history
        .iter()
        .filter(|item| {
            matches!(item, ResponseItem::Message { role, .. } if role == "user")
                || matches!(item, ResponseItem::AgentMessage { .. })
        })
        .count()
}

/// Keep an absolute chat-bubble prefix, including tool/reasoning items belonging
/// to the final retained assistant bubble.
pub fn truncate_to_chat_bubbles(history: &mut Vec<ResponseItem>, keep: usize) {
    if keep == 0 {
        history.clear();
        return;
    }
    let starts = response_item_bubble_starts(history);
    if let Some(&remove_from) = starts.get(keep) {
        history.truncate(remove_from);
    }
}

fn response_item_bubble_starts(history: &[ResponseItem]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut assistant_bubble_open = false;
    for (index, item) in history.iter().enumerate() {
        match item {
            ResponseItem::Message { role, .. } if role == "user" => {
                starts.push(index);
                assistant_bubble_open = false;
            }
            _ if is_assistant_response_item(item) && !assistant_bubble_open => {
                starts.push(index);
                assistant_bubble_open = true;
            }
            _ => {}
        }
    }
    starts
}

fn is_assistant_response_item(item: &ResponseItem) -> bool {
    matches!(
        item,
        ResponseItem::Message { role, .. } if role == "assistant"
    ) || matches!(
        item,
        ResponseItem::FunctionCall { .. }
            | ResponseItem::CustomToolCall { .. }
            | ResponseItem::ToolSearchCall { .. }
            | ResponseItem::Reasoning { .. }
            | ResponseItem::LocalShellCall { .. }
            | ResponseItem::WebSearchCall { .. }
            | ResponseItem::ImageGenerationCall { .. }
            | ResponseItem::AgentMessage { .. }
    )
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
        drop_last_n_user_turns, effective_response_history, latest_context_usage,
        latest_thread_settings, latest_token_usage, read_last_context_usage,
        read_rollout_with_diagnostics, realtime_history,
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

    #[test]
    fn latest_thread_settings_uses_the_last_durable_snapshot() {
        let settings = |model: &str| {
            RolloutItem::EventMsg(agent_protocol::EventMsg::ThreadSettingsApplied(
                agent_protocol::ThreadSettingsAppliedEvent {
                    thread_settings: agent_protocol::ThreadSettingsSnapshot {
                        provider_id: Some("profile".into()),
                        provider: "openai".into(),
                        model: model.into(),
                        model_profile: types::ModelProfile {
                            supports_search_tool: model == "new",
                            ..types::ModelProfile::default()
                        },
                        interaction_mode: types::InteractionMode::Agent,
                        project_root: None,
                        workspace_roots: Vec::new(),
                        context_window: 128_000,
                        temperature: 0.7,
                        thinking_enabled: true,
                        reasoning_effort: "high".into(),
                        service_tier: None,
                        max_tokens: 4096,
                    },
                },
            ))
        };
        let items = vec![settings("old"), settings("new")];

        let latest = latest_thread_settings(&items).expect("latest settings");
        assert_eq!(latest.model, "new");
        assert!(latest.model_profile.supports_search_tool);
    }

    #[test]
    fn latest_token_usage_prefers_the_latest_checkpoint() {
        let record = |input_tokens, checkpoint: Option<&str>| {
            RolloutItem::TokenUsage(agent_protocol::TokenUsageRecord {
                record_id: format!("usage-{input_tokens}"),
                session_id: "thread-1".into(),
                turn_id: "turn-1".into(),
                root_turn_id: "turn-1".into(),
                response_id: None,
                latest: Default::default(),
                cumulative: agent_protocol::TokenUsageTotals {
                    input_tokens,
                    ..Default::default()
                },
                compaction_response_id: checkpoint.map(str::to_string),
            })
        };
        let items = vec![record(10, None), record(30, Some("compact-1"))];

        let restored = latest_token_usage(&items).unwrap();
        assert_eq!(restored.cumulative.input_tokens, 30);
        assert_eq!(
            restored.compaction_response_id.as_deref(),
            Some("compact-1")
        );
    }

    #[test]
    fn latest_context_usage_prefers_the_latest_snapshot() {
        let snapshot = |total_tokens: u32| {
            RolloutItem::EventMsg(agent_protocol::EventMsg::ContextUsage(
                agent_protocol::ContextUsageEvent {
                    turn_id: format!("turn-{total_tokens}"),
                    context_window: 1_000_000,
                    total_tokens,
                    estimated_total_tokens: total_tokens,
                    source: agent_protocol::ContextUsageSource::ProviderReported,
                    latest_usage: None,
                    segments: vec![agent_protocol::ContextUsageSegment {
                        id: "conversation".into(),
                        tokens: total_tokens,
                        count: None,
                        items: Vec::new(),
                    }],
                    updated_at: i64::from(total_tokens),
                    recommend_compact: false,
                },
            ))
        };

        assert!(latest_context_usage(&[RolloutItem::SessionMeta(
            serde_json::json!({"thread_id": "thread-1"})
        )])
        .is_none());

        let items = vec![snapshot(20_000), snapshot(85_541)];
        let restored = latest_context_usage(&items).expect("latest context usage");
        assert_eq!(restored.total_tokens, 85_541);
        assert_eq!(restored.segments[0].id, "conversation");
    }

    #[tokio::test]
    async fn read_last_context_usage_covers_tail_window_and_old_snapshots() {
        use std::fmt::Write as _;

        let temp = TempDir::new().unwrap();
        let snapshot_line = |total_tokens: u32| {
            serde_json::to_string(&crate::RolloutLine {
                timestamp: "2026-09-26T00:00:00Z".into(),
                ordinal: None,
                item: RolloutItem::EventMsg(agent_protocol::EventMsg::ContextUsage(
                    agent_protocol::ContextUsageEvent {
                        turn_id: format!("turn-{total_tokens}"),
                        context_window: 1_000_000,
                        total_tokens,
                        estimated_total_tokens: total_tokens,
                        source: agent_protocol::ContextUsageSource::ProviderReported,
                        latest_usage: None,
                        segments: Vec::new(),
                        updated_at: i64::from(total_tokens),
                        recommend_compact: false,
                    },
                )),
            })
            .unwrap()
        };
        let filler_line = serde_json::to_string(&crate::RolloutLine {
            timestamp: "2026-09-26T00:00:00Z".into(),
            ordinal: None,
            item: RolloutItem::SessionMeta(serde_json::json!({"padding": "x".repeat(160)})),
        })
        .unwrap();

        // 尾部窗口足够：快照在末尾，长文件也只扫窗口。
        let tail_path = temp.path().join("tail.jsonl");
        let mut deep = String::new();
        for _ in 0..2_000 {
            let _ = writeln!(deep, "{filler_line}");
        }
        let _ = writeln!(deep, "{}", snapshot_line(85_541));
        std::fs::write(&tail_path, deep).unwrap();
        assert!(
            std::fs::metadata(&tail_path).unwrap().len() > 256 * 1024,
            "fixture must exceed the tail window"
        );
        let found = read_last_context_usage(&tail_path)
            .await
            .unwrap()
            .expect("snapshot inside the tail window");
        assert_eq!(found.total_tokens, 85_541);

        // 快照早于窗口：退回整份解析，结果一致。
        let head_path = temp.path().join("head.jsonl");
        let mut shallow = String::new();
        let _ = writeln!(shallow, "{}", snapshot_line(20_000));
        for _ in 0..2_000 {
            let _ = writeln!(shallow, "{filler_line}");
        }
        std::fs::write(&head_path, shallow).unwrap();
        let old = read_last_context_usage(&head_path)
            .await
            .unwrap()
            .expect("snapshot older than the tail window");
        assert_eq!(old.total_tokens, 20_000);
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
                agent_protocol::ThreadRolledBackEvent {
                    num_turns: 1,
                    keep_chat_bubbles: None,
                },
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
    fn absolute_bubble_rollback_is_idempotent_for_edited_resubmission() {
        let call = agent_protocol::ResponseItem::FunctionCall {
            id: None,
            name: "exec_command".into(),
            namespace: None,
            arguments: "{}".into(),
            encrypted_function_args: None,
            call_id: "call-1".into(),
            internal_chat_message_metadata_passthrough: None,
        };
        let output = agent_protocol::ResponseItem::FunctionCallOutput {
            id: None,
            call_id: Some("call-1".into()),
            name: Some("exec_command".into()),
            namespace: None,
            output: agent_protocol::FunctionCallOutputPayload::from_text("ok".into()),
            internal_chat_message_metadata_passthrough: None,
        };
        let rollback = || {
            RolloutItem::EventMsg(agent_protocol::EventMsg::ThreadRolledBack(
                agent_protocol::ThreadRolledBackEvent {
                    num_turns: 0,
                    keep_chat_bubbles: Some(2),
                },
            ))
        };
        let items = vec![
            RolloutItem::ResponseItem(message("user", "one")),
            RolloutItem::ResponseItem(call.clone()),
            RolloutItem::ResponseItem(output.clone()),
            RolloutItem::ResponseItem(message("assistant", "answer one")),
            RolloutItem::ResponseItem(message("user", "old two")),
            RolloutItem::ResponseItem(message("assistant", "old answer two")),
            rollback(),
            rollback(),
            RolloutItem::ResponseItem(message("user", "new two")),
            RolloutItem::ResponseItem(message("assistant", "new answer two")),
        ];

        assert_eq!(
            effective_response_history(&items),
            vec![
                message("user", "one"),
                call,
                output,
                message("assistant", "answer one"),
                message("user", "new two"),
                message("assistant", "new answer two"),
            ]
        );
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
                agent_protocol::ThreadRolledBackEvent {
                    num_turns: 1,
                    keep_chat_bubbles: None,
                },
            )),
        ];
        assert!(effective_response_history(&rolled_back).is_empty());
    }
}
