//! Bounded, queryable command-hook execution history.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use serde::Serialize;

const MAX_RECENT_RUNS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookRunStatus {
    Running,
    Completed,
    Failed,
    Blocked,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookHandlerType {
    Command,
    McpTool,
    Prompt,
    Agent,
}

impl HookHandlerType {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::McpTool => "mcp_tool",
            Self::Prompt => "prompt",
            Self::Agent => "agent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookExecutionMode {
    Sync,
    Async,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookScope {
    Thread,
    Turn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookTrustStatus {
    Managed,
    Untrusted,
    Trusted,
    Modified,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookSource {
    System,
    User,
    Project,
    Mdm,
    SessionFlags,
    Plugin,
    CloudRequirements,
    CloudManagedConfig,
    LegacyManagedConfigFile,
    LegacyManagedConfigMdm,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookOutputEntryKind {
    Warning,
    Stop,
    Feedback,
    Context,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookOutputEntry {
    pub kind: HookOutputEntryKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookRunRecord {
    pub id: String,
    pub event_name: String,
    pub handler_id: String,
    pub handler_type: HookHandlerType,
    pub execution_mode: HookExecutionMode,
    pub scope: HookScope,
    pub source_path: String,
    pub source: HookSource,
    pub display_order: usize,
    pub status: HookRunStatus,
    pub status_message: Option<String>,
    pub summary: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub duration_ms: Option<u64>,
    pub entries: Vec<HookOutputEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookRunLifecycleEvent {
    Started {
        turn_id: Option<String>,
        run: HookRunRecord,
    },
    Completed {
        turn_id: Option<String>,
        run: HookRunRecord,
    },
}

pub type HookRunObserver = Arc<dyn Fn(HookRunLifecycleEvent) + Send + Sync>;

#[derive(Debug, Clone)]
struct StoredHookRun {
    session_id: String,
    turn_id: Option<String>,
    run: HookRunRecord,
}

#[derive(Clone, Default)]
pub struct HookRunStore {
    pub(crate) audit_file: Option<std::path::PathBuf>,
    records: Arc<Mutex<VecDeque<StoredHookRun>>>,
    observers: Arc<Mutex<HashMap<String, HookRunObserver>>>,
}

impl std::fmt::Debug for HookRunStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HookRunStore")
            .field(
                "recent_count",
                &self.records.lock().map_or(0, |runs| runs.len()),
            )
            .finish_non_exhaustive()
    }
}

impl HookRunStore {
    pub fn with_audit_root(base: &std::path::Path) -> Self {
        Self {
            audit_file: Some(
                home::hook_runs_dir(base)
                    .join(format!("runs-{}.jsonl", uuid::Uuid::new_v4().simple())),
            ),
            ..Default::default()
        }
    }

    fn persist_metadata(
        &self,
        phase: &str,
        session_id: &str,
        turn_id: Option<&str>,
        run: &HookRunRecord,
    ) {
        use std::io::Write;
        let Some(path) = &self.audit_file else {
            return;
        };
        let result = (|| -> anyhow::Result<()> {
            std::fs::create_dir_all(path.parent().unwrap())?;
            anyhow::ensure!(
                !path
                    .symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_symlink()),
                "hook audit file is a symlink"
            );
            let mut options = std::fs::OpenOptions::new();
            options.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(path)?;
            file.lock()?;
            let value = serde_json::json!({"phase":phase,"session_id":session_id,"turn_id":turn_id,"id":run.id,"handler_id":run.handler_id,"event":run.event_name,"status":run.status,"source":run.source_path,"started_at":run.started_at,"completed_at":run.completed_at,"duration_ms":run.duration_ms});
            writeln!(file, "{value}")?;
            file.unlock()?;
            Ok(())
        })();
        if let Err(error) = result {
            tracing::warn!(%error, "hook metadata audit write failed");
        }
    }
    pub fn set_observer(&self, session_id: impl Into<String>, observer: HookRunObserver) {
        if let Ok(mut observers) = self.observers.lock() {
            observers.insert(session_id.into(), observer);
        }
    }

    pub fn remove_observer(&self, session_id: &str) {
        if let Ok(mut observers) = self.observers.lock() {
            observers.remove(session_id);
        }
    }

    pub fn start(&self, session_id: String, turn_id: Option<String>, record: HookRunRecord) {
        self.persist_metadata("started", &session_id, turn_id.as_deref(), &record);
        let notify = record.execution_mode == HookExecutionMode::Sync;
        if let Ok(mut records) = self.records.lock() {
            records.push_back(StoredHookRun {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                run: record.clone(),
            });
            while records.len() > MAX_RECENT_RUNS {
                records.pop_front();
            }
        } else {
            return;
        }
        if notify {
            self.notify(
                &session_id,
                HookRunLifecycleEvent::Started {
                    turn_id,
                    run: record,
                },
            );
        }
    }

    pub fn finish(
        &self,
        id: &str,
        status: HookRunStatus,
        summary: String,
        duration_ms: u64,
        entries: Vec<HookOutputEntry>,
    ) {
        self.finish_inner(id, status, summary, duration_ms, entries, true);
    }

    pub(crate) fn finish_silently(
        &self,
        id: &str,
        status: HookRunStatus,
        summary: String,
        duration_ms: u64,
        entries: Vec<HookOutputEntry>,
    ) {
        self.finish_inner(id, status, summary, duration_ms, entries, false);
    }

    fn finish_inner(
        &self,
        id: &str,
        status: HookRunStatus,
        summary: String,
        duration_ms: u64,
        entries: Vec<HookOutputEntry>,
        notify: bool,
    ) {
        let completed = {
            let Ok(mut records) = self.records.lock() else {
                return;
            };
            records
                .iter_mut()
                .rev()
                .find(|record| record.run.id == id)
                .map(|record| {
                    record.run.status = status;
                    record.run.summary = summary;
                    record.run.completed_at = Some(unix_timestamp());
                    record.run.duration_ms = Some(duration_ms);
                    record.run.entries = entries;
                    (
                        record.session_id.clone(),
                        record.turn_id.clone(),
                        record.run.clone(),
                    )
                })
        };
        if let Some((session_id, turn_id, run)) = &completed {
            self.persist_metadata("completed", session_id, turn_id.as_deref(), run);
        }
        if let Some((session_id, turn_id, run)) = completed.filter(|_| notify) {
            self.notify(
                &session_id,
                HookRunLifecycleEvent::Completed { turn_id, run },
            );
        }
    }

    pub fn recent(&self) -> Vec<HookRunRecord> {
        self.records
            .lock()
            .map(|records| {
                records
                    .iter()
                    .rev()
                    .map(|record| record.run.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn notify(&self, session_id: &str, event: HookRunLifecycleEvent) {
        let observer = self
            .observers
            .lock()
            .ok()
            .and_then(|observers| observers.get(session_id).cloned());
        if let Some(observer) = observer {
            observer(event);
        }
    }
}

pub(crate) fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs().min(i64::MAX as u64) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_audit_keeps_metadata_not_hook_output() {
        let dir = tempfile::tempdir().unwrap();
        let store = HookRunStore::with_audit_root(dir.path());
        assert!(!home::hook_runs_dir(dir.path()).exists());
        let mut run = running_record();
        run.summary = "secret-command-output".into();
        run.status_message = Some("private message".into());
        store.start("session".into(), Some("turn".into()), run);
        let text = std::fs::read_to_string(store.audit_file.unwrap()).unwrap();
        assert!(text.contains("hook-run-1") && text.contains("session"));
        assert!(!text.contains("secret-command-output") && !text.contains("private message"));
    }

    fn running_record() -> HookRunRecord {
        HookRunRecord {
            id: "hook-run-1".into(),
            event_name: "PreToolUse".into(),
            handler_id: "handler-1".into(),
            handler_type: HookHandlerType::Command,
            execution_mode: HookExecutionMode::Sync,
            scope: HookScope::Turn,
            source_path: "/tmp/config.toml".into(),
            source: HookSource::Project,
            display_order: 0,
            status: HookRunStatus::Running,
            status_message: None,
            summary: "running command hook".into(),
            started_at: 1,
            completed_at: None,
            duration_ms: None,
            entries: Vec::new(),
        }
    }

    #[test]
    fn observer_receives_started_then_completed_snapshots() {
        let store = HookRunStore::default();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&observed);
        store.set_observer(
            "session-1",
            Arc::new(move |event| sink.lock().unwrap().push(event)),
        );

        store.start("session-1".into(), Some("turn-1".into()), running_record());
        store.finish(
            "hook-run-1",
            HookRunStatus::Completed,
            "done".into(),
            7,
            Vec::new(),
        );

        let observed = observed.lock().unwrap();
        assert!(matches!(
            &observed[0],
            HookRunLifecycleEvent::Started { turn_id, run }
                if turn_id.as_deref() == Some("turn-1")
                    && run.status == HookRunStatus::Running
        ));
        assert!(matches!(
            &observed[1],
            HookRunLifecycleEvent::Completed { turn_id, run }
                if turn_id.as_deref() == Some("turn-1")
                    && run.status == HookRunStatus::Completed
                    && run.duration_ms == Some(7)
        ));
    }

    #[test]
    fn observer_emits_only_completion_for_async_runs() {
        let store = HookRunStore::default();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&observed);
        store.set_observer(
            "session-1",
            Arc::new(move |event| sink.lock().unwrap().push(event)),
        );
        let mut record = running_record();
        record.execution_mode = HookExecutionMode::Async;

        store.start("session-1".into(), Some("turn-1".into()), record);
        store.finish(
            "hook-run-1",
            HookRunStatus::Completed,
            "done".into(),
            7,
            Vec::new(),
        );

        let observed = observed.lock().unwrap();
        assert_eq!(observed.len(), 1);
        assert!(matches!(
            &observed[0],
            HookRunLifecycleEvent::Completed { turn_id, run }
                if turn_id.as_deref() == Some("turn-1")
                    && run.execution_mode == HookExecutionMode::Async
                    && run.status == HookRunStatus::Completed
        ));
        assert_eq!(store.recent()[0].status, HookRunStatus::Completed);
    }

    #[test]
    fn silent_completion_updates_storage_without_notifying() {
        let store = HookRunStore::default();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&observed);
        store.set_observer(
            "session-1",
            Arc::new(move |event| sink.lock().unwrap().push(event)),
        );
        let mut record = running_record();
        record.execution_mode = HookExecutionMode::Async;

        store.start("session-1".into(), Some("turn-1".into()), record);
        store.finish_silently(
            "hook-run-1",
            HookRunStatus::Failed,
            "cancelled".into(),
            7,
            Vec::new(),
        );

        assert!(observed.lock().unwrap().is_empty());
        assert_eq!(store.recent()[0].status, HookRunStatus::Failed);
    }

    #[test]
    fn observers_are_isolated_by_session() {
        let store = HookRunStore::default();
        let first = Arc::new(Mutex::new(Vec::new()));
        let second = Arc::new(Mutex::new(Vec::new()));
        let first_sink = Arc::clone(&first);
        let second_sink = Arc::clone(&second);
        store.set_observer(
            "session-1",
            Arc::new(move |event| first_sink.lock().unwrap().push(event)),
        );
        store.set_observer(
            "session-2",
            Arc::new(move |event| second_sink.lock().unwrap().push(event)),
        );

        store.start("session-1".into(), Some("turn-1".into()), running_record());

        assert_eq!(first.lock().unwrap().len(), 1);
        assert!(second.lock().unwrap().is_empty());
    }
}
