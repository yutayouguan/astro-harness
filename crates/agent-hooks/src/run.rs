//! Bounded, queryable command-hook execution history.

use std::collections::VecDeque;
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
    pub source: String,
    pub display_order: usize,
    pub status: HookRunStatus,
    pub status_message: Option<String>,
    pub summary: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub duration_ms: Option<u64>,
    pub entries: Vec<HookOutputEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct HookRunStore {
    records: Arc<Mutex<VecDeque<HookRunRecord>>>,
}

impl HookRunStore {
    pub fn start(&self, record: HookRunRecord) {
        let Ok(mut records) = self.records.lock() else {
            return;
        };
        records.push_back(record);
        while records.len() > MAX_RECENT_RUNS {
            records.pop_front();
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
        let Ok(mut records) = self.records.lock() else {
            return;
        };
        if let Some(record) = records.iter_mut().rev().find(|record| record.id == id) {
            record.status = status;
            record.summary = summary;
            record.completed_at = Some(unix_timestamp());
            record.duration_ms = Some(duration_ms);
            record.entries = entries;
        }
    }

    pub fn recent(&self) -> Vec<HookRunRecord> {
        self.records
            .lock()
            .map(|records| records.iter().rev().cloned().collect())
            .unwrap_or_default()
    }
}

pub(crate) fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs().min(i64::MAX as u64) as i64)
}
