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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookRunRecord {
    pub id: String,
    pub event_name: String,
    pub handler_id: String,
    pub source: String,
    pub status: HookRunStatus,
    pub summary: String,
    pub duration_ms: Option<u64>,
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

    pub fn finish(&self, id: &str, status: HookRunStatus, summary: String, duration_ms: u64) {
        let Ok(mut records) = self.records.lock() else {
            return;
        };
        if let Some(record) = records.iter_mut().rev().find(|record| record.id == id) {
            record.status = status;
            record.summary = summary;
            record.duration_ms = Some(duration_ms);
        }
    }

    pub fn recent(&self) -> Vec<HookRunRecord> {
        self.records
            .lock()
            .map(|records| records.iter().rev().cloned().collect())
            .unwrap_or_default()
    }
}
