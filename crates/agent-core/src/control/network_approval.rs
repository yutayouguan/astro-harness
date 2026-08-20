//! Session-scoped network host approval service.
//!
//! Manages per-session allow/deny caches for managed network proxy decisions.
//! Each session owns one service instance through `SessionServices`; caches are
//! never shared across sessions.

use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::oneshot;
use types::{NetworkApprovalContext, NetworkApprovalProtocol};

/// Cache key for resolved host approval decisions.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct HostApprovalKey {
    pub profile_id: String,
    pub host: String,
    pub protocol: NetworkApprovalProtocol,
    pub port: u16,
}

impl HostApprovalKey {
    pub fn new(profile_id: &str, context: &NetworkApprovalContext, port: u16) -> Self {
        Self {
            profile_id: profile_id.to_string(),
            host: context.host.clone(),
            protocol: context.protocol,
            port,
        }
    }
}

/// Key for deduplicating concurrent pending approval requests.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct PendingHostApprovalKey {
    pub profile_id: String,
    pub host: String,
    pub protocol: NetworkApprovalProtocol,
}

/// The scope of an approval decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalScope {
    Once,
    Session,
    Persistent,
}

/// The decision rendered by a reviewer for a pending approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingApprovalDecision {
    Allow(ApprovalScope),
    Deny,
}

/// Waiters that are parked on a pending approval.
struct PendingHostApproval {
    waiters: Vec<oneshot::Sender<PendingApprovalDecision>>,
    generation: u64,
}

/// Handle given to the reviewer; dropping it without resolving fails all waiters closed.
pub struct PendingHostApprovalOwner {
    key: PendingHostApprovalKey,
    generation: u64,
    service: std::sync::Weak<NetworkApprovalServiceInner>,
}

impl Drop for PendingHostApprovalOwner {
    fn drop(&mut self) {
        if let Some(inner) = self.service.upgrade() {
            let mut pending = inner.pending.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = pending.get(&self.key) {
                if entry.generation == self.generation {
                    if let Some(entry) = pending.remove(&self.key) {
                        for waiter in entry.waiters {
                            let _ = waiter.send(PendingApprovalDecision::Deny);
                        }
                    }
                }
            }
        }
    }
}

impl PendingHostApprovalOwner {
    /// Resolve this pending approval with a decision.
    pub fn resolve(self, decision: PendingApprovalDecision) {
        if let Some(inner) = self.service.upgrade() {
            let mut pending = inner.pending.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = pending.get(&self.key) {
                if entry.generation == self.generation {
                    let entry = pending.remove(&self.key).unwrap();

                    match &decision {
                        PendingApprovalDecision::Allow(ApprovalScope::Session)
                        | PendingApprovalDecision::Allow(ApprovalScope::Persistent) => {
                            let mut allowed = inner
                                .session_allowed
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            allowed.insert(self.key.clone().into_host_key(), ());
                        }
                        PendingApprovalDecision::Deny => {
                            let mut denied = inner
                                .session_denied
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            denied.insert(self.key.clone().into_host_key(), ());
                        }
                        PendingApprovalDecision::Allow(ApprovalScope::Once) => {}
                    }

                    for waiter in entry.waiters {
                        let _ = waiter.send(decision.clone());
                    }
                }
            }
        }
        std::mem::forget(self);
    }
}

impl PendingHostApprovalKey {
    fn into_host_key(self) -> HostApprovalKey {
        HostApprovalKey {
            profile_id: self.profile_id,
            host: self.host,
            protocol: self.protocol,
            port: 0,
        }
    }
}

/// Cached decision result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachedDecision {
    Allowed,
    Denied,
}

struct NetworkApprovalServiceInner {
    session_allowed: Mutex<HashMap<HostApprovalKey, ()>>,
    session_denied: Mutex<HashMap<HostApprovalKey, ()>>,
    pending: Mutex<HashMap<PendingHostApprovalKey, PendingHostApproval>>,
    generation: Mutex<u64>,
}

/// Session-scoped network approval service.
///
/// Not `Clone` — each session owns exactly one through `SessionServices`.
pub struct NetworkApprovalService {
    inner: std::sync::Arc<NetworkApprovalServiceInner>,
}

impl Default for NetworkApprovalService {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkApprovalService {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(NetworkApprovalServiceInner {
                session_allowed: Mutex::new(HashMap::new()),
                session_denied: Mutex::new(HashMap::new()),
                pending: Mutex::new(HashMap::new()),
                generation: Mutex::new(0),
            }),
        }
    }

    /// Check if this host has already been approved or denied in this session.
    ///
    /// Deny cache takes priority over allow cache.
    pub fn cached_decision(&self, key: &HostApprovalKey) -> Option<CachedDecision> {
        let profile_key = HostApprovalKey {
            port: 0,
            ..key.clone()
        };
        let denied = self
            .inner
            .session_denied
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if denied.contains_key(&profile_key) {
            return Some(CachedDecision::Denied);
        }
        drop(denied);

        let allowed = self
            .inner
            .session_allowed
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if allowed.contains_key(&profile_key) {
            return Some(CachedDecision::Allowed);
        }
        None
    }

    /// Begin a pending approval or join an existing one for the same key.
    ///
    /// Returns `Ok((owner, None))` if this is the first request for the key —
    /// the caller is the reviewer and owns the `PendingHostApprovalOwner`.
    /// Returns `Ok((waiter_rx, Some(rx)))` if another request is already pending —
    /// the caller parks on the receiver.
    pub fn begin_or_join(&self, pending_key: PendingHostApprovalKey) -> BeginResult {
        let mut pending = self.inner.pending.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(entry) = pending.get_mut(&pending_key) {
            let (tx, rx) = oneshot::channel();
            entry.waiters.push(tx);
            return BeginResult::Joined(rx);
        }

        let mut gen = self
            .inner
            .generation
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *gen += 1;
        let generation = *gen;
        drop(gen);

        pending.insert(
            pending_key.clone(),
            PendingHostApproval {
                waiters: Vec::new(),
                generation,
            },
        );

        let owner = PendingHostApprovalOwner {
            key: pending_key,
            generation,
            service: std::sync::Arc::downgrade(&self.inner),
        };

        BeginResult::Owner(owner)
    }
}

/// Result of `begin_or_join`.
pub enum BeginResult {
    /// Caller is the first requester and owns the approval.
    Owner(PendingHostApprovalOwner),
    /// Another request is pending; await the receiver for the shared decision.
    Joined(oneshot::Receiver<PendingApprovalDecision>),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key(host: &str) -> HostApprovalKey {
        HostApprovalKey {
            profile_id: "test-profile".into(),
            host: host.into(),
            protocol: NetworkApprovalProtocol::Https,
            port: 443,
        }
    }

    fn test_pending_key(host: &str) -> PendingHostApprovalKey {
        PendingHostApprovalKey {
            profile_id: "test-profile".into(),
            host: host.into(),
            protocol: NetworkApprovalProtocol::Https,
        }
    }

    #[test]
    fn allow_once_does_not_enter_session_cache() {
        let service = NetworkApprovalService::new();
        let key = test_pending_key("example.com");
        let host_key = test_key("example.com");

        let result = service.begin_or_join(key);
        let owner = match result {
            BeginResult::Owner(o) => o,
            BeginResult::Joined(_) => panic!("expected owner"),
        };
        owner.resolve(PendingApprovalDecision::Allow(ApprovalScope::Once));

        assert!(service.cached_decision(&host_key).is_none());
    }

    #[test]
    fn allow_for_session_enters_cache_and_matches_same_profile_host_protocol() {
        let service = NetworkApprovalService::new();
        let key = test_pending_key("api.example.com");
        let host_key = test_key("api.example.com");

        let result = service.begin_or_join(key);
        let owner = match result {
            BeginResult::Owner(o) => o,
            BeginResult::Joined(_) => panic!("expected owner"),
        };
        owner.resolve(PendingApprovalDecision::Allow(ApprovalScope::Session));

        assert_eq!(
            service.cached_decision(&host_key),
            Some(CachedDecision::Allowed)
        );

        let different_host = test_key("other.example.com");
        assert!(service.cached_decision(&different_host).is_none());

        let different_profile = HostApprovalKey {
            profile_id: "other-profile".into(),
            ..test_key("api.example.com")
        };
        assert!(service.cached_decision(&different_profile).is_none());
    }

    #[test]
    fn deny_cache_takes_priority_over_allow_cache() {
        let service = NetworkApprovalService::new();

        // First allow
        let result = service.begin_or_join(test_pending_key("conflict.example.com"));
        match result {
            BeginResult::Owner(o) => {
                o.resolve(PendingApprovalDecision::Allow(ApprovalScope::Session))
            }
            _ => panic!("expected owner"),
        }

        // Then deny
        let result = service.begin_or_join(test_pending_key("conflict.example.com"));
        match result {
            BeginResult::Owner(o) => o.resolve(PendingApprovalDecision::Deny),
            _ => panic!("expected owner"),
        }

        assert_eq!(
            service.cached_decision(&test_key("conflict.example.com")),
            Some(CachedDecision::Denied)
        );
    }

    #[tokio::test]
    async fn concurrent_requests_for_same_key_only_trigger_one_reviewer() {
        let service = NetworkApprovalService::new();
        let key = test_pending_key("shared.example.com");

        let result1 = service.begin_or_join(key.clone());
        let owner = match result1 {
            BeginResult::Owner(o) => o,
            BeginResult::Joined(_) => panic!("first request should be owner"),
        };

        let result2 = service.begin_or_join(key.clone());
        let rx = match result2 {
            BeginResult::Joined(rx) => rx,
            BeginResult::Owner(_) => panic!("second request should join"),
        };

        owner.resolve(PendingApprovalDecision::Allow(ApprovalScope::Session));

        let decision = rx.await.unwrap();
        assert_eq!(
            decision,
            PendingApprovalDecision::Allow(ApprovalScope::Session)
        );
    }

    #[tokio::test]
    async fn owner_drop_fails_all_waiters_closed() {
        let service = NetworkApprovalService::new();
        let key = test_pending_key("dropped.example.com");

        let result1 = service.begin_or_join(key.clone());
        let owner = match result1 {
            BeginResult::Owner(o) => o,
            BeginResult::Joined(_) => panic!("expected owner"),
        };

        let result2 = service.begin_or_join(key.clone());
        let rx = match result2 {
            BeginResult::Joined(rx) => rx,
            BeginResult::Owner(_) => panic!("expected joiner"),
        };

        drop(owner);

        let decision = rx.await.unwrap();
        assert_eq!(decision, PendingApprovalDecision::Deny);
    }

    #[tokio::test]
    async fn owner_drop_does_not_affect_new_generation() {
        let service = NetworkApprovalService::new();
        let key = test_pending_key("regen.example.com");

        // Generation 1: create and drop
        let result1 = service.begin_or_join(key.clone());
        let owner1 = match result1 {
            BeginResult::Owner(o) => o,
            _ => panic!("expected owner"),
        };
        drop(owner1);

        // Generation 2: new owner should work
        let result2 = service.begin_or_join(key.clone());
        let owner2 = match result2 {
            BeginResult::Owner(o) => o,
            _ => panic!("expected new owner after drop"),
        };

        let result3 = service.begin_or_join(key.clone());
        let rx = match result3 {
            BeginResult::Joined(rx) => rx,
            _ => panic!("expected joiner"),
        };

        owner2.resolve(PendingApprovalDecision::Allow(ApprovalScope::Once));
        let decision = rx.await.unwrap();
        assert_eq!(
            decision,
            PendingApprovalDecision::Allow(ApprovalScope::Once)
        );
    }
}
