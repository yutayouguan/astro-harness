//! 分叉 rollout：把源 thread 的历史前缀复制成新 thread 自己的 append-only 文件。
//!
//! rollout 是线程历史的权威事实源，因此聊天分叉不能只复制 SQLite 投影——否则新分支
//! 一旦走 rollout 恢复就是空的。源文件全程只读。

use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::{find_rollout, new_rollout_path, read_rollout, RolloutItem, RolloutRecorder};

#[derive(Debug, Clone, PartialEq)]
pub struct ForkedRollout {
    pub path: PathBuf,
    pub copied_items: usize,
    /// 实际写入新文件的 user 消息数；源历史更短时会小于请求值。
    pub copied_user_turns: usize,
}

/// 把 `source_thread_id` 的 rollout 前缀复制给 `new_thread_id`。
///
/// 前缀停在第 `keep_user_turns + 1` 条 user 消息之前，与 SQLite 侧的分叉边界一致。
/// 源 thread 没有 rollout 时返回 `Ok(None)`：老会话可能只有 SQLite 投影。
pub async fn fork_rollout(
    root: &Path,
    source_thread_id: &str,
    new_thread_id: &str,
    keep_user_turns: usize,
    ephemeral: bool,
    exclude_turns: bool,
    now: DateTime<Utc>,
) -> io::Result<Option<ForkedRollout>> {
    if source_thread_id == new_thread_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "fork_rollout: source and target thread ids must differ",
        ));
    }
    let Some(source_path) = find_rollout(root, source_thread_id)? else {
        return Ok(None);
    };
    if find_rollout(root, new_thread_id)?.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("rollout already exists for thread {new_thread_id}"),
        ));
    }

    let items = read_rollout(&source_path).await?;
    let mut prefix = Vec::new();
    let mut copied_user_turns = 0usize;
    let mut has_meta = false;
    for item in items {
        if is_user_response(&item) {
            if copied_user_turns == keep_user_turns {
                break;
            }
            copied_user_turns += 1;
        }
        has_meta |= matches!(item, RolloutItem::SessionMeta(_));
        prefix.push(retarget(
            item,
            new_thread_id,
            source_thread_id,
            keep_user_turns,
            ephemeral,
            exclude_turns,
        ));
    }
    if !has_meta {
        prefix.insert(
            0,
            RolloutItem::SessionMeta(fork_meta(
                new_thread_id,
                source_thread_id,
                keep_user_turns,
                ephemeral,
                exclude_turns,
            )),
        );
    }

    let path = new_rollout_path(root, new_thread_id, now);
    let recorder = RolloutRecorder::open(path.clone()).await?;
    let copied_items = prefix.len();
    recorder.record(prefix).await?;
    recorder.flush().await?;
    recorder.shutdown().await?;

    Ok(Some(ForkedRollout {
        path,
        copied_items,
        copied_user_turns,
    }))
}

fn is_user_response(item: &RolloutItem) -> bool {
    matches!(
        item,
        RolloutItem::ResponseItem(agent_protocol::ResponseItem::Message { role, .. })
            if role == "user"
    )
}

/// 复制过来的 session meta 必须指向新 thread，并留下分叉来源。
fn retarget(
    item: RolloutItem,
    new_thread_id: &str,
    source_thread_id: &str,
    keep_user_turns: usize,
    ephemeral: bool,
    exclude_turns: bool,
) -> RolloutItem {
    let RolloutItem::SessionMeta(mut meta) = item else {
        return item;
    };
    if let Some(object) = meta.as_object_mut() {
        for key in ["thread_id", "id", "session_id"] {
            if object.get(key).is_some_and(serde_json::Value::is_string) {
                object.insert(key.into(), new_thread_id.into());
            }
        }
        object.insert(
            "forked_from".into(),
            serde_json::json!({
                "thread_id": source_thread_id,
                "user_turns": keep_user_turns,
            }),
        );
        object.insert("ephemeral".into(), ephemeral.into());
        object.insert("exclude_turns".into(), exclude_turns.into());
    }
    RolloutItem::SessionMeta(meta)
}

fn fork_meta(
    new_thread_id: &str,
    source_thread_id: &str,
    keep_user_turns: usize,
    ephemeral: bool,
    exclude_turns: bool,
) -> serde_json::Value {
    serde_json::json!({
        "thread_id": new_thread_id,
        "forked_from": {
            "thread_id": source_thread_id,
            "user_turns": keep_user_turns,
        },
        "ephemeral": ephemeral,
        "exclude_turns": exclude_turns,
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use tempfile::TempDir;
    use types::message::Message;

    use super::fork_rollout;
    use crate::{find_rollout, new_rollout_path, read_rollout, RolloutItem, RolloutRecorder};

    fn response(message: Message) -> RolloutItem {
        RolloutItem::ResponseItem(
            crate::response_items_from_message(&message, None)
                .unwrap()
                .into_iter()
                .next()
                .unwrap(),
        )
    }

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 8, 26, 9, 0, 0).unwrap()
    }

    async fn seed_source(root: &std::path::Path) {
        let path = new_rollout_path(root, "source", now());
        let recorder = RolloutRecorder::open(path).await.unwrap();
        recorder
            .record(vec![
                RolloutItem::SessionMeta(serde_json::json!({"thread_id": "source"})),
                response(Message::user("first")),
                RolloutItem::WorldState(serde_json::json!({
                    "full": true,
                    "state": {"astro.prompt_context.v1": {"version": 2, "messages": []}},
                })),
                response(Message::assistant("first answer")),
                response(Message::user("second")),
                RolloutItem::WorldState(serde_json::json!({
                    "full": false,
                    "state": {"astro.prompt_context.v1": {"version": 2, "messages": []}},
                })),
                response(Message::assistant("second answer")),
            ])
            .await
            .unwrap();
        recorder.flush().await.unwrap();
        recorder.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn fork_copies_the_prefix_and_retargets_session_meta() {
        let temp = TempDir::new().unwrap();
        seed_source(temp.path()).await;
        let source_before = read_rollout(&find_rollout(temp.path(), "source").unwrap().unwrap())
            .await
            .unwrap();

        let forked = fork_rollout(temp.path(), "source", "branch", 1, true, true, now())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(forked.copied_user_turns, 1);
        let items = read_rollout(&forked.path).await.unwrap();
        let RolloutItem::SessionMeta(meta) = &items[0] else {
            panic!("expected retargeted session meta");
        };
        assert_eq!(meta["thread_id"], "branch");
        assert_eq!(meta["forked_from"]["thread_id"], "source");
        assert_eq!(meta["ephemeral"], true);
        assert_eq!(meta["exclude_turns"], true);
        let copied_messages = crate::reconstruct_messages(&items).unwrap();
        assert_eq!(
            copied_messages
                .iter()
                .map(|entry| entry.message.content_text())
                .collect::<Vec<_>>(),
            vec!["first", "first answer"]
        );
        let copied_world_states = items
            .iter()
            .filter_map(|item| match item {
                RolloutItem::WorldState(value) => Some(value),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(copied_world_states.len(), 1);
        assert_eq!(copied_world_states[0]["full"], true);
        assert_eq!(
            read_rollout(&find_rollout(temp.path(), "source").unwrap().unwrap())
                .await
                .unwrap(),
            source_before
        );
    }

    #[tokio::test]
    async fn fork_without_source_history_is_not_an_error() {
        let temp = TempDir::new().unwrap();
        assert!(
            fork_rollout(temp.path(), "missing", "branch", 3, false, false, now())
                .await
                .unwrap()
                .is_none()
        );
        assert!(find_rollout(temp.path(), "branch").unwrap().is_none());
    }

    #[tokio::test]
    async fn fork_refuses_to_overwrite_an_existing_target_history() {
        let temp = TempDir::new().unwrap();
        seed_source(temp.path()).await;
        fork_rollout(temp.path(), "source", "branch", 2, false, false, now())
            .await
            .unwrap()
            .unwrap();

        let error = fork_rollout(temp.path(), "source", "branch", 2, false, false, now())
            .await
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    }
}
