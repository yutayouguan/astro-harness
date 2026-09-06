use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
struct DeniedAssessment {
    canonical_action: String,
}

#[derive(Debug, Clone)]
struct AuthorizedRetry {
    canonical_action: String,
    assessment_id: String,
}

/// Trusted, in-memory bridge from a denied assessment to one exact retry.
#[derive(Debug, Default)]
pub(crate) struct GuardianRetryState {
    denied: Mutex<HashMap<String, DeniedAssessment>>,
    authorized_once: Mutex<VecDeque<AuthorizedRetry>>,
}

impl GuardianRetryState {
    const MAX_PENDING_ASSESSMENTS: usize = 128;
    const MAX_AUTHORIZED_RETRIES: usize = 128;

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
        let mut authorized = self
            .authorized_once
            .lock()
            .expect("guardian retry mutex poisoned");
        if authorized.len() >= Self::MAX_AUTHORIZED_RETRIES {
            authorized.pop_front();
        }
        authorized.push_back(AuthorizedRetry {
            canonical_action: denied.canonical_action,
            assessment_id: assessment_id.to_string(),
        });
        true
    }

    pub(crate) fn consume_retry(&self, canonical_action: &str) -> Option<String> {
        let mut authorized = self
            .authorized_once
            .lock()
            .expect("guardian retry mutex poisoned");
        let index = authorized
            .iter()
            .position(|retry| retry.canonical_action == canonical_action)?;
        authorized.remove(index).map(|retry| retry.assessment_id)
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

    #[test]
    fn identical_actions_keep_independent_one_shot_approvals() {
        let state = GuardianRetryState::default();
        let action = GuardianRetryState::canonical_action(
            "exec_command",
            &serde_json::json!({"command":"rm one"}),
        );
        state.record_denied("first".into(), action.clone());
        state.record_denied("second".into(), action.clone());
        assert!(state.approve_denied("first"));
        assert!(state.approve_denied("second"));

        assert_eq!(state.consume_retry(&action).as_deref(), Some("first"));
        assert_eq!(state.consume_retry(&action).as_deref(), Some("second"));
        assert!(state.consume_retry(&action).is_none());
    }

    #[test]
    fn authorized_retry_queue_is_bounded() {
        let state = GuardianRetryState::default();
        for index in 0..=GuardianRetryState::MAX_AUTHORIZED_RETRIES {
            let action = format!("action-{index}");
            let assessment = format!("assessment-{index}");
            state.record_denied(assessment.clone(), action);
            assert!(state.approve_denied(&assessment));
        }

        assert!(state.consume_retry("action-0").is_none());
        assert_eq!(
            state.consume_retry("action-1").as_deref(),
            Some("assessment-1")
        );
    }
}
