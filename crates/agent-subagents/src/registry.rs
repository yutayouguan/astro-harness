//! Agent 线程树的内存注册表与配额管理（派生/执行并发控制）。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use anyhow::{bail, Context};

use crate::{
    ActivityBus, AgentActivityKind, AgentGraphStore, AgentPath, AgentStatusV2, AgentThreadV2,
};

/// Agent 线程树的资源配额：最大线程数、最大深度、最大并发执行数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_threads: usize,
    pub max_depth: usize,
    pub max_running: usize,
}

#[derive(Debug, Default)]
struct RegistryState {
    identities: BTreeMap<AgentPath, String>,
    thread_paths: HashMap<String, AgentPath>,
    reserved_paths: HashMap<AgentPath, String>,
    active_executions: HashSet<String>,
}

/// 线程树内存注册表，管理路径→线程映射、预留槽位和执行许可。
#[derive(Debug)]
pub struct AgentRegistry {
    root_thread_id: String,
    limits: Limits,
    state: Mutex<RegistryState>,
}

impl AgentRegistry {
    /// 从已有线程列表构建注册表，校验根线程存在且无重复。
    pub fn from_threads(limits: Limits, threads: &[AgentThreadV2]) -> anyhow::Result<Self> {
        let root = threads
            .iter()
            .find(|thread| thread.canonical_path == AgentPath::root())
            .context("agent registry requires a root thread")?;
        if root.parent_thread_id.is_some() {
            bail!("root agent thread must not have a parent");
        }

        let mut state = RegistryState::default();
        for thread in threads {
            if thread.root_thread_id != root.thread_id {
                bail!(
                    "agent thread {:?} belongs to root {:?}, expected {:?}",
                    thread.thread_id,
                    thread.root_thread_id,
                    root.thread_id
                );
            }
            if state.thread_paths.contains_key(&thread.thread_id) {
                bail!("duplicate agent thread id {:?}", thread.thread_id);
            }
            if state.identities.contains_key(&thread.canonical_path) {
                bail!("duplicate agent path {:?}", thread.canonical_path);
            }
            state
                .thread_paths
                .insert(thread.thread_id.clone(), thread.canonical_path.clone());
            state
                .identities
                .insert(thread.canonical_path.clone(), thread.thread_id.clone());
        }

        Ok(Self {
            root_thread_id: root.thread_id.clone(),
            limits,
            state: Mutex::new(state),
        })
    }

    /// 在父路径下预留一个子线程派生槽位（默认类型）。
    pub fn reserve_spawn<'a>(
        &'a self,
        parent: &AgentPath,
        task_name: &str,
        thread_id: &str,
    ) -> anyhow::Result<SpawnReservation<'a>> {
        self.reserve_spawn_typed(parent, task_name, "default", thread_id)
    }

    /// 在父路径下预留指定类型的子线程槽位，校验深度和数量配额。
    pub fn reserve_spawn_typed<'a>(
        &'a self,
        parent: &AgentPath,
        task_name: &str,
        agent_type: &str,
        thread_id: &str,
    ) -> anyhow::Result<SpawnReservation<'a>> {
        if thread_id.trim().is_empty() {
            bail!("agent thread id must not be empty");
        }
        if agent_type.trim().is_empty() {
            bail!("agent type must not be empty");
        }
        let path = parent.child(task_name).map_err(anyhow::Error::msg)?;
        if path.depth() > self.limits.max_depth {
            bail!(
                "agent spawn depth limit reached ({}/{})",
                path.depth(),
                self.limits.max_depth
            );
        }

        let mut state = self.lock_state()?;
        if !state.identities.contains_key(parent) {
            bail!("unknown parent agent path {parent:?}");
        }
        let parent_thread_id = state
            .identities
            .get(parent)
            .context("parent agent identity disappeared")?
            .clone();
        if state.identities.contains_key(&path) || state.reserved_paths.contains_key(&path) {
            bail!("agent path {path:?} already exists or is reserved");
        }
        let identity_count = descendant_identity_count(&state);
        if identity_count + state.reserved_paths.len() >= self.limits.max_threads {
            bail!(
                "agent identity limit reached ({}/{})",
                identity_count + state.reserved_paths.len(),
                self.limits.max_threads
            );
        }
        state
            .reserved_paths
            .insert(path.clone(), thread_id.to_string());
        drop(state);

        let thread = AgentThreadV2 {
            thread_id: thread_id.to_string(),
            root_thread_id: self.root_thread_id.clone(),
            parent_thread_id: Some(parent_thread_id),
            canonical_path: path.clone(),
            task_name: task_name.to_string(),
            agent_type: agent_type.trim().to_string(),
            session_id: thread_id.to_string(),
            status: AgentStatusV2::PendingInit,
            created_at: String::new(),
            updated_at: String::new(),
        };

        Ok(SpawnReservation {
            registry: self,
            path,
            thread_id: thread_id.to_string(),
            thread,
            persisted_store: None,
            activity: None,
            active: true,
        })
    }

    /// 获取子线程执行许可，受 `max_running` 并发限制。
    pub fn acquire_execution<'a>(&'a self, thread_id: &str) -> anyhow::Result<ExecutionPermit<'a>> {
        let mut state = self.lock_state()?;
        let path = state
            .thread_paths
            .get(thread_id)
            .with_context(|| format!("unknown agent thread id {thread_id:?}"))?;
        if path == &AgentPath::root() {
            bail!("root thread does not consume a subagent execution slot");
        }
        if state.active_executions.contains(thread_id) {
            bail!("agent thread {thread_id:?} already has an active execution");
        }
        if state.active_executions.len() >= self.limits.max_running {
            bail!(
                "active agent execution limit reached ({}/{})",
                state.active_executions.len(),
                self.limits.max_running
            );
        }
        state.active_executions.insert(thread_id.to_string());
        drop(state);
        Ok(ExecutionPermit {
            registry: self,
            thread_id: thread_id.to_string(),
            active: true,
        })
    }

    pub fn identity_count(&self) -> anyhow::Result<usize> {
        let state = self.lock_state()?;
        Ok(descendant_identity_count(&state))
    }

    pub fn active_execution_count(&self) -> anyhow::Result<usize> {
        Ok(self.lock_state()?.active_executions.len())
    }

    pub fn thread_id_for_path(&self, path: &AgentPath) -> anyhow::Result<Option<String>> {
        Ok(self.lock_state()?.identities.get(path).cloned())
    }

    pub fn committed_path_for_thread(&self, thread_id: &str) -> anyhow::Result<Option<AgentPath>> {
        Ok(self.lock_state()?.thread_paths.get(thread_id).cloned())
    }

    pub(crate) fn rollback_committed_spawn(
        &self,
        path: &AgentPath,
        thread_id: &str,
    ) -> anyhow::Result<()> {
        let mut state = self.lock_state()?;
        if state.active_executions.contains(thread_id) {
            bail!("cannot roll back agent thread {thread_id:?} with an active execution");
        }
        if state.thread_paths.get(thread_id) != Some(path)
            || state.identities.get(path).map(String::as_str) != Some(thread_id)
        {
            bail!("agent thread {thread_id:?} is not committed at path {path}");
        }
        state.thread_paths.remove(thread_id);
        state.identities.remove(path);
        Ok(())
    }

    pub(crate) fn rollback_committed_spawn_if_matches(
        &self,
        path: &AgentPath,
        thread_id: &str,
    ) -> anyhow::Result<bool> {
        let mut state = self.lock_state()?;
        if state.active_executions.contains(thread_id) {
            bail!("cannot roll back agent thread {thread_id:?} with an active execution");
        }
        let path_owner = state.identities.get(path).map(String::as_str);
        let thread_path = state.thread_paths.get(thread_id);
        match (path_owner, thread_path) {
            (None, None) => Ok(false),
            (Some(owner), Some(stored_path)) if owner == thread_id && stored_path == path => {
                state.thread_paths.remove(thread_id);
                state.identities.remove(path);
                Ok(true)
            }
            _ => bail!("refusing to roll back stale agent identity {thread_id:?} at path {path}"),
        }
    }

    pub fn root_thread_id(&self) -> &str {
        &self.root_thread_id
    }

    fn lock_state(&self) -> anyhow::Result<MutexGuard<'_, RegistryState>> {
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("agent registry mutex is poisoned"))
    }

    fn release_reservation(&self, path: &AgentPath, thread_id: &str) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                tracing::warn!("recovering poisoned agent registry during reservation rollback");
                poisoned.into_inner()
            }
        };
        if state.reserved_paths.get(path).map(String::as_str) == Some(thread_id) {
            state.reserved_paths.remove(path);
        }
    }

    fn release_execution(&self, thread_id: &str) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                tracing::warn!("recovering poisoned agent registry during execution release");
                poisoned.into_inner()
            }
        };
        state.active_executions.remove(thread_id);
    }
}

/// 派生预留令牌，drop 时自动回滚未提交的预留。
pub struct SpawnReservation<'a> {
    registry: &'a AgentRegistry,
    path: AgentPath,
    thread_id: String,
    thread: AgentThreadV2,
    persisted_store: Option<&'a AgentGraphStore>,
    activity: Option<&'a ActivityBus>,
    active: bool,
}

impl<'a> SpawnReservation<'a> {
    pub fn canonical_path(&self) -> &AgentPath {
        &self.path
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn thread(&self) -> &AgentThreadV2 {
        &self.thread
    }

    pub(crate) fn attach_persisted(
        &mut self,
        store: &'a AgentGraphStore,
        activity: &'a ActivityBus,
        thread: AgentThreadV2,
    ) {
        self.persisted_store = Some(store);
        self.activity = Some(activity);
        self.thread = thread;
    }

    /// 主动放弃预留，回滚持久层和内存状态。
    pub fn abort(mut self) -> anyhow::Result<()> {
        self.rollback_durable()
            .context("failed to abort durable agent reservation")?;
        self.release_memory();
        Ok(())
    }

    /// 提交预留，将线程身份正式注册到注册表。
    pub fn commit(mut self) -> anyhow::Result<()> {
        let precheck_result = (|| {
            let state = self.registry.lock_state()?;
            if state.thread_paths.contains_key(&self.thread_id) {
                bail!("agent thread id {:?} already exists", self.thread_id);
            }
            if state.reserved_paths.get(&self.path).map(String::as_str)
                != Some(self.thread_id.as_str())
            {
                bail!("agent path {:?} is no longer reserved", self.path);
            }
            Ok(())
        })();
        if let Err(commit_error) = precheck_result {
            return self.rollback_after_commit_error(commit_error);
        }
        if let Some(store) = self.persisted_store {
            if let Err(error) = store.validate_pending_reservation(&self.thread) {
                return self.rollback_after_commit_error(
                    error.context("durable pending reservation validation failed"),
                );
            }
        }
        let commit_result = (|| {
            let mut state = self.registry.lock_state()?;
            if state.thread_paths.contains_key(&self.thread_id) {
                bail!("agent thread id {:?} already exists", self.thread_id);
            }
            if state.reserved_paths.get(&self.path).map(String::as_str)
                != Some(self.thread_id.as_str())
            {
                bail!("agent path {:?} is no longer reserved", self.path);
            }
            state.reserved_paths.remove(&self.path);
            state
                .identities
                .insert(self.path.clone(), self.thread_id.clone());
            state
                .thread_paths
                .insert(self.thread_id.clone(), self.path.clone());
            Ok(())
        })();
        if let Err(commit_error) = commit_result {
            return self.rollback_after_commit_error(commit_error);
        }
        self.active = false;
        if let Some(activity) = self.activity {
            activity.publish(
                AgentActivityKind::Spawned {
                    thread_id: self.thread_id.clone(),
                },
                Some(self.thread.clone()),
            );
        }
        Ok(())
    }

    fn rollback_after_commit_error(&mut self, commit_error: anyhow::Error) -> anyhow::Result<()> {
        match self.rollback_durable() {
            Ok(()) => {
                self.release_memory();
                Err(commit_error)
            }
            Err(rollback_error) => Err(anyhow::anyhow!(
                "agent identity commit failed: {commit_error:#}; durable rollback failed: {rollback_error:#}"
            )),
        }
    }

    fn rollback_durable(&mut self) -> anyhow::Result<()> {
        if let Some(store) = self.persisted_store {
            store.rollback_pending_thread(&self.thread_id)?;
            self.persisted_store = None;
        }
        Ok(())
    }

    fn release_memory(&mut self) {
        if self.active {
            self.registry
                .release_reservation(&self.path, &self.thread_id);
            self.active = false;
        }
    }
}

impl Drop for SpawnReservation<'_> {
    fn drop(&mut self) {
        if self.active {
            if let Err(error) = self.rollback_durable() {
                tracing::warn!(
                    thread_id = %self.thread_id,
                    %error,
                    "failed to roll back pending agent thread"
                );
            }
            self.release_memory();
        }
    }
}

/// 执行许可令牌，drop 时自动释放并发执行槽位。
#[derive(Debug)]
pub struct ExecutionPermit<'a> {
    registry: &'a AgentRegistry,
    thread_id: String,
    active: bool,
}

impl Drop for ExecutionPermit<'_> {
    fn drop(&mut self) {
        if self.active {
            self.registry.release_execution(&self.thread_id);
            self.active = false;
        }
    }
}

fn descendant_identity_count(state: &RegistryState) -> usize {
    state
        .identities
        .keys()
        .filter(|path| **path != AgentPath::root())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentPath, AgentStatusV2, AgentThreadV2};

    fn limits(max_threads: usize, max_depth: usize, max_running: usize) -> Limits {
        Limits {
            max_threads,
            max_depth,
            max_running,
        }
    }

    fn thread(path: &str, status: AgentStatusV2) -> AgentThreadV2 {
        let canonical_path = AgentPath::parse(path).unwrap();
        let thread_id = if canonical_path == AgentPath::root() {
            "root-thread".to_string()
        } else {
            format!("thread-{}", canonical_path.name())
        };
        AgentThreadV2 {
            root_thread_id: "root-thread".into(),
            parent_thread_id: canonical_path
                .parent()
                .map(|parent| format!("thread-{}", parent.name())),
            canonical_path: canonical_path.clone(),
            task_name: canonical_path.name().to_string(),
            agent_type: "default".into(),
            session_id: thread_id.clone(),
            thread_id,
            status,
            created_at: "now".into(),
            updated_at: "now".into(),
        }
    }

    fn attach_persisted<'a>(
        reservation: &mut SpawnReservation<'a>,
        store: &'a AgentGraphStore,
        activity: &'a ActivityBus,
    ) {
        let thread = reservation.thread().clone();
        let persisted = store
            .reserve_thread(&crate::ThreadReservation {
                thread_id: thread.thread_id,
                root_thread_id: thread.root_thread_id,
                parent_thread_id: thread.parent_thread_id.unwrap(),
                canonical_path: thread.canonical_path,
                task_name: thread.task_name,
                agent_type: thread.agent_type,
                session_id: thread.session_id,
            })
            .unwrap();
        reservation.attach_persisted(store, activity, persisted);
    }

    #[test]
    fn dropped_spawn_reservation_releases_path_and_identity_slot() {
        let registry = AgentRegistry::from_threads(
            limits(1, 1, 1),
            &[thread("/root", AgentStatusV2::Running)],
        )
        .unwrap();
        {
            let _reservation = registry
                .reserve_spawn(&AgentPath::root(), "worker", "thread-worker")
                .unwrap();
            assert!(registry
                .reserve_spawn(&AgentPath::root(), "worker", "other-thread")
                .is_err());
        }

        assert!(registry
            .reserve_spawn(&AgentPath::root(), "worker", "thread-worker")
            .is_ok());
    }

    #[test]
    fn commit_retains_identity_and_failed_commit_releases_reservation() {
        let registry = AgentRegistry::from_threads(
            limits(3, 1, 1),
            &[thread("/root", AgentStatusV2::Running)],
        )
        .unwrap();
        registry
            .reserve_spawn(&AgentPath::root(), "first", "shared-thread")
            .unwrap()
            .commit()
            .unwrap();
        assert_eq!(registry.identity_count().unwrap(), 1);
        assert_eq!(
            registry
                .thread_id_for_path(&AgentPath::parse("/root/first").unwrap())
                .unwrap()
                .as_deref(),
            Some("shared-thread")
        );

        let error = registry
            .reserve_spawn(&AgentPath::root(), "second", "shared-thread")
            .unwrap()
            .commit()
            .unwrap_err();
        assert!(error.to_string().contains("thread id"));
        assert_eq!(registry.identity_count().unwrap(), 1);
        assert!(registry
            .reserve_spawn(&AgentPath::root(), "second", "new-thread")
            .is_ok());
    }

    #[test]
    fn failed_commit_rolls_back_attached_pending_row() {
        let registry = AgentRegistry::from_threads(
            limits(3, 1, 1),
            &[thread("/root", AgentStatusV2::Running)],
        )
        .unwrap();
        registry
            .reserve_spawn(&AgentPath::root(), "first", "shared-thread")
            .unwrap()
            .commit()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store.ensure_root_thread("root-thread").unwrap();
        let activity = ActivityBus::default();
        let mut reservation = registry
            .reserve_spawn(&AgentPath::root(), "second", "shared-thread")
            .unwrap();
        attach_persisted(&mut reservation, &store, &activity);

        reservation.commit().unwrap_err();

        assert!(store.get_thread("shared-thread").unwrap().is_none());
        assert!(registry
            .reserve_spawn(&AgentPath::root(), "second", "replacement")
            .is_ok());
    }

    #[test]
    fn failed_commit_reports_durable_rollback_failure() {
        let registry = AgentRegistry::from_threads(
            limits(3, 1, 1),
            &[thread("/root", AgentStatusV2::Running)],
        )
        .unwrap();
        registry
            .reserve_spawn(&AgentPath::root(), "first", "shared-thread")
            .unwrap()
            .commit()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        store.ensure_root_thread("root-thread").unwrap();
        let activity = ActivityBus::default();
        let mut reservation = registry
            .reserve_spawn(&AgentPath::root(), "second", "shared-thread")
            .unwrap();
        attach_persisted(&mut reservation, &store, &activity);
        store
            .apply_status_event(
                "shared-thread",
                crate::RunnerEvent::TurnStarted {
                    turn_id: "invalid-early-start".into(),
                },
            )
            .unwrap();

        let error = reservation.commit().unwrap_err();

        assert!(error.to_string().contains("thread id"));
        assert!(error.to_string().contains("durable rollback"));
        assert!(registry
            .reserve_spawn(&AgentPath::root(), "second", "replacement")
            .is_ok());
    }

    #[test]
    fn identity_depth_and_execution_limits_are_independent() {
        let threads = vec![
            thread("/root", AgentStatusV2::Running),
            thread(
                "/root/completed",
                AgentStatusV2::Completed {
                    last_message: "done".into(),
                },
            ),
            thread("/root/interrupted", AgentStatusV2::Interrupted),
            thread(
                "/root/errored",
                AgentStatusV2::Errored {
                    message: "boom".into(),
                },
            ),
        ];
        let registry = AgentRegistry::from_threads(limits(3, 2, 1), &threads).unwrap();

        assert_eq!(registry.identity_count().unwrap(), 3);
        assert!(registry
            .reserve_spawn(&AgentPath::root(), "overflow", "thread-overflow")
            .is_err());

        let completed = registry.acquire_execution("thread-completed").unwrap();
        assert_eq!(registry.active_execution_count().unwrap(), 1);
        assert!(registry.acquire_execution("thread-interrupted").is_err());
        drop(completed);
        assert_eq!(registry.active_execution_count().unwrap(), 0);
        assert!(registry.acquire_execution("thread-interrupted").is_ok());

        let roomier = AgentRegistry::from_threads(limits(4, 1, 2), &threads[..2]).unwrap();
        assert!(roomier
            .reserve_spawn(
                &AgentPath::parse("/root/completed").unwrap(),
                "too_deep",
                "deep-thread",
            )
            .is_err());
    }
}
