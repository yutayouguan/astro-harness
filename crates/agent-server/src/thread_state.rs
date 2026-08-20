use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use agent_protocol::{Event, EventMsg, TurnItem};
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};

use crate::transport::ConnectionGenerationKey;

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
    pub pending_background_turn_ids: Vec<String>,
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
                if self
                    .active
                    .as_ref()
                    .is_some_and(|turn| turn.id == started.turn_id)
                    || self.completed.iter().any(|turn| turn.id == started.turn_id)
                {
                    return;
                }
                // Codex finishes the previous pending turn before opening the next one,
                // preserving its last observed status and items when no terminal arrived.
                if let Some(turn) = self.active.take() {
                    self.completed.push(turn);
                }
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
            EventMsg::ItemCompleted(item)
                if matches!(
                    &item.item,
                    TurnItem::Extension(extension)
                        if matches!(
                            extension.namespace.as_str(),
                            "astro.agent_thread" | "astro.agent_thread_resync"
                        )
                ) => {}
            EventMsg::ItemCompleted(item) => {
                self.upsert_item(&item.turn_id, &item.item, "completed");
            }
            EventMsg::AgentMessageContentDelta(delta) => {
                self.append_text_delta(&delta.turn_id, &delta.item_id, &delta.delta, false);
            }
            EventMsg::ReasoningContentDelta(delta) => {
                self.append_text_delta(&delta.turn_id, &delta.item_id, &delta.delta, true);
            }
            EventMsg::TurnComplete(completed) => {
                let completes_active = self
                    .active
                    .as_ref()
                    .is_some_and(|turn| turn.id == completed.turn_id);
                if completes_active {
                    if let Some(mut turn) = self.active.take() {
                        Self::complete_turn(&mut turn, completed);
                        self.completed.push(turn);
                    }
                } else if let Some(turn) = self
                    .completed
                    .iter_mut()
                    .find(|turn| turn.id == completed.turn_id)
                {
                    Self::complete_turn(turn, completed);
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
                } else if let Some(turn_id) = &aborted.turn_id {
                    if let Some(turn) = self.completed.iter_mut().find(|turn| turn.id == *turn_id) {
                        turn.status = "aborted".into();
                    }
                }
            }
            _ => {}
        }
    }

    fn complete_turn(turn: &mut TurnSnapshot, completed: &agent_protocol::TurnCompleteEvent) {
        turn.status = if completed.error.is_some() {
            "failed".into()
        } else {
            "completed".into()
        };
        turn.last_agent_message = completed.last_agent_message.clone();
        turn.error = completed.error.clone();
    }

    fn upsert_item(&mut self, turn_id: &str, item: &TurnItem, status: &str) {
        let existing_turn = self
            .active
            .as_mut()
            .filter(|turn| turn.id == turn_id)
            .or_else(|| self.completed.iter_mut().find(|turn| turn.id == turn_id));
        let turn = match existing_turn {
            Some(turn) => turn,
            None if matches!(item, TurnItem::Extension(_)) => {
                self.completed.push(TurnSnapshot {
                    id: turn_id.into(),
                    status: "completed".into(),
                    items: Vec::new(),
                    last_agent_message: None,
                    error: None,
                });
                self.completed.last_mut().expect("synthetic turn inserted")
            }
            None => return,
        };
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

    fn append_text_delta(&mut self, turn_id: &str, item_id: &str, delta: &str, reasoning: bool) {
        let Some(item) = self
            .active
            .as_mut()
            .filter(|turn| turn.id == turn_id)
            .and_then(|turn| {
                turn.items
                    .iter_mut()
                    .find(|item| item.id == item_id && item.status == "in_progress")
            })
        else {
            return;
        };
        match (&mut item.item, reasoning) {
            (TurnItem::AgentMessage(message), false) | (TurnItem::Reasoning(message), true) => {
                message.content.push_str(delta)
            }
            _ => {}
        }
    }

    pub fn active_turn_snapshot(&self) -> Option<TurnSnapshot> {
        self.active.clone()
    }

    pub fn completed_turns(&self) -> &[TurnSnapshot] {
        &self.completed
    }

    pub fn contains_item_payload(&self, item_id: &str, payload_json: &str) -> bool {
        self.active.iter().chain(self.completed.iter()).any(|turn| {
            turn.items.iter().any(|item| {
                item.id == item_id
                    && serde_json::to_string(&item.item)
                        .is_ok_and(|serialized| serialized == payload_json)
            })
        })
    }
}

pub enum ListenerCommand {
    CoreEvent(Event),
    /// Runtime event observed by the production side-effect supervisor.
    ObservedCoreEvent(Event),
    Resume {
        subscription: ConnectionGenerationKey,
        include_turns: bool,
        reply: oneshot::Sender<ThreadSnapshot>,
    },
    Unsubscribe {
        subscription: ConnectionGenerationKey,
        reply: Option<oneshot::Sender<()>>,
    },
    WaitForExtension {
        waiter_id: uuid::Uuid,
        item_id: String,
        payload_json: String,
        reply: oneshot::Sender<()>,
    },
    CancelExtensionWaiter {
        waiter_id: uuid::Uuid,
    },
    #[cfg(test)]
    ExtensionWaiterCount {
        reply: oneshot::Sender<usize>,
    },
    ExpireBackgroundSink {
        turn_id: String,
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
    pub subscribers: HashMap<ConnectionId, ConnectionGenerationKey>,
    /// Terminal-time logical delivery targets retained only until `astro.background_complete`.
    pub background_extension_sinks: HashMap<String, HashSet<ConnectionId>>,
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
        let subscribers = state.lock().await.subscribers.keys().cloned().collect();
        subscribers
    }

    pub async fn unsubscribe(
        &self,
        thread_id: &str,
        subscription: ConnectionGenerationKey,
    ) -> bool {
        let Some(state) = self.get(thread_id).await else {
            return false;
        };
        let command_tx = state.lock().await.listener_command_tx.clone();
        let (reply, receive) = oneshot::channel();
        if command_tx
            .send(ListenerCommand::Unsubscribe {
                subscription,
                reply: Some(reply),
            })
            .is_err()
        {
            return false;
        }
        receive.await.is_ok()
    }

    pub async fn has_subscribers(&self, thread_id: &str) -> bool {
        let Some(state) = self.get(thread_id).await else {
            return false;
        };
        let has_subscribers = !state.lock().await.subscribers.is_empty();
        has_subscribers
    }

    pub async fn unsubscribe_all_detached(&self, subscription: &ConnectionGenerationKey) {
        let states = self
            .states
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for state in states {
            let command_tx = state.lock().await.listener_command_tx.clone();
            let _ = command_tx.send(ListenerCommand::Unsubscribe {
                subscription: subscription.clone(),
                reply: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{
        DeltaEvent, Event, EventMsg, ExtensionItem, ItemEvent, TextItem, TurnAbortReason,
        TurnAbortedEvent, TurnCompleteEvent, TurnItem, TurnStartedEvent,
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

    fn content_delta(turn_id: &str, item_id: &str, delta: &str, reasoning: bool) -> Event {
        let delta = DeltaEvent {
            turn_id: turn_id.into(),
            item_id: item_id.into(),
            delta: delta.into(),
        };
        Event {
            id: format!("delta-{turn_id}-{item_id}"),
            msg: if reasoning {
                EventMsg::ReasoningContentDelta(delta)
            } else {
                EventMsg::AgentMessageContentDelta(delta)
            },
        }
    }

    #[test]
    fn builder_accumulates_agent_delta_into_matching_active_item() {
        let mut builder = ThreadHistoryBuilder::default();
        builder.track(&started("turn-1"));
        builder.track(&item("turn-1", "item-1", "", false));
        builder.track(&content_delta("turn-1", "item-1", "lost", false));

        let active = builder.active_turn_snapshot().expect("active turn");
        assert!(matches!(
            &active.items[0].item,
            TurnItem::AgentMessage(message) if message.content == "lost"
        ));
    }

    #[test]
    fn agent_control_extensions_do_not_create_chat_turns() {
        let mut builder = ThreadHistoryBuilder::default();
        builder.track(&Event {
            id: "background".into(),
            msg: EventMsg::ItemCompleted(ItemEvent {
                turn_id: "thread-1:background".into(),
                item: TurnItem::Extension(ExtensionItem {
                    id: "agent-thread-1".into(),
                    namespace: "astro.agent_thread".into(),
                    payload: serde_json::json!({"activity_sequence": 1}),
                }),
            }),
        });
        assert!(builder.active_turn_snapshot().is_none());
        assert!(builder.completed_turns().is_empty());
    }

    #[test]
    fn builder_routes_delta_by_turn_and_item_then_accepts_completed_canonical() {
        let mut builder = ThreadHistoryBuilder::default();
        builder.track(&started("turn-1"));
        builder.track(&item("turn-1", "item-1", "", false));
        builder.track(&content_delta("turn-old", "item-1", "wrong-turn", false));
        builder.track(&content_delta("turn-1", "item-other", "wrong-item", false));
        builder.track(&content_delta("turn-1", "item-1", "draft", false));

        let active = builder.active_turn_snapshot().expect("active turn");
        assert!(matches!(
            &active.items[0].item,
            TurnItem::AgentMessage(message) if message.content == "draft"
        ));

        builder.track(&item("turn-1", "item-1", "canonical", true));
        builder.track(&content_delta("turn-1", "item-1", "-late", false));
        let active = builder.active_turn_snapshot().expect("active turn");
        assert!(matches!(
            &active.items[0].item,
            TurnItem::AgentMessage(message) if message.content == "canonical"
        ));
    }

    #[test]
    fn builder_accumulates_reasoning_delta_into_matching_active_item() {
        let mut builder = ThreadHistoryBuilder::default();
        builder.track(&started("turn-1"));
        let started_reasoning = ItemEvent {
            turn_id: "turn-1".into(),
            item: TurnItem::Reasoning(TextItem {
                id: "reasoning-1".into(),
                content: String::new(),
            }),
        };
        builder.track(&Event {
            id: "reasoning-started".into(),
            msg: EventMsg::ItemStarted(started_reasoning),
        });
        builder.track(&content_delta("turn-1", "reasoning-1", "thinking", true));

        let active = builder.active_turn_snapshot().expect("active turn");
        assert!(matches!(
            &active.items[0].item,
            TurnItem::Reasoning(reasoning) if reasoning.content == "thinking"
        ));
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

    #[test]
    fn repeated_start_is_idempotent_and_new_start_preserves_previous_turn() {
        let mut builder = ThreadHistoryBuilder::default();
        builder.track(&started("turn-1"));
        builder.track(&item("turn-1", "item-1", "running", false));

        builder.track(&started("turn-1"));
        assert_eq!(
            builder
                .active_turn_snapshot()
                .expect("repeated start should keep the active turn")
                .items
                .len(),
            1
        );
        assert!(builder.completed_turns().is_empty());

        builder.track(&started("turn-2"));
        assert_eq!(builder.completed_turns().len(), 1);
        assert_eq!(builder.completed_turns()[0].id, "turn-1");
        assert_eq!(builder.completed_turns()[0].status, "in_progress");
        assert_eq!(builder.completed_turns()[0].items.len(), 1);
        assert_eq!(
            builder
                .active_turn_snapshot()
                .expect("new turn should become active")
                .id,
            "turn-2"
        );
    }

    #[test]
    fn late_item_completion_updates_its_finished_turn() {
        let mut builder = ThreadHistoryBuilder::default();
        builder.track(&started("turn-1"));
        builder.track(&item("turn-1", "item-1", "running", false));
        builder.track(&Event {
            id: "turn-1".into(),
            msg: EventMsg::TurnComplete(TurnCompleteEvent {
                turn_id: "turn-1".into(),
                last_agent_message: None,
                error: None,
            }),
        });
        builder.track(&started("turn-2"));

        builder.track(&item("turn-1", "item-1", "late done", true));

        let old_turn = &builder.completed_turns()[0];
        assert_eq!(old_turn.id, "turn-1");
        assert_eq!(old_turn.items.len(), 1);
        assert_eq!(old_turn.items[0].status, "completed");
        assert!(matches!(
            &old_turn.items[0].item,
            TurnItem::AgentMessage(message) if message.content == "late done"
        ));
        assert!(builder
            .active_turn_snapshot()
            .expect("new turn should remain active")
            .items
            .is_empty());
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
                subscribers: subscribers
                    .iter()
                    .map(|id| {
                        let key = ConnectionGenerationKey::new(*id);
                        ((*id).to_string(), key)
                    })
                    .collect(),
                background_extension_sinks: HashMap::new(),
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
        let first = thread_state.lock().await.subscribers["first"].clone();
        let unsubscribe = tokio::spawn({
            let manager = manager.clone();
            let first = first.clone();
            async move { manager.unsubscribe("thread-1", first).await }
        });

        let command = commands.recv().await.expect("unsubscribe should be queued");
        match command {
            ListenerCommand::Unsubscribe {
                subscription,
                reply: Some(reply),
            } if subscription == first => reply.send(()).expect("unsubscribe ack"),
            _ => panic!("expected acknowledged unsubscribe"),
        }
        assert!(unsubscribe.await.expect("unsubscribe task"));
        assert!(thread_state.lock().await.subscribers.contains_key("first"));
        let missing = ConnectionGenerationKey::new("missing");
        let unsubscribe = tokio::spawn({
            let manager = manager.clone();
            let missing = missing.clone();
            async move { manager.unsubscribe("thread-1", missing).await }
        });
        match commands
            .recv()
            .await
            .expect("unknown subscriber removal should still be serialized")
        {
            ListenerCommand::Unsubscribe {
                subscription,
                reply: Some(reply),
            } if subscription == missing => reply.send(()).expect("unsubscribe ack"),
            _ => panic!("expected acknowledged unsubscribe"),
        }
        assert!(unsubscribe.await.expect("unsubscribe task"));
        assert!(!manager.unsubscribe("missing", first).await);
    }

    #[tokio::test]
    async fn manager_queues_unsubscribe_behind_pending_resume() {
        let manager = ThreadStateManager::default();
        let (thread_state, mut commands) = state(&[]);
        let command_tx = thread_state.lock().await.listener_command_tx.clone();
        manager.insert("thread-1".into(), thread_state).await;

        let (reply, _reply_rx) = tokio::sync::oneshot::channel();
        let first = ConnectionGenerationKey::new("first");
        command_tx
            .send(ListenerCommand::Resume {
                subscription: first.clone(),
                include_turns: false,
                reply,
            })
            .expect("resume should be queued");
        let unsubscribe = tokio::spawn({
            let manager = manager.clone();
            let first = first.clone();
            async move { manager.unsubscribe("thread-1", first).await }
        });

        assert!(matches!(
            commands.recv().await.expect("resume command should exist"),
            ListenerCommand::Resume { subscription, .. } if subscription == first
        ));
        match commands
            .recv()
            .await
            .expect("unsubscribe command should follow resume")
        {
            ListenerCommand::Unsubscribe {
                subscription,
                reply: Some(reply),
            } if subscription == first => reply.send(()).expect("unsubscribe ack"),
            _ => panic!("expected acknowledged unsubscribe"),
        }
        assert!(unsubscribe.await.expect("unsubscribe task"));
    }
}
