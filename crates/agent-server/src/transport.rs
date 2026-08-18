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
    ) -> (mpsc::Receiver<proto::ThreadEvent>, CancellationToken) {
        let (tx, rx) = mpsc::channel(self.capacity);
        let cancel = CancellationToken::new();
        let entry = Arc::new(ConnectionEntry {
            tx,
            cancel: cancel.clone(),
        });
        let replaced = self.entries.write().await.insert(connection_id, entry);
        if let Some(replaced) = replaced {
            replaced.cancel.cancel();
        }
        (rx, cancel)
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

    /// Removes and cancels the current connection with `connection_id`.
    pub async fn remove(&self, connection_id: &str) {
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
        if cancel {
            observed.cancel.cancel();
        }
        {
            let mut entries = self.entries.write().await;
            let is_current = entries
                .get(connection_id)
                .is_some_and(|current| Arc::ptr_eq(current, observed));
            if is_current {
                entries.remove(connection_id);
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
        let (_slow_rx, slow_cancel) = registry.register("slow".into()).await;
        let (mut fast_rx, fast_cancel) = registry.register("fast".into()).await;
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
        let (mut old_rx, old_cancel) = registry.register("connection".into()).await;
        let (mut new_rx, new_cancel) = registry.register("connection".into()).await;

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
    async fn closed_connection_is_removed() {
        let registry = ConnectionRegistry::default();
        let (rx, cancel) = registry.register("closed".into()).await;
        drop(rx);

        assert!(!registry.send_to("closed", event("thread", "closed")).await);
        assert!(!registry.entries.read().await.contains_key("closed"));
        assert!(!cancel.is_cancelled());
    }

    #[tokio::test]
    async fn stale_full_cleanup_does_not_remove_replacement() {
        let registry = ConnectionRegistry::with_capacity(1);
        let (_old_rx, old_cancel) = registry.register("connection".into()).await;
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

        let (mut replacement_rx, replacement_cancel) = registry.register("connection".into()).await;
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
    async fn remove_cancels_current_connection() {
        let registry = ConnectionRegistry::default();
        let (_rx, cancel) = registry.register("connection".into()).await;

        registry.remove("connection").await;

        assert!(cancel.is_cancelled());
        assert!(!registry.entries.read().await.contains_key("connection"));
    }
}
