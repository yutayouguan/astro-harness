use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use agent_protocol::{Event, EventMsg, TurnItem};
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};

pub type ConnectionId = String;

#[derive(Debug, Clone, PartialEq)]
pub struct ItemSnapshot {
    pub id: String,
    pub status: String,
    pub item: TurnItem,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnSnapshot {
    pub id: String,
    pub status: String,
    pub items: Vec<ItemSnapshot>,
    pub last_agent_message: Option<String>,
    pub error: Option<agent_protocol::ErrorEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThreadSnapshot {
    pub thread_id: String,
    pub status: String,
    pub turns: Vec<TurnSnapshot>,
    pub active_turn: Option<TurnSnapshot>,
}

#[derive(Default)]
pub struct ThreadHistoryBuilder {
    active: Option<TurnSnapshot>,
    completed: Vec<TurnSnapshot>,
}

impl ThreadHistoryBuilder {
    pub fn track(&mut self, event: &Event) {
        match &event.msg {
            EventMsg::TurnStarted(started) => {
                self.active = Some(TurnSnapshot {
                    id: started.turn_id.clone(),
                    status: "in_progress".into(),
                    items: Vec::new(),
                    last_agent_message: None,
                    error: None,
                });
            }
            EventMsg::ItemStarted(item) => {
                self.upsert_item(&item.turn_id, &item.item, "in_progress");
            }
            EventMsg::ItemCompleted(item) => {
                self.upsert_item(&item.turn_id, &item.item, "completed");
            }
            EventMsg::TurnComplete(completed) => {
                if self
                    .active
                    .as_ref()
                    .is_some_and(|turn| turn.id == completed.turn_id)
                {
                    if let Some(mut turn) = self.active.take() {
                        turn.status = if completed.error.is_some() {
                            "failed".into()
                        } else {
                            "completed".into()
                        };
                        turn.last_agent_message = completed.last_agent_message.clone();
                        turn.error = completed.error.clone();
                        self.completed.push(turn);
                    }
                }
            }
            EventMsg::TurnAborted(aborted) => {
                let aborts_active = match (&self.active, &aborted.turn_id) {
                    (Some(active), Some(turn_id)) => active.id == *turn_id,
                    (Some(_), None) => true,
                    (None, _) => false,
                };
                if aborts_active {
                    if let Some(mut turn) = self.active.take() {
                        turn.status = "aborted".into();
                        self.completed.push(turn);
                    }
                }
            }
            _ => {}
        }
    }

    fn upsert_item(&mut self, turn_id: &str, item: &TurnItem, status: &str) {
        let Some(turn) = self.active.as_mut() else {
            return;
        };
        if turn.id != turn_id {
            return;
        }
        if let Some(existing) = turn.items.iter_mut().find(|entry| entry.id == item.id()) {
            existing.status = status.into();
            existing.item = item.clone();
        } else {
            turn.items.push(ItemSnapshot {
                id: item.id().into(),
                status: status.into(),
                item: item.clone(),
            });
        }
    }

    pub fn active_turn_snapshot(&self) -> Option<TurnSnapshot> {
        self.active.clone()
    }

    pub fn completed_turns(&self) -> &[TurnSnapshot] {
        &self.completed
    }
}

pub enum ListenerCommand {
    CoreEvent(Event),
    Resume {
        connection_id: ConnectionId,
        include_turns: bool,
        reply: oneshot::Sender<ThreadSnapshot>,
    },
    Unsubscribe {
        connection_id: ConnectionId,
    },
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadActivity {
    pub status: String,
    pub has_subscribers: bool,
}

pub struct ThreadState {
    pub status: String,
    pub history: ThreadHistoryBuilder,
    pub subscribers: HashSet<ConnectionId>,
    pub listener_command_tx: mpsc::UnboundedSender<ListenerCommand>,
    pub activity_tx: tokio::sync::watch::Sender<ThreadActivity>,
}

#[derive(Clone, Default)]
pub struct ThreadStateManager {
    states: Arc<RwLock<HashMap<String, Arc<Mutex<ThreadState>>>>>,
}

impl ThreadStateManager {
    pub async fn insert(
        &self,
        thread_id: String,
        state: Arc<Mutex<ThreadState>>,
    ) -> Option<Arc<Mutex<ThreadState>>> {
        self.states.write().await.insert(thread_id, state)
    }

    pub async fn get(&self, thread_id: &str) -> Option<Arc<Mutex<ThreadState>>> {
        self.states.read().await.get(thread_id).cloned()
    }

    pub async fn remove(&self, thread_id: &str) -> Option<Arc<Mutex<ThreadState>>> {
        self.states.write().await.remove(thread_id)
    }

    pub async fn subscribed_connection_ids(&self, thread_id: &str) -> Vec<ConnectionId> {
        let Some(state) = self.get(thread_id).await else {
            return Vec::new();
        };
        let subscribers = state.lock().await.subscribers.iter().cloned().collect();
        subscribers
    }

    pub async fn unsubscribe(&self, thread_id: &str, connection_id: &str) -> bool {
        let Some(state) = self.get(thread_id).await else {
            return false;
        };
        let command_tx = {
            let state = state.lock().await;
            if !state.subscribers.contains(connection_id) {
                return false;
            }
            state.listener_command_tx.clone()
        };
        command_tx
            .send(ListenerCommand::Unsubscribe {
                connection_id: connection_id.into(),
            })
            .is_ok()
    }

    pub async fn has_subscribers(&self, thread_id: &str) -> bool {
        let Some(state) = self.get(thread_id).await else {
            return false;
        };
        let has_subscribers = !state.lock().await.subscribers.is_empty();
        has_subscribers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{
        Event, EventMsg, ItemEvent, TextItem, TurnAbortReason, TurnAbortedEvent, TurnCompleteEvent,
        TurnItem, TurnStartedEvent,
    };
    use tokio::sync::{mpsc, watch, Mutex};

    fn started(turn_id: &str) -> Event {
        Event {
            id: turn_id.into(),
            msg: EventMsg::TurnStarted(TurnStartedEvent {
                turn_id: turn_id.into(),
            }),
        }
    }

    fn item(turn_id: &str, item_id: &str, content: &str, completed: bool) -> Event {
        let event = ItemEvent {
            turn_id: turn_id.into(),
            item: TurnItem::AgentMessage(TextItem {
                id: item_id.into(),
                content: content.into(),
            }),
        };
        Event {
            id: turn_id.into(),
            msg: if completed {
                EventMsg::ItemCompleted(event)
            } else {
                EventMsg::ItemStarted(event)
            },
        }
    }

    #[test]
    fn builder_tracks_active_item_then_completes_turn() {
        let mut builder = ThreadHistoryBuilder::default();
        builder.track(&started("turn-1"));
        builder.track(&item("turn-1", "item-1", "done", true));

        assert_eq!(
            builder
                .active_turn_snapshot()
                .expect("turn should remain active")
                .items
                .len(),
            1
        );

        builder.track(&Event {
            id: "turn-1".into(),
            msg: EventMsg::TurnComplete(TurnCompleteEvent {
                turn_id: "turn-1".into(),
                last_agent_message: Some("done".into()),
                error: None,
            }),
        });

        assert!(builder.active_turn_snapshot().is_none());
        assert_eq!(builder.completed_turns().len(), 1);
        assert_eq!(builder.completed_turns()[0].status, "completed");
    }

    #[test]
    fn builder_upserts_items_by_stable_id_and_tracks_abort() {
        let mut builder = ThreadHistoryBuilder::default();
        builder.track(&started("turn-1"));
        builder.track(&item("turn-1", "item-1", "running", false));
        builder.track(&item("turn-1", "item-1", "done", true));

        let active = builder
            .active_turn_snapshot()
            .expect("turn should remain active");
        assert_eq!(active.items.len(), 1);
        assert_eq!(active.items[0].id, "item-1");
        assert_eq!(active.items[0].status, "completed");

        builder.track(&Event {
            id: "turn-1".into(),
            msg: EventMsg::TurnAborted(TurnAbortedEvent {
                turn_id: Some("turn-1".into()),
                reason: TurnAbortReason::Interrupted,
            }),
        });
        assert!(builder.active_turn_snapshot().is_none());
        assert_eq!(builder.completed_turns()[0].status, "aborted");
    }

    fn state(
        subscribers: &[&str],
    ) -> (
        std::sync::Arc<Mutex<ThreadState>>,
        mpsc::UnboundedReceiver<ListenerCommand>,
    ) {
        let (listener_command_tx, listener_command_rx) = mpsc::unbounded_channel();
        let (activity_tx, _activity_rx) = watch::channel(ThreadActivity {
            status: "idle".into(),
            has_subscribers: !subscribers.is_empty(),
        });
        (
            std::sync::Arc::new(Mutex::new(ThreadState {
                status: "idle".into(),
                history: ThreadHistoryBuilder::default(),
                subscribers: subscribers.iter().map(|id| (*id).to_string()).collect(),
                listener_command_tx,
                activity_tx,
            })),
            listener_command_rx,
        )
    }

    #[tokio::test]
    async fn manager_inserts_gets_and_removes_thread_state() {
        let manager = ThreadStateManager::default();
        let (thread_state, _commands) = state(&[]);

        assert!(manager
            .insert("thread-1".into(), thread_state.clone())
            .await
            .is_none());
        assert!(std::sync::Arc::ptr_eq(
            &manager
                .get("thread-1")
                .await
                .expect("inserted state should be available"),
            &thread_state
        ));
        assert!(std::sync::Arc::ptr_eq(
            &manager
                .remove("thread-1")
                .await
                .expect("inserted state should be removable"),
            &thread_state
        ));
        assert!(manager.get("thread-1").await.is_none());
    }

    #[tokio::test]
    async fn manager_reads_subscribers_and_queues_unsubscribe() {
        let manager = ThreadStateManager::default();
        let (thread_state, mut commands) = state(&["first", "second"]);
        manager
            .insert("thread-1".into(), thread_state.clone())
            .await;

        let mut ids = manager.subscribed_connection_ids("thread-1").await;
        ids.sort();
        assert_eq!(ids, ["first", "second"]);
        assert!(manager.has_subscribers("thread-1").await);
        assert!(manager.unsubscribe("thread-1", "first").await);

        let command = commands.recv().await.expect("unsubscribe should be queued");
        assert!(matches!(
            command,
            ListenerCommand::Unsubscribe { connection_id } if connection_id == "first"
        ));
        assert!(thread_state.lock().await.subscribers.contains("first"));
        assert!(!manager.unsubscribe("thread-1", "missing").await);
        assert!(!manager.unsubscribe("missing", "first").await);
    }
}
