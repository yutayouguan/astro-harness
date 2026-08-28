//! 根会话级 AgentControl 进程目录，按 (db_path, root_session_id) 去重复用。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use subagents::{AgentControl, AgentGraphStore, Limits};

const DEFAULT_LIMITS: Limits = Limits {
    max_threads: 32,
    max_depth: 8,
    max_running: 8,
};

type StoreFactory = dyn Fn(&Path) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<AgentGraphStore>> + Send>> + Send + Sync;

/// 进程级 AgentControl 单例目录，按 root session 去重并弱引用缓存。
pub struct AgentControlDirectory {
    controls: Mutex<HashMap<(PathBuf, String), Weak<AgentControl>>>,
    store_factory: Arc<StoreFactory>,
}

impl AgentControlDirectory {
    /// 获取全局唯一的进程级目录实例。
    pub fn global() -> &'static Self {
        static DIRECTORY: OnceLock<AgentControlDirectory> = OnceLock::new();
        DIRECTORY.get_or_init(|| AgentControlDirectory {
            controls: Mutex::new(HashMap::new()),
            store_factory: Arc::new(|path| Box::pin(AgentGraphStore::open(path.to_path_buf()))),
        })
    }

    /// 打开或复用根会话的 AgentControl（默认 db 路径），含崩溃恢复。
    pub async fn open_root(&self, root_session_id: &str) -> anyhow::Result<Arc<AgentControl>> {
        self.open_root_at(
            root_session_id,
            &home::default_memory_dir().join("subagents-v2.db"),
        )
        .await
    }

    /// 在指定 db 路径打开或复用 AgentControl，清理残留预约并恢复中断线程。
    pub async fn open_root_at(
        &self,
        root_session_id: &str,
        graph_db_path: &Path,
    ) -> anyhow::Result<Arc<AgentControl>> {
        anyhow::ensure!(
            !root_session_id.trim().is_empty(),
            "root session id must not be empty"
        );
        let graph_db_path = normalize_graph_db_path(graph_db_path)?;
        let key = (graph_db_path.clone(), root_session_id.to_string());
        let mut controls = self
            .controls
            .lock()
            .map_err(|_| anyhow::anyhow!("agent control directory mutex is poisoned"))?;
        if let Some(control) = controls.get(&key).and_then(Weak::upgrade) {
            return Ok(control);
        }

        let store = (self.store_factory)(&graph_db_path).await?;
        store.cleanup_pending_reservations(root_session_id).await?;
        store.recover_running_as_interrupted(root_session_id).await?;
        let control = AgentControl::open(root_session_id.to_string(), store, DEFAULT_LIMITS).await?;
        controls.insert(key, Arc::downgrade(&control));
        Ok(control)
    }

    pub fn get(&self, root_session_id: &str) -> Option<Arc<AgentControl>> {
        self.get_at(
            root_session_id,
            &home::default_memory_dir().join("subagents-v2.db"),
        )
    }

    pub fn get_at(&self, root_session_id: &str, graph_db_path: &Path) -> Option<Arc<AgentControl>> {
        let graph_db_path = normalize_graph_db_path(graph_db_path).ok()?;
        self.controls
            .lock()
            .ok()?
            .get(&(graph_db_path, root_session_id.to_string()))
            .and_then(Weak::upgrade)
    }
}

fn normalize_graph_db_path(path: &Path) -> anyhow::Result<PathBuf> {
    if let Ok(path) = path.canonicalize() {
        return Ok(path);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let Some(parent) = absolute.parent() else {
        return Ok(absolute);
    };
    let Some(file_name) = absolute.file_name() else {
        return Ok(absolute);
    };
    Ok(parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf())
        .join(file_name))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex, Weak};

    use subagents::{
        AgentControl, AgentGraphStore, AgentPath, AgentStatusV2, RunnerEvent, ThreadReservation,
    };

    use super::{normalize_graph_db_path, AgentControlDirectory};

    fn directory(store: AgentGraphStore, opens: Arc<AtomicUsize>) -> AgentControlDirectory {
        AgentControlDirectory {
            controls: Mutex::new(HashMap::new()),
            store_factory: Arc::new(move |_| {
                let opens = opens.clone();
                let store = store.clone();
                Box::pin(async move {
                    opens.fetch_add(1, Ordering::SeqCst);
                    Ok(store)
                })
            }),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn returns_one_live_control_per_root_and_rebuilds_expired_weak() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("agents.db")).await.unwrap();
        let opens = Arc::new(AtomicUsize::new(0));
        let directory = directory(store, Arc::clone(&opens));

        let first = directory.open_root("root-a").await.unwrap();
        let same = directory.open_root("root-a").await.unwrap();
        let other = directory.open_root("root-b").await.unwrap();

        assert!(Arc::ptr_eq(&first, &same));
        assert!(!Arc::ptr_eq(&first, &other));
        assert!(Arc::ptr_eq(
            &first,
            &directory.get("root-a").expect("live root control")
        ));
        assert_eq!(opens.load(Ordering::SeqCst), 2);

        let old = Arc::downgrade(&first);
        drop(first);
        drop(same);
        assert!(old.upgrade().is_none());
        assert!(directory.get("root-a").is_none());

        let rebuilt = directory.open_root("root-a").await.unwrap();
        assert_eq!(rebuilt.root_thread_id(), "root-a");
        assert_eq!(opens.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn same_root_id_is_isolated_by_graph_database_path() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first_path = first_dir.path().join("subagents-v2.db");
        let second_path = second_dir.path().join("subagents-v2.db");
        let opens = Arc::new(AtomicUsize::new(0));
        let opened_paths = Arc::new(Mutex::new(Vec::new()));
        let directory = AgentControlDirectory {
            controls: Mutex::new(HashMap::<_, Weak<AgentControl>>::new()),
            store_factory: {
                let opens = Arc::clone(&opens);
                let opened_paths = Arc::clone(&opened_paths);
                Arc::new(move |path| {
                    opens.fetch_add(1, Ordering::SeqCst);
                    opened_paths.lock().unwrap().push(path.to_path_buf());
                    AgentGraphStore::open(path.to_path_buf())
                })
            },
        };

        let first = directory.open_root_at("shared-root", &first_path).unwrap();
        let first_again = directory.open_root_at("shared-root", &first_path).unwrap();
        let second = directory.open_root_at("shared-root", &second_path).unwrap();

        assert!(Arc::ptr_eq(&first, &first_again));
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(opens.load(Ordering::SeqCst), 2);
        assert_eq!(
            opened_paths.lock().unwrap().as_slice(),
            &[
                normalize_graph_db_path(&first_path).unwrap(),
                normalize_graph_db_path(&second_path).unwrap(),
            ]
        );
        assert!(first_path.exists());
        assert!(second_path.exists());
    }

    #[test]
    fn open_root_is_the_single_recovery_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("agents.db")).unwrap();
        store.ensure_root_thread("root").unwrap();
        store
            .reserve_thread(&ThreadReservation {
                thread_id: "pending".into(),
                root_thread_id: "root".into(),
                parent_thread_id: "root".into(),
                canonical_path: AgentPath::parse("/root/pending").unwrap(),
                task_name: "pending".into(),
                agent_type: "default".into(),
                session_id: "pending".into(),
            })
            .unwrap();
        assert!(AgentControl::open(
            "root".into(),
            store.clone(),
            subagents::Limits {
                max_threads: 32,
                max_depth: 8,
                max_running: 8,
            }
        )
        .is_err());

        store
            .reserve_thread(&ThreadReservation {
                thread_id: "running".into(),
                root_thread_id: "root".into(),
                parent_thread_id: "root".into(),
                canonical_path: AgentPath::parse("/root/running").unwrap(),
                task_name: "running".into(),
                agent_type: "default".into(),
                session_id: "running".into(),
            })
            .unwrap();
        store
            .apply_status_event(
                "running",
                RunnerEvent::TurnStarted {
                    turn_id: "crashed-turn".into(),
                },
            )
            .unwrap();

        let opens = Arc::new(AtomicUsize::new(0));
        let directory = directory(store.clone(), Arc::clone(&opens));
        let control = directory.open_root("root").unwrap();

        assert!(store.get_thread("pending").unwrap().is_none());
        assert_eq!(
            store.get_thread("running").unwrap().unwrap().status,
            AgentStatusV2::Interrupted
        );
        assert_eq!(
            store
                .status_events("running")
                .unwrap()
                .into_iter()
                .map(|event| event.event)
                .collect::<Vec<_>>(),
            vec![
                RunnerEvent::TurnStarted {
                    turn_id: "crashed-turn".into()
                },
                RunnerEvent::TurnInterrupted {
                    turn_id: "crashed-turn".into(),
                    reason: "runtime recovered after process interruption".into(),
                },
            ]
        );
        assert_eq!(
            control
                .resolve_target(&AgentPath::root(), "running")
                .unwrap()
                .thread_id,
            "running"
        );
        assert_eq!(opens.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn concurrent_open_of_one_root_runs_factory_and_recovery_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("agents.db")).unwrap();
        let opens = Arc::new(AtomicUsize::new(0));
        let directory = Arc::new(directory(store, Arc::clone(&opens)));
        let barrier = Arc::new(Barrier::new(3));

        let callers = (0..2)
            .map(|_| {
                let directory = Arc::clone(&directory);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    directory.open_root("root").unwrap()
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let controls = callers
            .into_iter()
            .map(|caller| caller.join().unwrap())
            .collect::<Vec<_>>();

        assert!(Arc::ptr_eq(&controls[0], &controls[1]));
        assert_eq!(opens.load(Ordering::SeqCst), 1);
    }
}
