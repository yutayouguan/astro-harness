use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

/// Per-connection queue size used by the thread-event transport.
pub const CHANNEL_CAPACITY: usize = 128;

struct ConnectionEntry {
    tx: mpsc::Sender<proto::ThreadEvent>,
    cancel: CancellationToken,
}

/// Opaque identity for one registration of a connection id.
///
/// Keep this handle with the stream and pass it to
/// [`ConnectionRegistry::remove_generation`] when that stream exits. Unlike an
/// id-only removal, cleanup through this handle cannot evict a newer stream
/// that reused the same connection id.
pub struct ConnectionGeneration {
    connection_id: String,
    entry: Arc<ConnectionEntry>,
}

/// Registry of bounded, independently backpressured thread-event connections.
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
    /// Creates a registry whose connections each have the supplied queue capacity.
    ///
    /// # Panics
    ///
    /// Panics when `capacity` is zero because Tokio bounded channels require a
    /// positive capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "connection channel capacity must be positive");
        Self {
            capacity,
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Registers a connection, cancelling any older connection with the same id.
    pub async fn register(
        &self,
        connection_id: String,
    ) -> (
        mpsc::Receiver<proto::ThreadEvent>,
        CancellationToken,
        ConnectionGeneration,
    ) {
        let (tx, rx) = mpsc::channel(self.capacity);
        let cancel = CancellationToken::new();
        let entry = Arc::new(ConnectionEntry {
            tx,
            cancel: cancel.clone(),
        });
        let generation = ConnectionGeneration {
            connection_id: connection_id.clone(),
            entry: Arc::clone(&entry),
        };
        let replaced = self.entries.write().await.insert(connection_id, entry);
        if let Some(replaced) = replaced {
            replaced.cancel.cancel();
        }
        (rx, cancel, generation)
    }

    /// Attempts to enqueue an event without waiting for a slow consumer.
    ///
    /// A full queue evicts and cancels only the generation that was observed by
    /// this send. A concurrently registered replacement remains active.
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

    /// Cleans up exactly the registration represented by `generation`.
    ///
    /// This is the normal stream-cleanup API. It cancels the observed
    /// generation and removes it only while it remains current.
    pub async fn remove_generation(&self, generation: &ConnectionGeneration) {
        self.remove_if_current(&generation.connection_id, &generation.entry, true)
            .await;
    }

    /// Administratively removes whichever generation is current for an id.
    ///
    /// Stream teardown must use [`Self::remove_generation`] instead, otherwise
    /// a stale stream could remove its replacement.
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
    ) {
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
        if cancel {
            if let Some(entry) = removed {
                entry.cancel.cancel();
            }
        }
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
