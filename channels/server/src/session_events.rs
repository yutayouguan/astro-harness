//! Session-scoped memory event fan-out for gRPC subscribers.

use proto::session_event::Payload;
use proto::{MemoryUpdatedEvent, PendingChangedEvent, SessionEvent, SessionMetadataChangedEvent};
use tokio::sync::broadcast;

/// Subscriber filter matching [`SubscribeSessionEventsRequest`] semantics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubscribeFilter {
    /// Empty / absent = global pending only; non-empty = that session + global pending.
    pub session_id: Option<String>,
    /// Optional agent filter; empty = no agent filter.
    pub agent_id: Option<String>,
}

/// Internal memory-updated payload before proto conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryUpdatedPayload {
    pub source: String,
    pub target: String,
    pub summary: String,
    pub live_written: bool,
}

/// Internal pending-queue payload before proto conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChangedPayload {
    pub pending_count: u32,
    pub reason: String,
}

/// Internal session metadata payload (e.g. auto/manual title change).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMetadataChangedPayload {
    pub title: String,
}

/// Hub message: one memory, pending, or metadata notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEventMsg {
    pub session_id: Option<String>,
    pub agent_id: String,
    pub memory_updated: Option<MemoryUpdatedPayload>,
    pub pending_changed: Option<PendingChangedPayload>,
    pub session_metadata_changed: Option<SessionMetadataChangedPayload>,
}

/// Filtered receiver skipping non-matching broadcast events.
pub struct FilteredReceiver {
    rx: broadcast::Receiver<SessionEventMsg>,
    filter: SubscribeFilter,
}

impl FilteredReceiver {
    /// Returns the next event matching the subscription filter, or `None` if closed.
    pub async fn recv(&mut self) -> Option<SessionEventMsg> {
        loop {
            match self.rx.recv().await {
                Ok(ev) if event_matches(&self.filter, &ev) => return Some(ev),
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

/// Tokio broadcast fan-out for session memory events.
#[derive(Debug, Clone)]
pub struct SessionEventHub {
    tx: broadcast::Sender<SessionEventMsg>,
}

impl SessionEventHub {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity.max(1));
        Self { tx }
    }

    /// Publishes an event; no-op when there are no subscribers.
    pub fn publish(&self, ev: SessionEventMsg) {
        let _ = self.tx.send(ev);
    }

    /// Raw broadcast receiver (no filtering).
    pub fn subscribe_raw(&self) -> broadcast::Receiver<SessionEventMsg> {
        self.tx.subscribe()
    }

    /// Returns a receiver that only yields events matching `filter`.
    pub fn subscribe(&self, filter: SubscribeFilter) -> FilteredReceiver {
        FilteredReceiver {
            rx: self.tx.subscribe(),
            filter,
        }
    }
}

/// Whether `ev` should be delivered to a subscriber with `filter`.
pub fn event_matches(filter: &SubscribeFilter, ev: &SessionEventMsg) -> bool {
    if let Some(ref want_agent) = filter.agent_id {
        if !want_agent.is_empty() && &ev.agent_id != want_agent {
            return false;
        }
    }
    // 侧栏需要任意会话的标题更新，即使当前订阅正过滤到另一会话。
    if ev.session_metadata_changed.is_some() {
        return true;
    }
    match filter.session_id.as_deref().filter(|s| !s.is_empty()) {
        None => ev.pending_changed.is_some() && ev.session_id.is_none(),
        Some(sid) => {
            if ev.pending_changed.is_some() && ev.session_id.is_none() {
                return true;
            }
            ev.session_id.as_deref() == Some(sid)
        }
    }
}

/// Converts a hub message to the gRPC [`SessionEvent`] type.
pub fn to_proto(msg: &SessionEventMsg) -> SessionEvent {
    let payload = if let Some(ref mem) = msg.memory_updated {
        Some(Payload::MemoryUpdated(MemoryUpdatedEvent {
            source: mem.source.clone(),
            target: mem.target.clone(),
            summary: mem.summary.clone(),
            live_written: mem.live_written,
        }))
    } else if let Some(ref pend) = msg.pending_changed {
        Some(Payload::PendingChanged(PendingChangedEvent {
            pending_count: pend.pending_count,
            reason: pend.reason.clone(),
        }))
    } else {
        msg.session_metadata_changed.as_ref().map(|meta| {
            Payload::SessionMetadataChanged(SessionMetadataChangedEvent {
                title: meta.title.clone(),
            })
        })
    };

    SessionEvent {
        session_id: msg.session_id.clone().unwrap_or_default(),
        agent_id: msg.agent_id.clone(),
        ts_ms: chrono::Utc::now().timestamp_millis(),
        payload,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_reaches_matching_subscriber() {
        let hub = SessionEventHub::new(16);
        let mut rx = hub.subscribe(SubscribeFilter {
            session_id: Some("s1".into()),
            agent_id: None,
        });
        hub.publish(SessionEventMsg {
            session_id: Some("s1".into()),
            agent_id: "workspace".into(),
            memory_updated: Some(MemoryUpdatedPayload {
                source: "review".into(),
                target: "memory".into(),
                summary: "ok".into(),
                live_written: true,
            }),
            pending_changed: None,
            session_metadata_changed: None,
        });
        let ev = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ev.session_id.as_deref(), Some("s1"));
        assert!(ev.memory_updated.unwrap().live_written);
    }

    #[tokio::test]
    async fn empty_session_filter_receives_global_pending() {
        let hub = SessionEventHub::new(16);
        let mut rx = hub.subscribe(SubscribeFilter {
            session_id: None,
            agent_id: None,
        });
        hub.publish(SessionEventMsg {
            session_id: None,
            agent_id: "workspace".into(),
            memory_updated: None,
            pending_changed: Some(PendingChangedPayload {
                pending_count: 2,
                reason: "enqueued".into(),
            }),
            session_metadata_changed: None,
        });
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev.pending_changed.unwrap().pending_count, 2);
    }

    #[test]
    fn publish_with_no_subscribers_does_not_panic() {
        let hub = SessionEventHub::new(8);
        hub.publish(SessionEventMsg {
            session_id: Some("x".into()),
            agent_id: "a".into(),
            memory_updated: Some(MemoryUpdatedPayload {
                source: "approve".into(),
                target: "user".into(),
                summary: "x".into(),
                live_written: true,
            }),
            pending_changed: None,
            session_metadata_changed: None,
        });
    }

    #[test]
    fn metadata_changed_matches_even_when_subscribed_to_other_session() {
        let filter = SubscribeFilter {
            session_id: Some("s-active".into()),
            agent_id: None,
        };
        let ev = SessionEventMsg {
            session_id: Some("s-other".into()),
            agent_id: "workspace".into(),
            memory_updated: None,
            pending_changed: None,
            session_metadata_changed: Some(SessionMetadataChangedPayload {
                title: "新标题".into(),
            }),
        };
        assert!(event_matches(&filter, &ev));
    }
}
