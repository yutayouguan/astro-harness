//! Weak, home-scoped registry of actually installed turns, including background
//! and subagent turns that do not have a Desktop ManagedThread.
use super::Session;
use crate::HitlGate;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

struct Entry {
    session: Weak<Session>,
    gate: Option<Weak<HitlGate>>,
    turn_id: String,
}
pub struct LiveTurn {
    pub session: Arc<Session>,
    pub gate: Option<Arc<HitlGate>>,
    pub turn_id: String,
}
type Key = (PathBuf, String);
fn entries() -> &'static Mutex<HashMap<Key, Entry>> {
    static ENTRIES: OnceLock<Mutex<HashMap<Key, Entry>>> = OnceLock::new();
    ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
}
fn root(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
pub fn register(session: &Arc<Session>, gate: Option<&Arc<HitlGate>>, turn_id: &str) {
    let mut all = entries().lock().unwrap();
    all.retain(|_, e| e.session.strong_count() > 0);
    all.insert(
        (root(session.memory_dir()), session.session_id().into()),
        Entry {
            session: Arc::downgrade(session),
            gate: gate.map(Arc::downgrade),
            turn_id: turn_id.into(),
        },
    );
    types::pending_interaction::changed();
}
pub async fn active(base: &Path) -> Vec<LiveTurn> {
    let base = root(base);
    let candidates: Vec<_> = entries()
        .lock()
        .unwrap()
        .iter()
        .filter(|((path, _), _)| path == &base)
        .filter_map(|(_, e)| {
            Some(LiveTurn {
                session: e.session.upgrade()?,
                gate: e.gate.as_ref().and_then(Weak::upgrade),
                turn_id: e.turn_id.clone(),
            })
        })
        .collect();
    let mut live = Vec::new();
    for entry in candidates {
        if entry.session.current_turn_id().await.as_deref() == Some(&entry.turn_id) {
            live.push(entry);
        }
    }
    live
}
pub async fn find(base: &Path, session_id: &str) -> Option<LiveTurn> {
    active(base)
        .await
        .into_iter()
        .find(|e| e.session.session_id() == session_id)
}
