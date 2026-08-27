//! Durable baseline for role-bearing prompt context.
//!
//! Stable base instructions and native tool schemas deliberately stay outside this state. The
//! payload mirrors Codex's world-state contract: the first snapshot is full, later changes are
//! patches, and unchanged state produces no rollout item.

use agent_rollout::RolloutItem;
use serde::Serialize;
use serde_json::{Map, Value};

use super::PromptContract;

const PROMPT_CONTEXT_KEY: &str = "astro.prompt_context.v1";

#[derive(Serialize)]
struct PromptContextSnapshot<'a> {
    version: u32,
    messages: &'a [providers::types::message::Message],
    usage: PromptContextUsage<'a>,
}

#[derive(Serialize)]
struct PromptContextUsage<'a> {
    developer: &'a [super::contract::PromptSourceUsage],
    user: &'a [super::contract::PromptSourceUsage],
}

pub(crate) fn snapshot(prompt: &PromptContract) -> serde_json::Result<Value> {
    serde_json::to_value(PromptContextSnapshot {
        version: 1,
        messages: &prompt.context,
        usage: PromptContextUsage {
            developer: &prompt.usage.developer,
            user: &prompt.usage.user,
        },
    })
}

pub(crate) fn rollout_update(previous: Option<&Value>, current: &Value) -> Option<RolloutItem> {
    if previous == Some(current) {
        return None;
    }

    let mut state = Map::new();
    state.insert(PROMPT_CONTEXT_KEY.to_string(), current.clone());
    Some(RolloutItem::WorldState(serde_json::json!({
        "full": previous.is_none(),
        "state": state,
    })))
}

/// Replay full snapshots and patches, returning the latest prompt-context baseline.
///
/// Unknown legacy `WorldState` payloads are ignored. A well-formed full snapshot without this
/// namespace clears the baseline, matching the replacement semantics of a full world state.
pub(crate) fn restore(items: &[RolloutItem]) -> Option<Value> {
    let mut baseline = None;
    for item in items {
        let RolloutItem::WorldState(payload) = item else {
            continue;
        };
        let Some(full) = payload.get("full").and_then(Value::as_bool) else {
            continue;
        };
        let Some(state) = payload.get("state").and_then(Value::as_object) else {
            continue;
        };

        if full {
            baseline = state.get(PROMPT_CONTEXT_KEY).cloned();
        } else if let Some(value) = state.get(PROMPT_CONTEXT_KEY) {
            baseline = (!value.is_null()).then(|| value.clone());
        }
    }
    baseline
}

#[cfg(test)]
mod tests {
    use providers::types::message::Message;

    use super::*;

    fn prompt(text: &str) -> PromptContract {
        PromptContract {
            base_instructions: "stable base".into(),
            context: vec![Message::developer(text)],
            usage: Default::default(),
        }
    }

    #[test]
    fn writes_full_then_patch_and_skips_unchanged_context() {
        let first = snapshot(&prompt("first")).unwrap();
        let second = snapshot(&prompt("second")).unwrap();

        let full = rollout_update(None, &first).unwrap();
        assert_eq!(
            match &full {
                RolloutItem::WorldState(value) => value["full"].as_bool(),
                _ => None,
            },
            Some(true)
        );
        assert!(rollout_update(Some(&first), &first).is_none());

        let patch = rollout_update(Some(&first), &second).unwrap();
        assert_eq!(
            match &patch {
                RolloutItem::WorldState(value) => value["full"].as_bool(),
                _ => None,
            },
            Some(false)
        );
        assert_eq!(restore(&[full, patch]), Some(second));
    }

    #[test]
    fn snapshot_excludes_stable_base_instructions() {
        let mut first = prompt("context");
        let mut second = first.clone();
        second.base_instructions = "different stable base".into();
        second
            .usage
            .base
            .push(super::super::contract::PromptSourceUsage {
                id: "base".into(),
                chars: 999,
            });

        assert_eq!(snapshot(&first).unwrap(), snapshot(&second).unwrap());
        first.context.push(Message::user_text("changed context"));
        assert_ne!(snapshot(&first).unwrap(), snapshot(&second).unwrap());
    }

    #[test]
    fn ignores_legacy_world_state_and_honors_full_replacement() {
        let value = snapshot(&prompt("current")).unwrap();
        let full = rollout_update(None, &value).unwrap();
        let legacy = RolloutItem::WorldState(serde_json::json!({"cwd": "/tmp"}));
        let replacement = RolloutItem::WorldState(serde_json::json!({
            "full": true,
            "state": {"other.namespace": {"enabled": true}},
        }));

        assert_eq!(restore(&[legacy, full.clone()]), Some(value));
        assert_eq!(restore(&[full, replacement]), None);
    }
}
