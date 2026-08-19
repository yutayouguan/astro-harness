use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::watch;

use crate::AgentThreadV2;

const MAX_BUFFERED_ACTIVITIES: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActivityCursor(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentActivityKind {
    Spawned { thread_id: String },
    Mailbox { thread_id: String },
    StatusChanged { thread_id: String },
    EdgeClosed { thread_id: String },
    MainSteer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentActivity {
    pub sequence: u64,
    pub kind: AgentActivityKind,
    pub thread: Option<AgentThreadV2>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityObservation {
    Activity(Box<AgentActivity>),
    Gap {
        oldest: ActivityCursor,
        latest: ActivityCursor,
    },
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DurableModelActivity {
    Mailbox,
    Steer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelWaitSignal {
    Activity(Box<AgentActivity>),
    Durable(DurableModelActivity),
}

pub struct ActivityBus {
    sequence: AtomicU64,
    tx: watch::Sender<ActivityCursor>,
    events: Mutex<VecDeque<AgentActivity>>,
}

impl Default for ActivityBus {
    fn default() -> Self {
        let (tx, _rx) = watch::channel(ActivityCursor(0));
        Self {
            sequence: AtomicU64::new(0),
            tx,
            events: Mutex::new(VecDeque::new()),
        }
    }
}

impl ActivityBus {
    pub fn cursor(&self) -> ActivityCursor {
        ActivityCursor(self.sequence.load(Ordering::Acquire))
    }

    pub fn publish(&self, kind: AgentActivityKind, thread: Option<AgentThreadV2>) -> AgentActivity {
        let mut events = self.lock_events();
        let sequence = self.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        let activity = AgentActivity {
            sequence,
            kind,
            thread,
        };
        events.push_back(activity.clone());
        while events.len() > MAX_BUFFERED_ACTIVITIES {
            events.pop_front();
        }
        self.tx.send_replace(ActivityCursor(sequence));
        activity
    }

    pub async fn wait_after(
        &self,
        cursor: ActivityCursor,
        wait_timeout: Duration,
    ) -> Option<AgentActivity> {
        let mut rx = self.tx.subscribe();
        if let Some(activity) = self.first_after(cursor) {
            return Some(activity);
        }

        tokio::time::timeout(wait_timeout, async {
            loop {
                if rx.changed().await.is_err() {
                    return None;
                }
                if let Some(activity) = self.first_after(cursor) {
                    return Some(activity);
                }
            }
        })
        .await
        .ok()
        .flatten()
    }

    /// Wait for activity that is visible to the Codex V2 model wait tool.
    /// Spawn/start noise is deliberately skipped; durable mailbox delivery,
    /// final lifecycle changes, and main-task steering wake the caller.
    pub(crate) async fn wait_model_after<F>(
        &self,
        cursor: ActivityCursor,
        wait_timeout: Duration,
        caller_thread_id: &str,
        caller_is_root: bool,
        mut durable_activity: F,
    ) -> anyhow::Result<Option<ModelWaitSignal>>
    where
        F: FnMut() -> anyhow::Result<Option<DurableModelActivity>>,
    {
        let mut rx = self.tx.subscribe();
        if let Some(signal) = self.first_model_after(
            cursor,
            caller_thread_id,
            caller_is_root,
            durable_activity()?,
        ) {
            return Ok(Some(signal));
        }

        let signal: anyhow::Result<Option<ModelWaitSignal>> =
            tokio::time::timeout(wait_timeout, async {
                loop {
                    if rx.changed().await.is_err() {
                        return Ok(None);
                    }
                    if let Some(signal) = self.first_model_after(
                        cursor,
                        caller_thread_id,
                        caller_is_root,
                        durable_activity()?,
                    ) {
                        return Ok(Some(signal));
                    }
                }
            })
            .await
            .unwrap_or(Ok(None));
        signal
    }

    /// Returns the earliest buffered activity after `cursor` without applying
    /// the model wait path's `MainSteer` priority. This is the read-only
    /// observer surface used by projection streams that must preserve order.
    pub async fn next_after(
        &self,
        cursor: ActivityCursor,
        wait_timeout: Duration,
    ) -> Option<AgentActivity> {
        let mut rx = self.tx.subscribe();
        if let Some(activity) = self.first_after_in_order(cursor) {
            return Some(activity);
        }

        tokio::time::timeout(wait_timeout, async {
            loop {
                if rx.changed().await.is_err() {
                    return None;
                }
                if let Some(activity) = self.first_after_in_order(cursor) {
                    return Some(activity);
                }
            }
        })
        .await
        .ok()
        .flatten()
    }

    /// Observes the ordered activity stream while explicitly reporting when
    /// the requested cursor has fallen behind the bounded retention window.
    pub async fn observe_after(
        &self,
        cursor: ActivityCursor,
        wait_timeout: Duration,
    ) -> ActivityObservation {
        let mut rx = self.tx.subscribe();
        if let Some(observation) = self.observation_after(cursor) {
            return observation;
        }

        tokio::time::timeout(wait_timeout, async {
            loop {
                if rx.changed().await.is_err() {
                    return ActivityObservation::TimedOut;
                }
                if let Some(observation) = self.observation_after(cursor) {
                    return observation;
                }
            }
        })
        .await
        .unwrap_or(ActivityObservation::TimedOut)
    }

    fn first_after(&self, cursor: ActivityCursor) -> Option<AgentActivity> {
        let events = self.lock_events();
        let mut first = None;
        for activity in events.iter().filter(|event| event.sequence > cursor.0) {
            if activity.kind == AgentActivityKind::MainSteer {
                return Some(activity.clone());
            }
            if first.is_none() {
                first = Some(activity.clone());
            }
        }
        first
    }

    fn first_after_in_order(&self, cursor: ActivityCursor) -> Option<AgentActivity> {
        self.lock_events()
            .iter()
            .find(|event| event.sequence > cursor.0)
            .cloned()
    }

    fn first_model_after(
        &self,
        cursor: ActivityCursor,
        caller_thread_id: &str,
        caller_is_root: bool,
        durable_activity: Option<DurableModelActivity>,
    ) -> Option<ModelWaitSignal> {
        let events = self.lock_events();
        if caller_is_root {
            if let Some(activity) = events
                .iter()
                .filter(|event| event.sequence > cursor.0)
                .find(|activity| activity.kind == AgentActivityKind::MainSteer)
            {
                return Some(ModelWaitSignal::Activity(Box::new(activity.clone())));
            }
        }
        if durable_activity == Some(DurableModelActivity::Steer) {
            return Some(ModelWaitSignal::Durable(DurableModelActivity::Steer));
        }
        for activity in events.iter().filter(|event| event.sequence > cursor.0) {
            let visible = match (&activity.kind, activity.thread.as_ref()) {
                (AgentActivityKind::Mailbox { thread_id }, _) => {
                    thread_id == caller_thread_id && durable_activity.is_some()
                }
                (AgentActivityKind::StatusChanged { .. }, Some(thread)) => {
                    !matches!(
                        thread.status,
                        crate::AgentStatusV2::PendingInit | crate::AgentStatusV2::Running
                    ) && thread.parent_thread_id.as_deref() == Some(caller_thread_id)
                }
                (AgentActivityKind::EdgeClosed { .. }, Some(thread)) => {
                    thread.parent_thread_id.as_deref() == Some(caller_thread_id)
                }
                _ => false,
            };
            if visible {
                return Some(ModelWaitSignal::Activity(Box::new(activity.clone())));
            }
        }
        durable_activity.map(ModelWaitSignal::Durable)
    }

    fn observation_after(&self, cursor: ActivityCursor) -> Option<ActivityObservation> {
        let events = self.lock_events();
        let oldest = events.front()?;
        let latest = events.back()?;
        if cursor.0.saturating_add(1) < oldest.sequence {
            return Some(ActivityObservation::Gap {
                oldest: ActivityCursor(oldest.sequence),
                latest: ActivityCursor(latest.sequence),
            });
        }
        events
            .iter()
            .find(|event| event.sequence > cursor.0)
            .cloned()
            .map(Box::new)
            .map(ActivityObservation::Activity)
    }

    fn lock_events(&self) -> MutexGuard<'_, VecDeque<AgentActivity>> {
        match self.events.lock() {
            Ok(events) => events,
            Err(poisoned) => {
                tracing::warn!("recovering poisoned agent activity buffer");
                poisoned.into_inner()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn cursor_is_monotonic_and_wait_observes_coalesced_activity() {
        let bus = ActivityBus::default();
        let start = bus.cursor();
        let status = bus.publish(
            AgentActivityKind::StatusChanged {
                thread_id: "worker".into(),
            },
            None,
        );
        let mailbox = bus.publish(
            AgentActivityKind::Mailbox {
                thread_id: "worker".into(),
            },
            None,
        );

        assert!(status.sequence > start.0);
        assert!(mailbox.sequence > status.sequence);
        assert_eq!(bus.cursor(), ActivityCursor(mailbox.sequence));
        assert_eq!(
            bus.wait_after(start, Duration::from_millis(10))
                .await
                .unwrap(),
            status
        );
        assert_eq!(
            bus.wait_after(ActivityCursor(status.sequence), Duration::from_millis(10))
                .await
                .unwrap(),
            mailbox
        );
    }

    #[tokio::test]
    async fn wait_after_times_out_without_new_activity() {
        let bus = ActivityBus::default();
        assert_eq!(
            bus.wait_after(bus.cursor(), Duration::from_millis(1)).await,
            None
        );
    }

    #[tokio::test]
    async fn main_steer_takes_priority_over_earlier_ordinary_activity() {
        let bus = ActivityBus::default();
        let cursor = bus.cursor();
        bus.publish(
            AgentActivityKind::Mailbox {
                thread_id: "worker".into(),
            },
            None,
        );
        let steer = bus.publish(AgentActivityKind::MainSteer, None);

        assert_eq!(
            bus.wait_after(cursor, Duration::from_millis(10)).await,
            Some(steer)
        );
    }

    #[tokio::test]
    async fn observer_next_after_preserves_every_activity_in_sequence_order() {
        let bus = ActivityBus::default();
        let start = bus.cursor();
        let status = bus.publish(
            AgentActivityKind::StatusChanged {
                thread_id: "worker".into(),
            },
            None,
        );
        let steer = bus.publish(AgentActivityKind::MainSteer, None);

        assert_eq!(
            bus.next_after(start, Duration::from_millis(10))
                .await
                .unwrap(),
            status
        );
        assert_eq!(
            bus.next_after(ActivityCursor(status.sequence), Duration::from_millis(10))
                .await
                .unwrap(),
            steer
        );
    }

    #[tokio::test]
    async fn observer_cursor_advances_after_lag_and_does_not_regress_after_timeout() {
        let bus = ActivityBus::default();
        for _ in 0..(MAX_BUFFERED_ACTIVITIES + 32) {
            bus.publish(AgentActivityKind::MainSteer, None);
        }

        let recovered = bus
            .next_after(ActivityCursor(1), Duration::from_millis(10))
            .await
            .unwrap();
        assert!(recovered.sequence > 1);

        let end = bus.cursor();
        assert_eq!(bus.next_after(end, Duration::from_millis(1)).await, None);
        let fresh = bus.publish(AgentActivityKind::MainSteer, None);
        assert_eq!(
            bus.next_after(end, Duration::from_millis(10))
                .await
                .unwrap(),
            fresh
        );
        assert!(fresh.sequence > end.0);
    }

    #[tokio::test]
    async fn observer_reports_retention_gap_instead_of_silently_returning_tail() {
        let bus = ActivityBus::default();
        for _ in 0..(MAX_BUFFERED_ACTIVITIES + 8) {
            bus.publish(AgentActivityKind::MainSteer, None);
        }

        let observation = bus
            .observe_after(ActivityCursor(0), Duration::from_millis(10))
            .await;
        let ActivityObservation::Gap { oldest, latest } = observation else {
            panic!("expected activity retention gap");
        };
        assert!(oldest.0 > 1);
        assert_eq!(latest, bus.cursor());
    }
}
