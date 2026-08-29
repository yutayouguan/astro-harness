use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

/// Thread 事件传输层的每连接队列容量。
pub const CHANNEL_CAPACITY: usize = 128;

struct ConnectionEntry {
    tx: mpsc::Sender<proto::ThreadEvent>,
    cancel: CancellationToken,
    key: ConnectionGenerationKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConnectionGenerationKey {
    connection_id: String,
    generation_id: uuid::Uuid,
}

impl ConnectionGenerationKey {
    pub(crate) fn new(connection_id: impl Into<String>) -> Self {
        Self {
            connection_id: connection_id.into().trim().to_string(),
            generation_id: uuid::Uuid::new_v4(),
        }
    }

    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }
}

/// 某次 connection id 注册的不透明标识。
///
/// 将此 handle 与 stream 一起保留，在 stream 退出时传给
/// [`ConnectionRegistry::remove_generation`]。与按 id 移除不同，
/// 通过此 handle 清理不会误驱逐复用了同一 connection id 的新 stream。
#[derive(Clone)]
pub struct ConnectionGeneration {
    key: ConnectionGenerationKey,
    entry: Arc<ConnectionEntry>,
}

impl ConnectionGeneration {
    pub fn key(&self) -> &ConnectionGenerationKey {
        &self.key
    }
}

/// 有界、独立背压的 Thread 事件连接注册表。
#[derive(Clone)]
pub struct ConnectionRegistry {
    capacity: usize,
    entries: Arc<RwLock<HashMap<String, Arc<ConnectionEntry>>>>,
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        Self::with_capacity(CHANNEL_CAPACITY)
    }
}

impl ConnectionRegistry {
    /// 以指定队列容量创建注册表。
    ///
    /// # Panics
    ///
    /// `capacity` 为零时 panic，因为 Tokio bounded channel 要求正数容量。
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "connection channel capacity must be positive");
        Self {
            capacity,
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册连接，取消同 id 下的旧连接。
    pub async fn register(
        &self,
        connection_id: String,
    ) -> (
        mpsc::Receiver<proto::ThreadEvent>,
        CancellationToken,
        ConnectionGeneration,
    ) {
        let connection_id = connection_id.trim().to_string();
        let (tx, rx) = mpsc::channel(self.capacity);
        let cancel = CancellationToken::new();
        let key = ConnectionGenerationKey::new(connection_id.clone());
        let entry = Arc::new(ConnectionEntry {
            tx,
            cancel: cancel.clone(),
            key: key.clone(),
        });
        let generation = ConnectionGeneration {
            key,
            entry: Arc::clone(&entry),
        };
        let replaced = self.entries.write().await.insert(connection_id, entry);
        if let Some(replaced) = replaced {
            replaced.cancel.cancel();
        }
        (rx, cancel, generation)
    }

    /// 尝试入队事件，不等待慢消费者。
    ///
    /// 队列满时仅驱逐并取消本次 send 所观察到的 generation；
    /// 并发注册的替代连接不受影响。
    pub async fn send_to(&self, connection_id: &str, event: proto::ThreadEvent) -> bool {
        let entry = {
            let entries = self.entries.read().await;
            let Some(entry) = entries.get(connection_id) else {
                return false;
            };
            Arc::clone(entry)
        };

        match entry.tx.try_send(event) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.remove_if_current(connection_id, &entry, true).await;
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.remove_if_current(connection_id, &entry, false).await;
                false
            }
        }
    }

    /// 仅当指定的订阅 generation 仍为当前时才入队。
    pub async fn send_to_generation(
        &self,
        key: &ConnectionGenerationKey,
        event: proto::ThreadEvent,
    ) -> bool {
        let entry = {
            let entries = self.entries.read().await;
            let Some(entry) = entries.get(&key.connection_id) else {
                return false;
            };
            if entry.key != *key {
                return false;
            }
            Arc::clone(entry)
        };
        match entry.tx.try_send(event) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.remove_if_current(&key.connection_id, &entry, true)
                    .await;
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.remove_if_current(&key.connection_id, &entry, false)
                    .await;
                false
            }
        }
    }

    pub async fn current_generation_key(
        &self,
        connection_id: &str,
    ) -> Option<ConnectionGenerationKey> {
        self.entries
            .read()
            .await
            .get(connection_id.trim())
            .map(|entry| entry.key.clone())
    }

    /// 返回当前是否有活跃注册持有 `connection_id`。
    pub async fn contains(&self, connection_id: &str) -> bool {
        self.entries.read().await.contains_key(connection_id)
    }

    /// 精确清理 `generation` 所代表的注册。
    ///
    /// 这是常规的 stream 清理 API。取消被观察到的 generation，
    /// 仅在其仍为当前时移除。
    pub async fn remove_generation(&self, generation: &ConnectionGeneration) -> bool {
        self.remove_if_current(&generation.key.connection_id, &generation.entry, true)
            .await
    }

    /// 管理性移除指定 id 当前的 generation。
    ///
    /// Stream 拆除应使用 [`Self::remove_generation`]，否则过期 stream
    /// 可能误移除其替代连接。
    pub async fn force_remove(&self, connection_id: &str) {
        if let Some(entry) = self.entries.write().await.remove(connection_id) {
            entry.cancel.cancel();
        }
    }

    async fn remove_if_current(
        &self,
        connection_id: &str,
        observed: &Arc<ConnectionEntry>,
        cancel: bool,
    ) -> bool {
        let removed = {
            let mut entries = self.entries.write().await;
            let is_current = entries
                .get(connection_id)
                .is_some_and(|current| Arc::ptr_eq(current, observed));
            if is_current {
                entries.remove(connection_id)
            } else {
                None
            }
        };
        let was_removed = removed.is_some();
        if cancel {
            if let Some(entry) = removed {
                entry.cancel.cancel();
                return true;
            }
        }
        was_removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(thread_id: &str, turn_id: &str) -> proto::ThreadEvent {
        proto::ThreadEvent {
            thread_id: thread_id.into(),
            turn_id: turn_id.into(),
            payload: Some(proto::thread_event::Payload::TurnStarted(
                proto::ThreadTurnStarted {
                    turn_id: turn_id.into(),
                },
            )),
        }
    }

    #[test]
    fn default_capacity_is_128() {
        assert_eq!(ConnectionRegistry::default().capacity, CHANNEL_CAPACITY);
        assert_eq!(CHANNEL_CAPACITY, 128);
    }

    #[tokio::test]
    async fn slow_connection_does_not_block_fast_connection() {
        let registry = ConnectionRegistry::with_capacity(1);
        let (_slow_rx, slow_cancel, _slow_generation) = registry.register("slow".into()).await;
        let (mut fast_rx, fast_cancel, _fast_generation) = registry.register("fast".into()).await;
        registry.send_to("slow", event("thread", "first")).await;
        registry.send_to("slow", event("thread", "overflow")).await;
        registry.send_to("fast", event("thread", "first")).await;
        assert!(slow_cancel.is_cancelled());
        assert!(!fast_cancel.is_cancelled());
        let received = fast_rx.recv().await;
        assert_eq!(received.map(|event| event.turn_id), Some("first".into()));
    }

    #[tokio::test]
    async fn replacing_connection_cancels_previous_generation() {
        let registry = ConnectionRegistry::default();
        let (mut old_rx, old_cancel, _old_generation) =
            registry.register("connection".into()).await;
        let (mut new_rx, new_cancel, _new_generation) =
            registry.register("connection".into()).await;

        assert!(old_cancel.is_cancelled());
        assert!(!new_cancel.is_cancelled());
        assert!(registry.send_to("connection", event("thread", "new")).await);
        assert!(old_rx.try_recv().is_err());
        assert_eq!(
            new_rx.recv().await.map(|event| event.turn_id),
            Some("new".into())
        );
    }

    #[tokio::test]
    async fn stale_generation_cleanup_preserves_replacement() {
        let registry = ConnectionRegistry::default();
        let (_old_rx, _old_cancel, old_generation) = registry.register("connection".into()).await;
        let (mut replacement_rx, replacement_cancel, _replacement_generation) =
            registry.register("connection".into()).await;

        registry.remove_generation(&old_generation).await;

        assert!(!replacement_cancel.is_cancelled());
        assert!(
            registry
                .send_to("connection", event("thread", "replacement"))
                .await
        );
        assert_eq!(
            replacement_rx.recv().await.map(|event| event.turn_id),
            Some("replacement".into())
        );
    }

    #[tokio::test]
    async fn generation_scoped_send_never_retargets_replacement() {
        let registry = ConnectionRegistry::default();
        let (_old_rx, _old_cancel, old_generation) = registry.register(" connection ".into()).await;
        let (mut replacement_rx, _replacement_cancel, replacement_generation) =
            registry.register("connection".into()).await;

        assert!(
            !registry
                .send_to_generation(old_generation.key(), event("thread", "stale"))
                .await
        );
        assert!(replacement_rx.try_recv().is_err());
        assert!(
            registry
                .send_to_generation(replacement_generation.key(), event("thread", "current"))
                .await
        );
        assert_eq!(
            replacement_rx.recv().await.map(|event| event.turn_id),
            Some("current".into())
        );
    }

    #[tokio::test]
    async fn current_generation_cleanup_removes_and_cancels_connection() {
        let registry = ConnectionRegistry::default();
        let (_rx, cancel, generation) = registry.register("connection".into()).await;

        registry.remove_generation(&generation).await;

        assert!(cancel.is_cancelled());
        assert!(!registry.entries.read().await.contains_key("connection"));
    }

    #[tokio::test]
    async fn foreign_registry_cannot_cancel_a_connection_generation() {
        let registry_a = ConnectionRegistry::default();
        let registry_b = ConnectionRegistry::default();
        let (mut receiver_a, cancel_a, generation_a) =
            registry_a.register("connection".into()).await;
        let (mut receiver_b, cancel_b, _generation_b) =
            registry_b.register("connection".into()).await;

        registry_b.remove_generation(&generation_a).await;

        assert!(!cancel_a.is_cancelled());
        assert!(!cancel_b.is_cancelled());
        assert!(
            registry_a
                .send_to("connection", event("thread-a", "turn-a"))
                .await
        );
        assert!(
            registry_b
                .send_to("connection", event("thread-b", "turn-b"))
                .await
        );
        assert_eq!(
            receiver_a.recv().await.map(|event| event.turn_id),
            Some("turn-a".into())
        );
        assert_eq!(
            receiver_b.recv().await.map(|event| event.turn_id),
            Some("turn-b".into())
        );
    }

    #[tokio::test]
    async fn closed_connection_is_removed() {
        let registry = ConnectionRegistry::default();
        let (rx, cancel, _generation) = registry.register("closed".into()).await;
        drop(rx);

        assert!(!registry.send_to("closed", event("thread", "closed")).await);
        assert!(!registry.entries.read().await.contains_key("closed"));
        assert!(!cancel.is_cancelled());
    }

    #[tokio::test]
    async fn stale_full_cleanup_does_not_remove_replacement() {
        let registry = ConnectionRegistry::with_capacity(1);
        let (_old_rx, old_cancel, _old_generation) = registry.register("connection".into()).await;
        assert!(
            registry
                .send_to("connection", event("thread", "fill"))
                .await
        );

        let stale_entry = registry
            .entries
            .read()
            .await
            .get("connection")
            .cloned()
            .expect("registered connection must have an entry");
        assert!(matches!(
            stale_entry.tx.try_send(event("thread", "stale-overflow")),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_))
        ));

        let (mut replacement_rx, replacement_cancel, _replacement_generation) =
            registry.register("connection".into()).await;
        registry
            .remove_if_current("connection", &stale_entry, true)
            .await;

        assert!(old_cancel.is_cancelled());
        assert!(!replacement_cancel.is_cancelled());
        assert!(
            registry
                .send_to("connection", event("thread", "replacement"))
                .await
        );
        assert_eq!(
            replacement_rx.recv().await.map(|event| event.turn_id),
            Some("replacement".into())
        );
    }

    #[tokio::test]
    async fn force_remove_cancels_current_connection() {
        let registry = ConnectionRegistry::default();
        let (_rx, cancel, _generation) = registry.register("connection".into()).await;

        registry.force_remove("connection").await;

        assert!(cancel.is_cancelled());
        assert!(!registry.entries.read().await.contains_key("connection"));
    }
}
