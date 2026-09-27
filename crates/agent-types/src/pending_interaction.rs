//! Read-only UI projection of live control requests. No answers or drafts on disk.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::OnceLock;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionSnapshot {
    pub epoch: String,
    pub revision: u64,
    pub tasks: Vec<InteractionTask>,
    pub requests: Vec<PendingInteraction>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionTask {
    pub session_id: String,
    pub turn_id: String,
    pub title: String,
    pub project: String,
    pub parent_session_id: Option<String>,
    pub status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionAction {
    pub id: String,
    pub label: String,
    pub payload: Value,
    pub persistent: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PendingInteraction {
    pub key: String,
    pub session_id: String,
    pub turn_id: String,
    pub request_id: String,
    pub tool_call_id: String,
    pub kind: String,
    /// HITL 挂起原因（`confirmation` / `network_approval` / `input_required` …）。
    /// 消费端据此区分授权语义，不要再用动作 id 猜。
    pub reason: String,
    pub message: String,
    pub operations: Value,
    pub response_schema: Value,
    pub actions: Vec<InteractionAction>,
    pub expires_at: String,
    pub server_name: Option<String>,
    pub generation: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InteractionResponse {
    pub key: String,
    pub session_id: String,
    pub turn_id: String,
    pub action: String,
    pub payload: Value,
    #[serde(default)]
    pub confirmed_persistent: bool,
}

// Process-wide invalidation carries no request contents. Every service snapshots
// only its own live registries. Receivers subscribe before taking a snapshot.
pub fn changes() -> tokio::sync::watch::Receiver<u64> {
    signal().subscribe()
}
fn signal() -> &'static tokio::sync::watch::Sender<u64> {
    static SIGNAL: OnceLock<tokio::sync::watch::Sender<u64>> = OnceLock::new();
    SIGNAL.get_or_init(|| tokio::sync::watch::channel(0).0)
}
pub fn changed() {
    signal().send_modify(|revision| *revision = revision.wrapping_add(1));
}

/// Extract only producer-advertised actions; never infer broad permission grants
/// from a permissive schema or from the client's current provider/settings.
pub fn approval_actions(operations: &Value) -> Vec<InteractionAction> {
    fn collect(value: &Value, ids: &mut std::collections::BTreeSet<String>) {
        if let Some(name) = value.pointer("/action/event/name").and_then(Value::as_str) {
            ids.insert(name.into());
        }
        if value.get("variant").and_then(Value::as_str) == Some("approval") {
            ids.insert("approve".into());
            ids.insert("deny".into());
            if value.get("allowAlways").and_then(Value::as_bool) == Some(true) {
                ids.insert("approve_always".into());
            }
            if value
                .get("approvalTypeLabel")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty())
            {
                ids.insert("approve_type".into());
            }
        }
        match value {
            Value::Array(values) => {
                for v in values {
                    collect(v, ids);
                }
            }
            Value::Object(map) => {
                for v in map.values() {
                    collect(v, ids);
                }
            }
            _ => {}
        }
    }
    let mut ids = std::collections::BTreeSet::new();
    collect(operations, &mut ids);
    if !ids
        .iter()
        .any(|id| id.starts_with("approve") || id.starts_with("allow_"))
    {
        return vec![];
    }
    let mut actions: Vec<_> = ids
        .into_iter()
        .filter_map(|id| {
            let (label, payload, persistent) = match id.as_str() {
                "approve" => ("仅本次允许", json!({"approved":true}), false),
                "deny" => ("拒绝", json!({"approved":false}), false),
                "approve_always" => (
                    "永久允许此操作",
                    json!({"approved":true,"always":true,"scope":"exact"}),
                    true,
                ),
                "approve_type" => (
                    "永久允许同类操作",
                    json!({"approved":true,"always":true,"scope":"type"}),
                    true,
                ),
                "allow_once" => ("仅本次允许", json!({"scope":"allow_once"}), false),
                "allow_session" => ("本会话允许", json!({"scope":"allow_session"}), true),
                "allow_always" => ("永久允许", json!({"scope":"allow_always"}), true),
                _ => return None,
            };
            Some(InteractionAction {
                id,
                label: label.into(),
                payload,
                persistent,
            })
        })
        .collect();
    let order = [
        "approve",
        "allow_once",
        "deny",
        "allow_session",
        "approve_always",
        "allow_always",
        "approve_type",
    ];
    actions.sort_by_key(|action| {
        order
            .iter()
            .position(|id| *id == action.id)
            .unwrap_or(usize::MAX)
    });
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permission_options_require_explicit_advertisement() {
        assert!(approval_actions(&json!({})).is_empty());
        let a = approval_actions(
            &json!({"component":"ClarifyWizard","variant":"approval","allowAlways":false}),
        );
        assert_eq!(a.len(), 2);
        assert!(!a.iter().any(|a| a.persistent));
        let a = approval_actions(
            &json!({"variant":"approval","allowAlways":true,"approvalTypeLabel":"read"}),
        );
        assert_eq!(a.iter().filter(|a| a.persistent).count(), 2);
    }
}
