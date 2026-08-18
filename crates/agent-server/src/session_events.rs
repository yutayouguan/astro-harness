//! Session-scoped memory, metadata, and Agent Thread event fan-out.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use proto::session_event::Payload;
use proto::{
    AgentThreadChangedEvent, MemoryUpdatedEvent, PendingChangedEvent, SessionEvent,
    SessionMetadataChangedEvent,
};
use tokio::sync::broadcast;
use uuid::Uuid;

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

/// Complete V2 Agent Thread projection carried by one activity event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentThreadChangedPayload {
    pub activity_sequence: u64,
    pub root_thread_id: String,
    pub thread_id: String,
    pub parent_thread_id: String,
    pub canonical_path: String,
    pub task_name: String,
    pub agent_type: String,
    pub session_id: String,
    pub status_kind: String,
    pub status_payload_json: String,
    pub activity_kind: String,
}

/// Hub message: one memory, pending, or metadata notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEventMsg {
    pub session_id: Option<String>,
    pub agent_id: String,
    pub memory_updated: Option<MemoryUpdatedPayload>,
    pub pending_changed: Option<PendingChangedPayload>,
    pub session_metadata_changed: Option<SessionMetadataChangedPayload>,
    pub agent_thread_changed: Option<AgentThreadChangedPayload>,
}

/// Hub-assigned event envelope used for ordered delivery and reconnect replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequencedSessionEvent {
    pub event_id: u64,
    pub ts_ms: i64,
    pub event: SessionEventMsg,
}

/// Filtered receiver skipping non-matching broadcast events.
pub struct FilteredReceiver {
    rx: broadcast::Receiver<SequencedSessionEvent>,
    filter: SubscribeFilter,
    replay: VecDeque<SequencedSessionEvent>,
    last_seen_event_id: u64,
    inner: Arc<SessionEventHubInner>,
}

impl FilteredReceiver {
    /// Returns the next event matching the subscription filter, or `None` if closed.
    pub async fn recv(&mut self) -> Option<SequencedSessionEvent> {
        loop {
            if let Some(ev) = self.replay.pop_front() {
                self.last_seen_event_id = self.last_seen_event_id.max(ev.event_id);
                return Some(ev);
            }
            match self.rx.recv().await {
                Ok(ev) => {
                    if ev.event_id <= self.last_seen_event_id {
                        continue;
                    }
                    self.last_seen_event_id = ev.event_id;
                    if event_matches(&self.filter, &ev.event) {
                        return Some(ev);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    self.replay = replay_after(&self.inner, &self.filter, self.last_seen_event_id);
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

#[derive(Debug)]
struct SessionEventHubInner {
    tx: broadcast::Sender<SequencedSessionEvent>,
    stream_id: String,
    history_capacity: usize,
    state: Mutex<SessionEventHubState>,
}

#[derive(Debug)]
struct SessionEventHubState {
    next_event_id: u64,
    history: VecDeque<SequencedSessionEvent>,
}

/// Tokio broadcast fan-out with bounded replay for reconnecting subscribers.
#[derive(Debug, Clone)]
pub struct SessionEventHub {
    inner: Arc<SessionEventHubInner>,
}

impl SessionEventHub {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let (tx, _) = broadcast::channel(capacity);
        Self {
            inner: Arc::new(SessionEventHubInner {
                tx,
                stream_id: Uuid::new_v4().to_string(),
                history_capacity: capacity,
                state: Mutex::new(SessionEventHubState {
                    next_event_id: 1,
                    history: VecDeque::with_capacity(capacity),
                }),
            }),
        }
    }

    /// Publishes an event and retains it for bounded reconnect replay.
    pub fn publish(&self, ev: SessionEventMsg) {
        // Sequence allocation, history insertion, and broadcast are serialized
        // so concurrent publishers cannot expose ids out of order.
        let mut state = lock_state(&self.inner);
        let sequenced = SequencedSessionEvent {
            event_id: state.next_event_id,
            ts_ms: chrono::Utc::now().timestamp_millis(),
            event: ev,
        };
        state.next_event_id = state.next_event_id.saturating_add(1);
        if state.history.len() == self.inner.history_capacity {
            state.history.pop_front();
        }
        state.history.push_back(sequenced.clone());
        let _ = self.inner.tx.send(sequenced);
    }

    /// Raw broadcast receiver (no filtering).
    pub fn subscribe_raw(&self) -> broadcast::Receiver<SequencedSessionEvent> {
        self.inner.tx.subscribe()
    }

    /// Stable identifier for this in-process event stream generation.
    pub fn stream_id(&self) -> &str {
        &self.inner.stream_id
    }

    /// Returns a filtered receiver, replaying events after a valid cursor.
    pub fn subscribe(
        &self,
        filter: SubscribeFilter,
        resume_stream_id: &str,
        after_event_id: u64,
    ) -> FilteredReceiver {
        // Subscribe before snapshotting history. Events racing with the snapshot
        // can appear twice, and `last_seen_event_id` removes that duplicate.
        let rx = self.inner.tx.subscribe();
        let after_event_id = if resume_stream_id == self.stream_id() {
            after_event_id
        } else {
            0
        };
        FilteredReceiver {
            rx,
            replay: replay_after(&self.inner, &filter, after_event_id),
            last_seen_event_id: after_event_id,
            filter,
            inner: self.inner.clone(),
        }
    }
}

fn lock_state(inner: &SessionEventHubInner) -> std::sync::MutexGuard<'_, SessionEventHubState> {
    match inner.state.lock() {
        Ok(state) => state,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn replay_after(
    inner: &SessionEventHubInner,
    filter: &SubscribeFilter,
    after_event_id: u64,
) -> VecDeque<SequencedSessionEvent> {
    lock_state(inner)
        .history
        .iter()
        .filter(|ev| ev.event_id > after_event_id && event_matches(filter, &ev.event))
        .cloned()
        .collect()
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
pub fn to_proto(msg: &SequencedSessionEvent, stream_id: &str) -> SessionEvent {
    let event = &msg.event;
    let payload = if let Some(ref mem) = event.memory_updated {
        Some(Payload::MemoryUpdated(MemoryUpdatedEvent {
            source: mem.source.clone(),
            target: mem.target.clone(),
            summary: mem.summary.clone(),
            live_written: mem.live_written,
        }))
    } else if let Some(ref pend) = event.pending_changed {
        Some(Payload::PendingChanged(PendingChangedEvent {
            pending_count: pend.pending_count,
            reason: pend.reason.clone(),
        }))
    } else if let Some(ref meta) = event.session_metadata_changed {
        Some(Payload::SessionMetadataChanged(
            SessionMetadataChangedEvent {
                title: meta.title.clone(),
            },
        ))
    } else {
        event.agent_thread_changed.as_ref().map(|thread| {
            Payload::AgentThreadChanged(AgentThreadChangedEvent {
                activity_sequence: thread.activity_sequence,
                root_thread_id: thread.root_thread_id.clone(),
                thread_id: thread.thread_id.clone(),
                parent_thread_id: thread.parent_thread_id.clone(),
                canonical_path: thread.canonical_path.clone(),
                task_name: thread.task_name.clone(),
                agent_type: thread.agent_type.clone(),
                session_id: thread.session_id.clone(),
                status_kind: thread.status_kind.clone(),
                status_payload_json: thread.status_payload_json.clone(),
                activity_kind: thread.activity_kind.clone(),
            })
        })
    };

    SessionEvent {
        session_id: event.session_id.clone().unwrap_or_default(),
        agent_id: event.agent_id.clone(),
        ts_ms: msg.ts_ms,
        event_id: msg.event_id,
        stream_id: stream_id.to_string(),
        payload,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn agent_thread_event(
        root_thread_id: &str,
        activity_sequence: u64,
        canonical_path: &str,
        status_kind: &str,
    ) -> SessionEventMsg {
        SessionEventMsg {
            session_id: Some(root_thread_id.to_string()),
            agent_id: "reviewer".into(),
            memory_updated: None,
            pending_changed: None,
            session_metadata_changed: None,
            agent_thread_changed: Some(AgentThreadChangedPayload {
                activity_sequence,
                root_thread_id: root_thread_id.to_string(),
                thread_id: "worker-thread".into(),
                parent_thread_id: root_thread_id.to_string(),
                canonical_path: canonical_path.into(),
                task_name: "worker".into(),
                agent_type: "reviewer".into(),
                session_id: "worker-session".into(),
                status_kind: status_kind.into(),
                status_payload_json: format!(r#"{{"kind":"{status_kind}"}}"#),
                activity_kind: "status_changed".into(),
            }),
        }
    }

    #[tokio::test]
    async fn agent_thread_events_replay_after_cursor_in_order() {
        let hub = SessionEventHub::new(16);
        hub.publish(agent_thread_event("root", 7, "/root/a", "running"));
        hub.publish(agent_thread_event("root", 8, "/root/a", "completed"));

        let mut rx = hub.subscribe(
            SubscribeFilter {
                session_id: Some("root".into()),
                agent_id: None,
            },
            hub.stream_id(),
            1,
        );
        let event = rx.recv().await.unwrap();

        assert_eq!(event.event_id, 2);
        assert_eq!(
            event
                .event
                .agent_thread_changed
                .as_ref()
                .unwrap()
                .activity_sequence,
            8
        );
    }

    #[test]
    fn agent_thread_events_only_match_their_root_session() {
        let event = agent_thread_event("root-a", 1, "/root/worker", "running");
        assert!(event_matches(
            &SubscribeFilter {
                session_id: Some("root-a".into()),
                agent_id: None,
            },
            &event,
        ));
        assert!(!event_matches(
            &SubscribeFilter {
                session_id: Some("root-b".into()),
                agent_id: None,
            },
            &event,
        ));
        assert!(!event_matches(&SubscribeFilter::default(), &event));
    }

    #[test]
    fn agent_thread_projection_maps_to_proto_without_loss() {
        let event = SequencedSessionEvent {
            event_id: 9,
            ts_ms: 10,
            event: agent_thread_event("root", 8, "/root/a", "completed"),
        };

        let proto = to_proto(&event, "stream");
        let Some(Payload::AgentThreadChanged(projection)) = proto.payload else {
            panic!("expected agent thread payload");
        };
        assert_eq!(projection.activity_sequence, 8);
        assert_eq!(projection.root_thread_id, "root");
        assert_eq!(projection.thread_id, "worker-thread");
        assert_eq!(projection.parent_thread_id, "root");
        assert_eq!(projection.canonical_path, "/root/a");
        assert_eq!(projection.task_name, "worker");
        assert_eq!(projection.agent_type, "reviewer");
        assert_eq!(projection.session_id, "worker-session");
        assert_eq!(projection.status_kind, "completed");
        assert_eq!(projection.status_payload_json, r#"{"kind":"completed"}"#);
        assert_eq!(projection.activity_kind, "status_changed");
    }

    #[tokio::test]
    async fn publish_reaches_matching_subscriber() {
        let hub = SessionEventHub::new(16);
        let mut rx = hub.subscribe(
            SubscribeFilter {
                session_id: Some("s1".into()),
                agent_id: None,
            },
            "",
            0,
        );
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
            agent_thread_changed: None,
        });
        let ev = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ev.event_id, 1);
        assert_eq!(ev.event.session_id.as_deref(), Some("s1"));
        assert!(ev.event.memory_updated.unwrap().live_written);
    }

    #[tokio::test]
    async fn empty_session_filter_receives_global_pending() {
        let hub = SessionEventHub::new(16);
        let mut rx = hub.subscribe(
            SubscribeFilter {
                session_id: None,
                agent_id: None,
            },
            "",
            0,
        );
        hub.publish(SessionEventMsg {
            session_id: None,
            agent_id: "workspace".into(),
            memory_updated: None,
            pending_changed: Some(PendingChangedPayload {
                pending_count: 2,
                reason: "enqueued".into(),
            }),
            session_metadata_changed: None,
            agent_thread_changed: None,
        });
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev.event.pending_changed.unwrap().pending_count, 2);
    }

    #[tokio::test]
    async fn reconnect_replays_only_events_after_cursor() {
        let hub = SessionEventHub::new(16);
        for summary in ["first", "second"] {
            hub.publish(SessionEventMsg {
                session_id: Some("s1".into()),
                agent_id: "workspace".into(),
                memory_updated: Some(MemoryUpdatedPayload {
                    source: "review".into(),
                    target: "memory".into(),
                    summary: summary.into(),
                    live_written: true,
                }),
                pending_changed: None,
                session_metadata_changed: None,
                agent_thread_changed: None,
            });
        }

        let mut rx = hub.subscribe(
            SubscribeFilter {
                session_id: Some("s1".into()),
                agent_id: None,
            },
            hub.stream_id(),
            1,
        );
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev.event_id, 2);
        assert_eq!(ev.event.memory_updated.unwrap().summary, "second");
    }

    #[tokio::test]
    async fn stale_stream_id_restarts_replay_from_available_history() {
        let hub = SessionEventHub::new(16);
        hub.publish(SessionEventMsg {
            session_id: Some("s1".into()),
            agent_id: "workspace".into(),
            memory_updated: Some(MemoryUpdatedPayload {
                source: "review".into(),
                target: "memory".into(),
                summary: "restored".into(),
                live_written: true,
            }),
            pending_changed: None,
            session_metadata_changed: None,
            agent_thread_changed: None,
        });

        let mut rx = hub.subscribe(
            SubscribeFilter {
                session_id: Some("s1".into()),
                agent_id: None,
            },
            "old-server-generation",
            99,
        );
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev.event_id, 1);
    }

    #[tokio::test]
    async fn concurrent_publishers_are_observed_in_event_id_order() {
        let hub = SessionEventHub::new(16);
        let mut rx = hub.subscribe_raw();
        let publishers: Vec<_> = (0..8)
            .map(|index| {
                let hub = hub.clone();
                std::thread::spawn(move || {
                    hub.publish(SessionEventMsg {
                        session_id: Some("s1".into()),
                        agent_id: format!("agent-{index}"),
                        memory_updated: None,
                        pending_changed: Some(PendingChangedPayload {
                            pending_count: index,
                            reason: "test".into(),
                        }),
                        session_metadata_changed: None,
                        agent_thread_changed: None,
                    });
                })
            })
            .collect();
        for publisher in publishers {
            publisher.join().unwrap();
        }

        for expected_id in 1..=8 {
            assert_eq!(rx.recv().await.unwrap().event_id, expected_id);
        }
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
            agent_thread_changed: None,
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
            agent_thread_changed: None,
        };
        assert!(event_matches(&filter, &ev));
    }
}
