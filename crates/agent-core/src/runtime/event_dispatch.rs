//! Ordered persistence and delivery of runtime events.
//!
//! Keeping this boundary separate from [`Session`](super::Session) makes the
//! ordering contract explicit: durable events reach the rollout before live
//! subscribers observe them.

use agent_protocol::{Event, EventMsg};
use agent_rollout::RolloutItem;

use super::{event_identity, AgentStatus, Session};

impl Session {
    pub(crate) fn close_event_stream(&self) {
        self.hook_runtime()
            .remove_run_observer(self.session_id.as_str());
        if let Some(bindings) = self.runtime_io.get() {
            bindings.event_tx.close();
        }
    }

    /// Persist a unified event according to rollout policy before delivering it.
    pub async fn send_event(&self, turn_id: &str, mut msg: EventMsg) {
        let event_id = event_identity::normalize_event_msg(&mut msg, turn_id);
        let event = Event { id: event_id, msg };
        self.send_event_raw_with_persistence_and_hook(event, turn_id, true, async {})
            .await;
    }

    /// Send an already-normalized event while routing exact-turn listeners by
    /// the original internal turn id.
    pub(crate) async fn send_prepared_event(&self, raw_turn_id: &str, msg: EventMsg) {
        let event = Event {
            id: event_identity::event_turn_id(raw_turn_id),
            msg,
        };
        self.send_event_raw_with_persistence_and_hook(event, raw_turn_id, true, async {})
            .await;
    }

    pub(crate) async fn subscribe_turn_events(
        &self,
        turn_id: &str,
    ) -> async_channel::Receiver<Event> {
        let (tx, rx) = async_channel::unbounded();
        self.turn_event_taps
            .lock()
            .await
            .entry(turn_id.to_string())
            .or_default()
            .push(tx);
        rx
    }

    pub(crate) async fn remove_turn_event_taps(&self, turn_id: &str) {
        self.turn_event_taps.lock().await.remove(turn_id);
    }

    pub(crate) async fn send_event_raw_with_persistence(&self, event: Event, persist: bool) {
        let route_id = event.id.clone();
        self.send_event_raw_with_persistence_and_hook(event, &route_id, persist, async {})
            .await;
    }

    async fn send_event_raw_with_persistence_and_hook<F>(
        &self,
        event: Event,
        route_id: &str,
        persist: bool,
        after_persist: F,
    ) where
        F: std::future::Future<Output = ()>,
    {
        let _dispatch = self.event_dispatch.lock().await;
        if persist {
            if let Some(bindings) = self.runtime_io.get() {
                if let Err(error) = bindings
                    .rollout
                    .record(vec![RolloutItem::EventMsg(event.msg.clone())])
                    .await
                {
                    tracing::warn!(%error, event_id = %event.id, "failed to persist event");
                }
            }
        }
        after_persist.await;
        self.deliver_event_raw_inner(event, route_id).await;
    }

    #[cfg(test)]
    pub(super) async fn send_event_with_after_persist_hook<F>(
        &self,
        turn_id: &str,
        msg: EventMsg,
        after_persist: F,
    ) where
        F: std::future::Future<Output = ()>,
    {
        let mut msg = msg;
        let event = Event {
            id: event_identity::normalize_event_msg(&mut msg, turn_id),
            msg,
        };
        self.send_event_raw_with_persistence_and_hook(event, turn_id, true, after_persist)
            .await;
    }

    pub(crate) async fn deliver_event_raw(&self, event: Event) {
        let route_id = event.id.clone();
        let _dispatch = self.event_dispatch.lock().await;
        self.deliver_event_raw_inner(event, &route_id).await;
    }

    async fn deliver_event_raw_inner(&self, event: Event, route_id: &str) {
        if let Some(bindings) = self.runtime_io.get() {
            match &event.msg {
                EventMsg::TurnStarted(started) => {
                    let _ = bindings.status_tx.send(AgentStatus::Running {
                        turn_id: started.turn_id.clone(),
                    });
                }
                EventMsg::TurnComplete(_) | EventMsg::TurnAborted(_) => {
                    let _ = bindings.status_tx.send(AgentStatus::Idle);
                }
                EventMsg::ShutdownComplete => {
                    let _ = bindings.status_tx.send(AgentStatus::Shutdown);
                }
                _ => {}
            }
            let _ = bindings.event_tx.send(event.clone()).await;
        }
        let exact_turn_senders = {
            let mut taps = self.turn_event_taps.lock().await;
            if event.msg.is_terminal() {
                taps.remove(route_id).unwrap_or_default()
            } else {
                taps.get_mut(route_id)
                    .map(|senders| {
                        senders.retain(|sender| !sender.is_closed());
                        senders.clone()
                    })
                    .unwrap_or_default()
            }
        };
        for sender in exact_turn_senders {
            let _ = sender.send(event.clone()).await;
        }
    }
}
