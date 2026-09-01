use std::collections::HashMap;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
struct DeniedAssessment {
    canonical_action: String,
}

/// Trusted, in-memory bridge from a denied assessment to one exact retry.
#[derive(Debug, Default)]
pub(crate) struct GuardianRetryState {
    denied: Mutex<HashMap<String, DeniedAssessment>>,
    authorized_once: Mutex<HashMap<String, String>>,
}

impl GuardianRetryState {
    const MAX_PENDING_ASSESSMENTS: usize = 128;

    pub(crate) fn canonical_action(tool_name: &str, arguments: &serde_json::Value) -> String {
        let mut hasher = Sha256::new();
        hasher.update(tool_name.as_bytes());
        hasher.update([0]);
        hasher.update(serde_json::to_vec(arguments).unwrap_or_default());
        format!("sha256:{:x}", hasher.finalize())
    }

    pub(crate) fn record_denied(&self, assessment_id: String, canonical_action: String) {
        let mut denied = self.denied.lock().expect("guardian denied mutex poisoned");
        if denied.len() >= Self::MAX_PENDING_ASSESSMENTS {
            if let Some(stale_id) = denied.keys().next().cloned() {
                denied.remove(&stale_id);
            }
        }
        denied.insert(assessment_id, DeniedAssessment { canonical_action });
    }

    pub(crate) fn approve_denied(&self, assessment_id: &str) -> bool {
        let Some(denied) = self
            .denied
            .lock()
            .expect("guardian denied mutex poisoned")
            .remove(assessment_id)
        else {
            return false;
        };
        self.authorized_once
            .lock()
            .expect("guardian retry mutex poisoned")
            .insert(denied.canonical_action, assessment_id.to_string());
        true
    }

    pub(crate) fn consume_retry(&self, canonical_action: &str) -> Option<String> {
        self.authorized_once
            .lock()
            .expect("guardian retry mutex poisoned")
            .remove(canonical_action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_is_bound_to_one_exact_retry() {
        let state = GuardianRetryState::default();
        let action = GuardianRetryState::canonical_action(
            "exec_command",
            &serde_json::json!({"command":"rm one"}),
        );
        state.record_denied("assessment".into(), action.clone());
        assert!(state.approve_denied("assessment"));
        assert_eq!(state.consume_retry(&action).as_deref(), Some("assessment"));
        assert!(state.consume_retry(&action).is_none());
        assert!(!state.approve_denied("assessment"));
    }
}
