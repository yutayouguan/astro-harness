//! 会话级审批缓存：同一会话内，用户已批准的命令模式不再重复弹窗。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;

/// 缓存 key：按 `(tool_name, command_prefix)` 索引。
///
/// `command_prefix` 取命令的前 2 个 token（如 `cargo test`），实现模式泛化。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApprovalCacheKey {
    pub tool_name: String,
    pub command_prefix: String,
}

impl ApprovalCacheKey {
    pub fn new(tool_name: &str, command: &str) -> Self {
        let prefix = extract_command_prefix(command);
        Self {
            tool_name: tool_name.to_string(),
            command_prefix: prefix,
        }
    }
}

/// 取命令的前 2 个空白分隔 token 作为模式前缀。
///
/// - `cargo test -p agent` → `cargo test`
/// - `npm run build` → `npm run`
/// - `ls` → `ls`
fn extract_command_prefix(command: &str) -> String {
    let trimmed = command.trim();
    let tokens: Vec<&str> = trimmed.split_whitespace().take(2).collect();
    tokens.join(" ")
}

/// 已缓存的审批决定。
#[derive(Debug, Clone)]
pub struct CachedApproval {
    pub granted_at: Instant,
}

/// 单会话审批缓存。
///
/// 生命周期跟随 `HitlGate`——session 结束时一并清空。
/// 子 agent 通过 `derive_child_cache` 获取父级只读视图。
pub struct SessionApprovalCache {
    session_id: String,
    entries: Mutex<HashMap<ApprovalCacheKey, CachedApproval>>,
    parent: Option<Arc<SessionApprovalCache>>,
}

impl SessionApprovalCache {
    pub fn new(session_id: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            session_id: session_id.into(),
            entries: Mutex::new(HashMap::new()),
            parent: None,
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 查询缓存。先查自身，未命中再查父级（只读继承链）。
    pub fn lookup<'a>(
        &'a self,
        key: &'a ApprovalCacheKey,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<CachedApproval>> + Send + 'a>>
    {
        Box::pin(async move {
            if let Some(hit) = self.entries.lock().await.get(key).cloned() {
                return Some(hit);
            }
            if let Some(parent) = &self.parent {
                return parent.lookup(key).await;
            }
            None
        })
    }

    /// 写入缓存（仅写入自身，不影响父级）。
    pub async fn insert(&self, key: ApprovalCacheKey) {
        self.entries.lock().await.insert(
            key,
            CachedApproval {
                granted_at: Instant::now(),
            },
        );
    }

    /// 驱逐指定 key（如命令执行失败后主动移除）。
    pub async fn invalidate(&self, key: &ApprovalCacheKey) {
        self.entries.lock().await.remove(key);
    }

    /// 清空所有缓存（不影响父级）。
    pub async fn clear(&self) {
        self.entries.lock().await.clear();
    }

    /// 生成子 agent 用的缓存视图。
    ///
    /// 子 agent 继承父级已批准条目（只读），自己的新审批写入子级缓存。
    /// 子 agent 结束时其缓存自然丢弃，不回写父级。
    pub fn derive_child_cache(self: &Arc<Self>, child_session_id: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            session_id: child_session_id.into(),
            entries: Mutex::new(HashMap::new()),
            parent: Some(Arc::clone(self)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lookup_hit_and_miss() {
        let cache = SessionApprovalCache::new("s1");
        let key = ApprovalCacheKey::new("exec_command", "cargo test -p agent");
        assert!(cache.lookup(&key).await.is_none());
        cache.insert(key.clone()).await;
        assert!(cache.lookup(&key).await.is_some());
    }

    #[tokio::test]
    async fn command_prefix_generalization() {
        let cache = SessionApprovalCache::new("s2");
        let key1 = ApprovalCacheKey::new("exec_command", "cargo test -p agent");
        cache.insert(key1).await;

        let key2 = ApprovalCacheKey::new("exec_command", "cargo test -p tools");
        assert!(
            cache.lookup(&key2).await.is_some(),
            "same prefix should hit cache"
        );

        let key3 = ApprovalCacheKey::new("exec_command", "cargo build");
        assert!(
            cache.lookup(&key3).await.is_none(),
            "different prefix should miss"
        );
    }

    #[tokio::test]
    async fn invalidate_removes_entry() {
        let cache = SessionApprovalCache::new("s3");
        let key = ApprovalCacheKey::new("exec_command", "rm -rf /tmp/test");
        cache.insert(key.clone()).await;
        assert!(cache.lookup(&key).await.is_some());
        cache.invalidate(&key).await;
        assert!(cache.lookup(&key).await.is_none());
    }

    #[tokio::test]
    async fn clear_removes_all() {
        let cache = SessionApprovalCache::new("s4");
        cache
            .insert(ApprovalCacheKey::new("exec_command", "cargo test"))
            .await;
        cache
            .insert(ApprovalCacheKey::new("exec_command", "npm run build"))
            .await;
        cache.clear().await;
        assert!(cache
            .lookup(&ApprovalCacheKey::new("exec_command", "cargo test"))
            .await
            .is_none());
    }

    #[test]
    fn extract_prefix_cases() {
        assert_eq!(extract_command_prefix("cargo test -p agent"), "cargo test");
        assert_eq!(extract_command_prefix("npm run build"), "npm run");
        assert_eq!(extract_command_prefix("ls"), "ls");
        assert_eq!(extract_command_prefix("  git   status  "), "git status");
        assert_eq!(extract_command_prefix(""), "");
    }

    #[tokio::test]
    async fn different_tools_are_independent() {
        let cache = SessionApprovalCache::new("s5");
        cache
            .insert(ApprovalCacheKey::new("exec_command", "cargo test"))
            .await;
        assert!(
            cache
                .lookup(&ApprovalCacheKey::new("code_exec", "cargo test"))
                .await
                .is_none(),
            "different tool_name should not hit"
        );
    }

    #[tokio::test]
    async fn child_inherits_parent_cache() {
        let parent = SessionApprovalCache::new("parent");
        parent
            .insert(ApprovalCacheKey::new("exec_command", "cargo test"))
            .await;

        let child = parent.derive_child_cache("child");
        assert!(
            child
                .lookup(&ApprovalCacheKey::new("exec_command", "cargo test"))
                .await
                .is_some(),
            "child should inherit parent approval"
        );
    }

    #[tokio::test]
    async fn child_write_does_not_affect_parent() {
        let parent = SessionApprovalCache::new("parent");
        let child = parent.derive_child_cache("child");
        child
            .insert(ApprovalCacheKey::new("exec_command", "npm install"))
            .await;

        assert!(
            parent
                .lookup(&ApprovalCacheKey::new("exec_command", "npm install"))
                .await
                .is_none(),
            "parent should not see child-only entries"
        );
        assert!(
            child
                .lookup(&ApprovalCacheKey::new("exec_command", "npm install"))
                .await
                .is_some(),
            "child should see its own entries"
        );
    }

    #[tokio::test]
    async fn child_clear_does_not_affect_parent() {
        let parent = SessionApprovalCache::new("parent");
        parent
            .insert(ApprovalCacheKey::new("exec_command", "cargo test"))
            .await;

        let child = parent.derive_child_cache("child");
        child.clear().await;

        assert!(
            child
                .lookup(&ApprovalCacheKey::new("exec_command", "cargo test"))
                .await
                .is_some(),
            "parent entries still visible after child clear"
        );
    }
}
