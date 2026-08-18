use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use subagents::{AgentControl, AgentGraphStore, Limits};

const DEFAULT_LIMITS: Limits = Limits {
    max_threads: 32,
    max_depth: 8,
    max_running: 8,
};

/// Process locator for the one root-scoped control plane shared by all
/// sessions and short-lived agent turn runtimes belonging to that root.
pub struct AgentControlDirectory {
    controls: Mutex<HashMap<String, Weak<AgentControl>>>,
    store_factory: Arc<dyn Fn() -> anyhow::Result<AgentGraphStore> + Send + Sync>,
}

impl AgentControlDirectory {
    pub fn global() -> &'static Self {
        static DIRECTORY: OnceLock<AgentControlDirectory> = OnceLock::new();
        DIRECTORY.get_or_init(|| AgentControlDirectory {
            controls: Mutex::new(HashMap::new()),
            store_factory: Arc::new(AgentGraphStore::open_default_v2),
        })
    }

    pub fn open_root(&self, root_session_id: &str) -> anyhow::Result<Arc<AgentControl>> {
        anyhow::ensure!(
            !root_session_id.trim().is_empty(),
            "root session id must not be empty"
        );
        let mut controls = self
            .controls
            .lock()
            .map_err(|_| anyhow::anyhow!("agent control directory mutex is poisoned"))?;
        if let Some(control) = controls.get(root_session_id).and_then(Weak::upgrade) {
            return Ok(control);
        }

        let store = (self.store_factory)()?;
        store.cleanup_pending_reservations(root_session_id)?;
        store.recover_running_as_interrupted(root_session_id)?;
        let control = AgentControl::open(root_session_id.to_string(), store, DEFAULT_LIMITS)?;
        controls.insert(root_session_id.to_string(), Arc::downgrade(&control));
        Ok(control)
    }

    pub fn get(&self, root_session_id: &str) -> Option<Arc<AgentControl>> {
        self.controls
            .lock()
            .ok()?
            .get(root_session_id)
            .and_then(Weak::upgrade)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex, Weak};

    use subagents::{
        AgentControl, AgentGraphStore, AgentPath, AgentStatusV2, RunnerEvent, ThreadReservation,
    };

    use super::AgentControlDirectory;

    fn directory(store: AgentGraphStore, opens: Arc<AtomicUsize>) -> AgentControlDirectory {
        AgentControlDirectory {
            controls: Mutex::new(HashMap::<String, Weak<AgentControl>>::new()),
            store_factory: Arc::new(move || {
                opens.fetch_add(1, Ordering::SeqCst);
                Ok(store.clone())
            }),
        }
    }

    #[test]
    fn returns_one_live_control_per_root_and_rebuilds_expired_weak() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("agents.db")).unwrap();
        let opens = Arc::new(AtomicUsize::new(0));
        let directory = directory(store, Arc::clone(&opens));

        let first = directory.open_root("root-a").unwrap();
        let same = directory.open_root("root-a").unwrap();
        let other = directory.open_root("root-b").unwrap();

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

        let rebuilt = directory.open_root("root-a").unwrap();
        assert_eq!(rebuilt.root_thread_id(), "root-a");
        assert_eq!(opens.load(Ordering::SeqCst), 3);
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
