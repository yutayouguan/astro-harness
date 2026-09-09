//! 会话级网络主机审批服务。
//!
//! 管理受管网络代理决策的 per-session 允许/拒绝缓存。
//! 每个 session 通过 `SessionServices` 持有一个服务实例；缓存不跨会话共享。

use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::oneshot;
use types::{NetworkApprovalContext, NetworkApprovalProtocol};

/// 已决主机审批决定的缓存 key。
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

/// 用于去重并发待审批请求的 key。
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct PendingHostApprovalKey {
    pub profile_id: String,
    pub host: String,
    pub protocol: NetworkApprovalProtocol,
}

/// 审批决定的作用范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalScope {
    Once,
    Session,
    Persistent,
}

/// 审阅者对待审批请求做出的决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingApprovalDecision {
    Allow(ApprovalScope),
    Deny,
}

/// 等待待审批结果的 waiter 集合。
struct PendingHostApproval {
    waiters: Vec<oneshot::Sender<PendingApprovalDecision>>,
    generation: u64,
}

/// 交给审阅者的句柄；未 resolve 就 drop 会将所有 waiter 标记为拒绝。
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
    /// 以指定决定解决此待审批请求。
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

/// 已缓存的决定结果。
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

/// 会话级网络审批服务。
///
/// 不可 `Clone` — 每个 session 通过 `SessionServices` 恰好持有一个实例。
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

    /// 检查该主机在本次会话中是否已被批准或拒绝。
    ///
    /// 拒绝缓存优先于允许缓存。
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

    /// 发起待审批请求，或加入已有的同 key 审批。
    ///
    /// 如果是该 key 的首个请求，返回 `Owner` — 调用方是审阅者，持有 `PendingHostApprovalOwner`。
    /// 如果已有其它请求在等待，返回 `Joined` — 调用方通过 receiver 等待共享决定。
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

    /// 持久化网络域名修订并在成功时更新会话缓存。
    ///
    /// 仅对自定义叶子 profile 的精确主机生效。失败时不修改
    /// 会话缓存 — fail-closed。
    pub fn persist_amendment(
        &self,
        memory_dir: &std::path::Path,
        profile_id: &str,
        amendment: &types::NetworkPolicyAmendment,
    ) -> anyhow::Result<()> {
        let action = amendment.action;
        memory::amend_network_domain(memory_dir, profile_id, &amendment.host, action)?;

        let key = HostApprovalKey {
            profile_id: profile_id.to_string(),
            host: amendment.host.clone(),
            protocol: types::NetworkApprovalProtocol::Https,
            port: 0,
        };
        match amendment.action {
            types::NetworkPolicyRuleAction::Allow => {
                self.inner
                    .session_allowed
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(key, ());
            }
            types::NetworkPolicyRuleAction::Deny => {
                self.inner
                    .session_denied
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(key, ());
            }
        }
        Ok(())
    }
}

/// `begin_or_join` 的返回结果。
pub enum BeginResult {
    /// 调用方是首个请求者，拥有审批控制权。
    Owner(PendingHostApprovalOwner),
    /// 已有其它请求在等待；通过 receiver 等待共享决定。
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

        // 先允许
        let result = service.begin_or_join(test_pending_key("conflict.example.com"));
        match result {
            BeginResult::Owner(o) => {
                o.resolve(PendingApprovalDecision::Allow(ApprovalScope::Session))
            }
            _ => panic!("expected owner"),
        }

        // 再拒绝
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

        // 第 1 代：创建并 drop
        let result1 = service.begin_or_join(key.clone());
        let owner1 = match result1 {
            BeginResult::Owner(o) => o,
            _ => panic!("expected owner"),
        };
        drop(owner1);

        // 第 2 代：新 owner 应正常工作
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

    #[test]
    fn persist_amendment_updates_session_cache_on_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            r#"
"#,
        )
        .unwrap();
        let service = NetworkApprovalService::new();

        service
            .persist_amendment(
                dir.path(),
                "custom-profile",
                &types::NetworkPolicyAmendment {
                    host: "api.example.com".into(),
                    action: types::NetworkPolicyRuleAction::Allow,
                },
            )
            .unwrap();

        let key = test_key("api.example.com");
        let key = HostApprovalKey {
            profile_id: "custom-profile".into(),
            ..key
        };
        assert_eq!(service.cached_decision(&key), Some(CachedDecision::Allowed));
    }

    #[test]
    fn persist_amendment_rejects_builtin_profiles() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            r#"
"#,
        )
        .unwrap();
        let service = NetworkApprovalService::new();

        let result = service.persist_amendment(
            dir.path(),
            types::WORKSPACE_PROFILE,
            &types::NetworkPolicyAmendment {
                host: "api.example.com".into(),
                action: types::NetworkPolicyRuleAction::Allow,
            },
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("builtin"));
    }

    #[test]
    fn persist_amendment_rejects_wildcard_hosts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            r#"
"#,
        )
        .unwrap();
        let service = NetworkApprovalService::new();

        let result = service.persist_amendment(
            dir.path(),
            "custom-profile",
            &types::NetworkPolicyAmendment {
                host: "*.example.com".into(),
                action: types::NetworkPolicyRuleAction::Allow,
            },
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("wildcard"));
    }

    #[test]
    fn persist_deny_amendment_enters_deny_cache() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            r#"
"#,
        )
        .unwrap();
        let service = NetworkApprovalService::new();

        service
            .persist_amendment(
                dir.path(),
                "custom-profile",
                &types::NetworkPolicyAmendment {
                    host: "blocked.example.com".into(),
                    action: types::NetworkPolicyRuleAction::Deny,
                },
            )
            .unwrap();

        let key = HostApprovalKey {
            profile_id: "custom-profile".into(),
            host: "blocked.example.com".into(),
            protocol: NetworkApprovalProtocol::Https,
            port: 443,
        };
        assert_eq!(service.cached_decision(&key), Some(CachedDecision::Denied));
    }

    #[test]
    fn persist_amendment_preserves_other_toml_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            r#""approvals" = { "mode" = "user" }
"#,
        )
        .unwrap();
        let service = NetworkApprovalService::new();

        service
            .persist_amendment(
                dir.path(),
                "net-profile",
                &types::NetworkPolicyAmendment {
                    host: "api.example.com".into(),
                    action: types::NetworkPolicyRuleAction::Allow,
                },
            )
            .unwrap();

        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        let doc = home::settings::read_document(&dir.path().join("config.toml")).unwrap();
        let approvals: std::collections::HashMap<String, String> =
            home::settings::get(&doc, &["approvals"]).unwrap().unwrap();
        assert_eq!(approvals.get("mode").map(String::as_str), Some("user"));
        assert!(text.contains(r#""approvals" = { "mode" = "user" }"#));
        assert!(
            text.contains("api.example.com"),
            "amendment missing: {text}"
        );
    }
}
