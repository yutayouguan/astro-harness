//! 子 Agent 邮箱投递——在安全采样边界消费持久化消息。

use std::sync::Arc;

use session::ConversationStore;

use crate::runtime::Session;
use crate::tasks::TurnInput;

pub(crate) const MAILBOX_FINISH_PREFIX: &str = "agent-mailbox-through:";
const MAIN_STEER_PREFIX: &str = "astro-main-steer-v1:";

#[derive(serde::Deserialize, serde::Serialize)]
struct DurableSteerInput {
    content: String,
    image_data_urls: Vec<String>,
    #[serde(default)]
    client_message_id: Option<String>,
    #[serde(default)]
    inject_context: Option<String>,
}

/// 邮箱批量消费结果：已投递数、steer ID 列表、是否延迟。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MailboxDrainOutcome {
    pub(crate) delivered: usize,
    pub(crate) delivered_steer_ids: Vec<String>,
    pub(crate) delivered_client_message_ids: Vec<String>,
    pub(crate) deferred: bool,
}

#[cfg(test)]
pub(crate) fn encode_main_steer_input(input: &TurnInput) -> anyhow::Result<String> {
    encode_main_steer_input_with_context(input, None)
}

/// 将主线程 steer 输入编码为可持久化的邮箱消息格式。
pub(crate) fn encode_main_steer_input_with_context(
    input: &TurnInput,
    inject_context: Option<String>,
) -> anyhow::Result<String> {
    Ok(format!(
        "{MAIN_STEER_PREFIX}{}",
        serde_json::to_string(&DurableSteerInput {
            content: input.content.clone(),
            image_data_urls: input.image_data_urls.clone(),
            client_message_id: input.client_message_id.clone(),
            inject_context,
        })?
    ))
}

/// 仅在采样边界消费持久邮箱，写入 SessionStore 和内存历史后才确认。
pub(crate) async fn drain_mailbox_at_safe_boundary(
    session: &Session,
) -> anyhow::Result<MailboxDrainOutcome> {
    let mut total = MailboxDrainOutcome::default();
    loop {
        let batch = drain_mailbox_batch_at_safe_boundary(session).await?;
        total.delivered += batch.delivered;
        total.delivered_steer_ids.extend(batch.delivered_steer_ids);
        total
            .delivered_client_message_ids
            .extend(batch.delivered_client_message_ids);
        if batch.deferred {
            total.deferred = true;
            return Ok(total);
        }
        if batch.delivered == 0 {
            return Ok(total);
        }
        let pending = session
            .services
            .agent_control
            .drain_mailbox(&session.services.agent_path)
            .await?;
        if pending.is_empty() {
            return Ok(total);
        }
        // 恢复的旧标记可能仅覆盖了本代际准入快照的前缀。
        // 继续在同一安全边界处消费，确保后续序列号既不会
        // 滞留也不会重复。
    }
}

async fn drain_mailbox_batch_at_safe_boundary(
    session: &Session,
) -> anyhow::Result<MailboxDrainOutcome> {
    if session.cancel_signal().is_cancelled() {
        anyhow::bail!("agent turn interrupted before mailbox drain");
    }
    let control = Arc::clone(&session.services.agent_control);
    let path = session.services.agent_path.clone();
    let messages = control.drain_mailbox(&path).await?;
    if messages.is_empty() {
        return Ok(MailboxDrainOutcome::default());
    }
    if session.cancel_signal().is_cancelled() {
        anyhow::bail!("agent turn interrupted during mailbox drain");
    }
    let first_sequence = messages.first().expect("non-empty mailbox").sequence;
    let last_sequence = messages.last().expect("non-empty mailbox").sequence;
    let persisted_through = session
        .services
        .sessions
        .get_response_items(session.session_id())
        .await?
        .iter()
        .filter_map(|stored| {
            stored
                .item
                .metadata()?
                .get("astro_memory_marker")?
                .as_str()?
                .strip_prefix(MAILBOX_FINISH_PREFIX)?
                .parse::<i64>()
                .ok()
        })
        .max();
    let runtime_history = session.clone_history().await;
    let in_memory_through = runtime_history
        .iter()
        .filter_map(|item| {
            item.metadata()?
                .get("astro_memory_marker")?
                .as_str()?
                .strip_prefix(MAILBOX_FINISH_PREFIX)?
                .parse::<i64>()
                .ok()
        })
        .max();
    let through_sequence = persisted_through
        .into_iter()
        .chain(in_memory_through)
        .filter(|value| *value >= first_sequence && *value <= last_sequence)
        .max()
        .unwrap_or(last_sequence);
    let delivered = messages
        .iter()
        .take_while(|message| message.sequence <= through_sequence)
        .count();
    let marker = format!("{MAILBOX_FINISH_PREFIX}{through_sequence}");
    let durable = session.ensure_durable_turn_input_marker(&marker).await?;
    let in_memory = runtime_history.iter().any(|item| {
        item.metadata()
            .and_then(|metadata| metadata.get("astro_memory_marker"))
            .and_then(serde_json::Value::as_str)
            == Some(marker.as_str())
    });
    if !in_memory
        && runtime_history
            .last()
            .is_some_and(|item| item.role() == Some("user"))
    {
        return Ok(MailboxDrainOutcome {
            deferred: true,
            ..MailboxDrainOutcome::default()
        });
    }
    let mut contents = Vec::with_capacity(delivered);
    let mut image_data_urls = Vec::new();
    let mut delivered_steer_ids = Vec::new();
    let mut delivered_client_message_ids = Vec::new();
    let mut inject_contexts = Vec::new();
    for message in messages.into_iter().take(delivered) {
        if message.sender_thread_id == message.recipient_thread_id {
            if let Some(encoded) = message.payload.strip_prefix(MAIN_STEER_PREFIX) {
                let input: DurableSteerInput = serde_json::from_str(encoded)?;
                contents.push(input.content);
                image_data_urls.extend(input.image_data_urls);
                delivered_client_message_ids.extend(input.client_message_id);
                inject_contexts.extend(input.inject_context);
                delivered_steer_ids.push(message.message_id);
                continue;
            }
        }
        contents.push(message.payload);
    }
    let content = contents.join("\n\n");
    let input = TurnInput {
        content,
        image_data_urls,
        client_message_id: None,
    };
    if !durable {
        session.persist_turn_input(&input, Some(&marker)).await?;
    }
    if session.cancel_signal().is_cancelled() {
        anyhow::bail!("agent turn interrupted after durable mailbox history write");
    }
    if !in_memory {
        session
            .record_turn_input_in_memory(&input, Some(&marker))
            .await;
    }
    session.queue_inject_contexts(inject_contexts).await;
    if session.cancel_signal().is_cancelled() {
        anyhow::bail!("agent turn interrupted after in-memory mailbox history write");
    }
    control.ack_mailbox(&path, through_sequence).await?;
    Ok(MailboxDrainOutcome {
        delivered,
        delivered_steer_ids,
        delivered_client_message_ids,
        deferred: false,
    })
}
#[cfg(test)]
mod tests {
    use crate::runtime::Config;

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn mailbox_is_acked_only_after_safe_boundary_history_acceptance() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db"))
            .await
            .unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .await
        .unwrap();
        let reservation = root
            .reserve_spawn(&subagents::AgentPath::root(), "worker")
            .await
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().await.unwrap();
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "follow up safely".into(),
            },
            true,
        )
        .await
        .unwrap();
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "and keep tool rows".into(),
            },
            true,
        )
        .await
        .unwrap();

        let memory_dir = temp.path().join("memory");
        let interrupted = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir.clone()),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path.clone(),
        )
        .await
        .unwrap();
        interrupted.cancel_signal().cancel();
        assert!(drain_mailbox_at_safe_boundary(&{ interrupted })
            .await
            .is_err());
        assert_eq!(
            graph.pending_for(&thread.thread_id, 0).await.unwrap().len(),
            2
        );

        let retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path,
        )
        .await
        .unwrap();
        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            2
        );
        assert!(graph
            .pending_for(&thread.thread_id, 0)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            0
        );
        let history = retry.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role(), Some("user"));
        assert_eq!(
            history[0].content_str(),
            "follow up safely\n\nand keep tool rows"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mailbox_atomic_history_write_survives_cancel_and_restart() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db"))
            .await
            .unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .await
        .unwrap();
        let reservation = root
            .reserve_spawn(&subagents::AgentPath::root(), "worker")
            .await
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().await.unwrap();
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "persisted before ack".into(),
            },
            true,
        )
        .await
        .unwrap();
        let sequence = graph.pending_for(&thread.thread_id, 0).await.unwrap()[0].sequence;
        let marker = format!("{MAILBOX_FINISH_PREFIX}{sequence}");
        let memory_dir = temp.path().join("memory");
        let sessions = session::SessionStore::open_sessions_dir(&memory_dir.join("data"))
            .await
            .unwrap();
        sessions
            .create_session(&thread.session_id, "tauri", None, None, None)
            .await
            .unwrap();
        let retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir.clone()),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path.clone(),
        )
        .await
        .unwrap();
        let cancel = retry.cancel_signal();
        retry.set_turn_input_after_db_write_hook(Some(Arc::new(move || {
            cancel.cancel();
            anyhow::bail!("failpoint after durable mailbox write")
        })));

        assert!(drain_mailbox_at_safe_boundary(&retry).await.is_err());
        let stored = sessions
            .get_response_items(&thread.session_id)
            .await
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].finish_reason.is_none());
        assert_eq!(
            stored[0]
                .item
                .metadata()
                .and_then(|value| value.get("astro_memory_marker"))
                .and_then(serde_json::Value::as_str),
            Some(marker.as_str())
        );
        assert!(retry.clone_history().await.is_empty());
        assert_eq!(
            graph.pending_for(&thread.thread_id, 0).await.unwrap().len(),
            1
        );
        drop(retry);

        let retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path,
        )
        .await
        .unwrap();
        assert_eq!(retry.clone_history().await.len(), 1);

        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            1
        );
        assert!(graph
            .pending_for(&thread.thread_id, 0)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            sessions
                .get_response_items(&thread.session_id)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(retry.clone_history().await.len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mailbox_recovers_a_crash_after_user_insert_before_marker_update() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db"))
            .await
            .unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .await
        .unwrap();
        let reservation = root
            .reserve_spawn(&subagents::AgentPath::root(), "worker")
            .await
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().await.unwrap();
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "committed before marker update".into(),
            },
            true,
        )
        .await
        .unwrap();
        let sequence = graph.pending_for(&thread.thread_id, 0).await.unwrap()[0].sequence;
        let marker = format!("{MAILBOX_FINISH_PREFIX}{sequence}");
        let memory_dir = temp.path().join("memory");
        {
            let sessions = session::SessionStore::open_sessions_dir(&memory_dir.join("data"))
                .await
                .unwrap();
            sessions
                .create_session(&thread.session_id, "tauri", None, None, None)
                .await
                .unwrap();
            let item = agent_protocol::ResponseItem::Message {
                id: None,
                role: "user".into(),
                content: vec![agent_protocol::ContentItem::InputText {
                    text: "committed before marker update".into(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: Some(serde_json::json!({
                    "astro_memory_marker": marker,
                })),
            };
            sessions
                .append_response_item(session::NewResponseItem {
                    session_id: &thread.session_id,
                    item: &item,
                    token_count: None,
                    finish_reason: None,
                })
                .await
                .unwrap();
            let stored = sessions
                .get_response_items(&thread.session_id)
                .await
                .unwrap();
            assert_eq!(stored.len(), 1);
            assert!(stored[0].compressed_text().is_none());
        }

        let retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir.clone()),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path,
        )
        .await
        .unwrap();
        assert_eq!(retry.clone_history().await.len(), 1);

        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            1
        );
        assert!(graph
            .pending_for(&thread.thread_id, 0)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            0
        );
        let stored = session::SessionStore::open_sessions_dir(&memory_dir.join("data"))
            .await
            .unwrap()
            .get_response_items(&thread.session_id)
            .await
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0]
                .item
                .metadata()
                .and_then(|value| value.get("astro_memory_marker"))
                .and_then(serde_json::Value::as_str),
            Some(marker.as_str())
        );
        let history = retry.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0]
                .metadata()
                .and_then(|value| value.get("astro_memory_marker"))
                .and_then(serde_json::Value::as_str),
            Some(marker.as_str())
        );
        assert_eq!(
            serde_json::to_string(&history)
                .unwrap()
                .matches("committed before marker update")
                .count(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn main_steer_recovers_a_native_response_item_with_media() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db"))
            .await
            .unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .await
        .unwrap();
        let image = "data:image/png;base64,bGVnYWN5LXN0ZWVy";
        root.persist_main_steer(
            &subagents::AgentPath::root(),
            encode_main_steer_input(&TurnInput {
                content: "legacy steer".into(),
                image_data_urls: vec![image.into()],
                client_message_id: None,
            })
            .unwrap(),
        )
        .await
        .unwrap();
        let sequence = graph.pending_for("root-v2", 0).await.unwrap()[0].sequence;
        let marker = format!("{MAILBOX_FINISH_PREFIX}{sequence}");
        let memory_dir = temp.path().join("memory");
        {
            let sessions = session::SessionStore::open_sessions_dir(&memory_dir.join("data"))
                .await
                .unwrap();
            sessions
                .create_session("root-v2", "tauri", None, None, None)
                .await
                .unwrap();
            let item = agent_protocol::ResponseItem::Message {
                id: None,
                role: "user".into(),
                content: vec![
                    agent_protocol::ContentItem::InputText {
                        text: "legacy steer".into(),
                    },
                    agent_protocol::ContentItem::InputImage {
                        image_url: image.into(),
                        detail: None,
                    },
                ],
                phase: None,
                internal_chat_message_metadata_passthrough: Some(serde_json::json!({
                    "astro_memory_marker": marker,
                })),
            };
            sessions
                .append_response_item(session::NewResponseItem::new("root-v2", &item))
                .await
                .unwrap();
        }
        let retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir.clone()),
            "root-v2".into(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            subagents::AgentPath::root(),
        )
        .await
        .unwrap();

        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            1
        );
        assert!(graph.pending_for("root-v2", 0).await.unwrap().is_empty());
        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            0
        );
        let stored = session::SessionStore::open_sessions_dir(&memory_dir.join("data"))
            .await
            .unwrap()
            .get_response_items("root-v2")
            .await
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0]
                .item
                .metadata()
                .and_then(|value| value.get("astro_memory_marker"))
                .and_then(serde_json::Value::as_str),
            Some(marker.as_str())
        );
        let history = retry.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content_str(), "legacy steer");
        assert!(matches!(
            &history[0],
            agent_protocol::ResponseItem::Message { content, .. }
                if content.iter().any(|item| matches!(
                    item,
                    agent_protocol::ContentItem::InputImage { image_url, .. }
                        if image_url == image
                ))
        ));
        assert_eq!(
            history[0]
                .metadata()
                .and_then(|value| value.get("astro_memory_marker"))
                .and_then(serde_json::Value::as_str),
            Some(marker.as_str())
        );
        assert_eq!(
            serde_json::to_string(&history)
                .unwrap()
                .matches("legacy steer")
                .count(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mailbox_retry_repairs_a_memory_only_delivery_before_ack() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db"))
            .await
            .unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .await
        .unwrap();
        let reservation = root
            .reserve_spawn(&subagents::AgentPath::root(), "worker")
            .await
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().await.unwrap();
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "memory only follow up".into(),
            },
            true,
        )
        .await
        .unwrap();
        let sequence = graph.pending_for(&thread.thread_id, 0).await.unwrap()[0].sequence;
        let marker = format!("agent-mailbox-through:{sequence}");
        let memory_dir = temp.path().join("memory");
        let sessions = session::SessionStore::open_sessions_dir(&memory_dir.join("data"))
            .await
            .unwrap();
        sessions
            .create_session(&thread.session_id, "tauri", None, None, None)
            .await
            .unwrap();
        let retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path,
        )
        .await
        .unwrap();
        let mut memory_only = agent_protocol::ResponseItem::user_text("memory only follow up");
        *memory_only.metadata_mut().unwrap() = Some(serde_json::json!({
            "astro_memory_marker": marker
        }));
        retry.record_items(vec![memory_only]).await;
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "arrived after the partial delivery".into(),
            },
            true,
        )
        .await
        .unwrap();

        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            1
        );
        assert_eq!(
            graph.pending_for(&thread.thread_id, 0).await.unwrap().len(),
            1
        );
        assert_eq!(
            sessions
                .get_response_items(&thread.session_id)
                .await
                .unwrap()
                .len(),
            1
        );
        let history = retry.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(
            serde_json::to_string(&history)
                .unwrap()
                .matches("memory only follow up")
                .count(),
            1
        );
        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            0
        );
        assert_eq!(
            graph.pending_for(&thread.thread_id, 0).await.unwrap().len(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mailbox_retry_with_both_sides_present_only_acks_once() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db"))
            .await
            .unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .await
        .unwrap();
        let reservation = root
            .reserve_spawn(&subagents::AgentPath::root(), "worker")
            .await
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().await.unwrap();
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "already on both sides".into(),
            },
            true,
        )
        .await
        .unwrap();
        let memory_dir = temp.path().join("memory");
        let sessions = session::SessionStore::open_sessions_dir(&memory_dir.join("data"))
            .await
            .unwrap();
        sessions
            .create_session(&thread.session_id, "tauri", None, None, None)
            .await
            .unwrap();
        let retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path,
        )
        .await
        .unwrap();
        let cancel = retry.cancel_signal();
        retry.set_turn_input_after_memory_write_hook(Some(Arc::new(move || cancel.cancel())));

        assert!(drain_mailbox_at_safe_boundary(&retry).await.is_err());
        assert_eq!(
            sessions
                .get_response_items(&thread.session_id)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(retry.clone_history().await.len(), 1);
        assert_eq!(
            graph.pending_for(&thread.thread_id, 0).await.unwrap().len(),
            1
        );
        retry.set_turn_input_after_memory_write_hook(None);
        retry.cancel_signal().reset();

        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            1
        );
        assert!(graph
            .pending_for(&thread.thread_id, 0)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            sessions
                .get_response_items(&thread.session_id)
                .await
                .unwrap()
                .len(),
            1
        );
        let history = retry.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(
            serde_json::to_string(&history)
                .unwrap()
                .matches("already on both sides")
                .count(),
            1
        );
        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn main_steer_retry_repairs_db_only_delivery_with_media() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db"))
            .await
            .unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .await
        .unwrap();
        root.persist_main_steer(
            &subagents::AgentPath::root(),
            encode_main_steer_input(&TurnInput {
                content: "steer through failure".into(),
                image_data_urls: vec!["data:image/png;base64,c3RlZXI=".into()],
                client_message_id: None,
            })
            .unwrap(),
        )
        .await
        .unwrap();
        let memory_dir = temp.path().join("memory");
        let sessions = session::SessionStore::open_sessions_dir(&memory_dir.join("data"))
            .await
            .unwrap();
        sessions
            .create_session("root-v2", "tauri", None, None, None)
            .await
            .unwrap();
        let retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir),
            "root-v2".into(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            subagents::AgentPath::root(),
        )
        .await
        .unwrap();
        let cancel = retry.cancel_signal();
        retry.set_turn_input_after_db_write_hook(Some(Arc::new(move || {
            cancel.cancel();
            anyhow::bail!("steer failpoint after DB write")
        })));

        assert!(drain_mailbox_at_safe_boundary(&retry).await.is_err());
        assert_eq!(
            sessions.get_response_items("root-v2").await.unwrap().len(),
            1
        );
        assert!(retry.clone_history().await.is_empty());
        assert_eq!(graph.pending_for("root-v2", 0).await.unwrap().len(), 1);
        retry.set_turn_input_after_db_write_hook(None);
        retry.cancel_signal().reset();

        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            1
        );
        assert!(graph.pending_for("root-v2", 0).await.unwrap().is_empty());
        let history = retry.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content_str(), "steer through failure");
        assert!(matches!(
            &history[0],
            agent_protocol::ResponseItem::Message { content, .. }
                if content.iter().any(|item| matches!(
                    item,
                    agent_protocol::ContentItem::InputImage { image_url, .. }
                        if image_url == "data:image/png;base64,c3RlZXI="
                ))
        ));
        assert_eq!(
            drain_mailbox_at_safe_boundary(&retry)
                .await
                .unwrap()
                .delivered,
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mailbox_waits_until_the_current_user_turn_has_an_assistant_boundary() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db"))
            .await
            .unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .await
        .unwrap();
        root.persist_main_steer(
            &subagents::AgentPath::root(),
            encode_main_steer_input(&TurnInput {
                content: "early steer".into(),
                image_data_urls: Vec::new(),
                client_message_id: None,
            })
            .unwrap(),
        )
        .await
        .unwrap();
        let session = Session::with_session_id_for_agent_thread(
            Config::with_defaults(temp.path().join("memory")),
            "root-v2".into(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            subagents::AgentPath::root(),
        )
        .await
        .unwrap();
        session
            .record_items(vec![agent_protocol::ResponseItem::user_text("initial")])
            .await;

        let deferred = drain_mailbox_at_safe_boundary(&session).await.unwrap();
        assert_eq!(deferred.delivered, 0);
        assert!(deferred.deferred);
        assert_eq!(graph.pending_for("root-v2", 0).await.unwrap().len(), 1);

        session
            .record_items(vec![agent_protocol::ResponseItem::assistant_text(
                "first answer",
            )])
            .await;
        let delivered = drain_mailbox_at_safe_boundary(&session).await.unwrap();
        assert_eq!(delivered.delivered, 1);
        assert_eq!(delivered.delivered_steer_ids.len(), 1);
        assert!(!delivered.deferred);
        assert!(graph.pending_for("root-v2", 0).await.unwrap().is_empty());
        assert!(crate::runtime::validate_message_order(
            &session.clone_history().await
        ));
    }
}
