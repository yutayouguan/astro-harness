//! 带角色的 prompt 上下文持久化基线。
//!
//! 稳定的基础指令和原生工具 schema 被有意排除在此状态之外。
//! 负载遵循 world-state 契约：首次快照为全量，后续变更为增量补丁，
//! 未改变的状态不产生 rollout 条目。

use agent_rollout::RolloutItem;
use serde::Serialize;
use serde_json::{Map, Value};

use super::contract::{PromptContextRole, PromptContextSection};
use super::PromptContract;

const PROMPT_CONTEXT_KEY: &str = "astro.prompt_context.v1";

#[derive(Serialize)]
struct PromptContextSnapshot<'a> {
    version: u32,
    messages: &'a [agent_protocol::ResponseItem],
    sections: &'a [PromptContextSection],
    usage: PromptContextUsage<'a>,
}

#[derive(Serialize)]
struct PromptContextUsage<'a> {
    developer: &'a [super::contract::PromptSourceUsage],
    user: &'a [super::contract::PromptSourceUsage],
}

pub(crate) fn snapshot(prompt: &PromptContract) -> serde_json::Result<Value> {
    serde_json::to_value(PromptContextSnapshot {
        version: 2,
        messages: &prompt.context,
        sections: &prompt.context_sections,
        usage: PromptContextUsage {
            developer: &prompt.usage.developer,
            user: &prompt.usage.user,
        },
    })
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RestoredPromptContext {
    pub(crate) snapshot: Option<Value>,
    pub(crate) history: Vec<PromptContextEvent>,
}

/// 仅供 Provider 使用的上下文，在特定用户消息序号之前注入。
#[derive(Debug, Clone)]
pub(crate) struct PromptContextEvent {
    pub(crate) before_user: usize,
    pub(crate) messages: Vec<agent_protocol::ResponseItem>,
}

impl PromptContextEvent {
    pub(crate) fn new(before_user: usize, messages: Vec<agent_protocol::ResponseItem>) -> Self {
        Self {
            before_user,
            messages,
        }
    }
}

pub(crate) fn snapshot_messages(snapshot: &Value) -> Option<Vec<agent_protocol::ResponseItem>> {
    serde_json::from_value(snapshot.get("messages")?.clone()).ok()
}

fn snapshot_sections(snapshot: &Value) -> Option<Vec<PromptContextSection>> {
    serde_json::from_value(snapshot.get("sections")?.clone()).ok()
}

fn context_update(id: &str, content: Option<&str>) -> String {
    match content {
        Some(content) => format!(
            "<context_update source=\"{id}\">\n\
             The following context supersedes the earlier `{id}` section.\n\
             {content}\n\
             </context_update>"
        ),
        None => format!(
            "<context_update source=\"{id}\">\n\
             The earlier `{id}` context section no longer applies.\n\
             </context_update>"
        ),
    }
}

fn role_message(
    role: PromptContextRole,
    updates: impl IntoIterator<Item = String>,
) -> Option<agent_protocol::ResponseItem> {
    let body = updates.into_iter().collect::<Vec<_>>().join("\n\n");
    if body.is_empty() {
        return None;
    }
    Some(match role {
        PromptContextRole::Developer => agent_protocol::ResponseItem::developer_text(body),
        PromptContextRole::User => agent_protocol::ResponseItem::user_text(body),
    })
}

fn section_updates(previous: &Value, current: &Value) -> Option<Vec<agent_protocol::ResponseItem>> {
    let previous_messages = snapshot_messages(previous).unwrap_or_default();
    let current_messages = snapshot_messages(current).unwrap_or_default();
    let previous = snapshot_sections(previous)?;
    let current = snapshot_sections(current)?;
    if (previous.is_empty() && !previous_messages.is_empty())
        || (current.is_empty() && !current_messages.is_empty())
    {
        return None;
    }
    let mut updates = Vec::new();

    for role in [PromptContextRole::Developer, PromptContextRole::User] {
        let old = previous
            .iter()
            .filter(|section| section.role == role)
            .map(|section| (section.id.as_str(), section.content.as_str()))
            .collect::<std::collections::HashMap<_, _>>();
        let new = current
            .iter()
            .filter(|section| section.role == role)
            .map(|section| (section.id.as_str(), section.content.as_str()))
            .collect::<std::collections::HashMap<_, _>>();
        let changed = current
            .iter()
            .filter(|section| section.role == role)
            .filter(|section| {
                old.get(section.id.as_str()).copied() != Some(section.content.as_str())
            })
            .map(|section| context_update(&section.id, Some(&section.content)))
            .chain(
                previous
                    .iter()
                    .filter(|section| {
                        section.role == role && !new.contains_key(section.id.as_str())
                    })
                    .map(|section| context_update(&section.id, None)),
            );
        if let Some(message) = role_message(role, changed) {
            updates.push(message);
        }
    }
    Some(updates)
}

fn role_text(messages: &[agent_protocol::ResponseItem], role: &str) -> Option<String> {
    let body = messages
        .iter()
        .filter(|message| message.role() == Some(role))
        .map(agent_protocol::ResponseItem::text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (!body.is_empty()).then_some(body)
}

/// 渲染两个持久化快照之间模型可见的变更。
///
/// Version 2 快照按稳定 source id 进行 diff。Version 1 快照回退为整体替换已变更的角色，
/// 以确保旧版本写入的 rollout 仍可恢复。
pub(crate) fn model_updates(
    previous: Option<&Value>,
    current: &Value,
) -> Vec<agent_protocol::ResponseItem> {
    let Some(previous) = previous else {
        return snapshot_messages(current).unwrap_or_default();
    };
    if let Some(updates) = section_updates(previous, current) {
        return updates;
    }

    let old = snapshot_messages(previous).unwrap_or_default();
    let new = snapshot_messages(current).unwrap_or_default();
    let mut updates = Vec::new();
    for (role, context_role) in [
        ("developer", PromptContextRole::Developer),
        ("user", PromptContextRole::User),
    ] {
        let old_text = role_text(&old, role);
        let new_text = role_text(&new, role);
        if old_text != new_text {
            if let Some(message) =
                role_message(context_role, [context_update(role, new_text.as_deref())])
            {
                updates.push(message);
            }
        }
    }
    updates
}

pub(crate) fn rollout_update(
    previous: Option<&Value>,
    current: &Value,
    before_user: usize,
) -> Option<RolloutItem> {
    if previous == Some(current) {
        return None;
    }

    let mut state = Map::new();
    state.insert(PROMPT_CONTEXT_KEY.to_string(), current.clone());
    Some(RolloutItem::WorldState(serde_json::json!({
        "full": previous.is_none(),
        "before_user": before_user,
        "state": state,
    })))
}

/// 重放全量快照和增量补丁，返回最新的 prompt-context 基线。
///
/// 未知的遗留 `WorldState` 负载会被忽略。格式正确但不含本命名空间的全量快照
/// 会清除基线，与全量 world state 的替换语义一致。
pub(crate) fn restore(items: &[RolloutItem]) -> RestoredPromptContext {
    let mut restored = RestoredPromptContext::default();
    let mut user_count = 0usize;
    for item in items {
        match item {
            RolloutItem::Compacted(_) => {
                restored = RestoredPromptContext::default();
                user_count = 0;
                continue;
            }
            RolloutItem::ResponseItem(agent_protocol::ResponseItem::Message { role, .. })
                if role == "user" =>
            {
                user_count += 1;
                continue;
            }
            _ => {}
        }
        let RolloutItem::WorldState(payload) = item else {
            continue;
        };
        let Some(full) = payload.get("full").and_then(Value::as_bool) else {
            continue;
        };
        let Some(state) = payload.get("state").and_then(Value::as_object) else {
            continue;
        };
        // Version 2 rollout 携带精确的插入位置。旧版 rollout 在当前用户输入之后
        // 立即写入状态，因此前一个用户消息位置是安全的回退值。
        let before_user = payload
            .get("before_user")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or_else(|| user_count.saturating_sub(1));

        if full {
            restored.snapshot = state.get(PROMPT_CONTEXT_KEY).cloned();
            let messages = restored
                .snapshot
                .as_ref()
                .and_then(snapshot_messages)
                .unwrap_or_default();
            restored.history = (!messages.is_empty())
                .then(|| PromptContextEvent::new(before_user, messages))
                .into_iter()
                .collect();
        } else if let Some(value) = state.get(PROMPT_CONTEXT_KEY) {
            if value.is_null() {
                restored = RestoredPromptContext::default();
            } else {
                let messages = model_updates(restored.snapshot.as_ref(), value);
                if !messages.is_empty() {
                    restored
                        .history
                        .push(PromptContextEvent::new(before_user, messages));
                }
                restored.snapshot = Some(value.clone());
            }
        }
    }
    restored
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(text: &str) -> PromptContract {
        PromptContract {
            base_instructions: "stable base".into(),
            context: vec![agent_protocol::ResponseItem::developer_text(text)],
            context_sections: vec![PromptContextSection {
                id: "mode".into(),
                role: PromptContextRole::Developer,
                content: text.into(),
            }],
            usage: Default::default(),
        }
    }

    #[test]
    fn writes_full_then_patch_and_skips_unchanged_context() {
        let first = snapshot(&prompt("first")).unwrap();
        let second = snapshot(&prompt("second")).unwrap();

        let full = rollout_update(None, &first, 0).unwrap();
        assert_eq!(
            match &full {
                RolloutItem::WorldState(value) => value["full"].as_bool(),
                _ => None,
            },
            Some(true)
        );
        assert!(rollout_update(Some(&first), &first, 1).is_none());

        let patch = rollout_update(Some(&first), &second, 1).unwrap();
        assert_eq!(
            match &patch {
                RolloutItem::WorldState(value) => value["full"].as_bool(),
                _ => None,
            },
            Some(false)
        );
        let restored = restore(&[full, patch]);
        assert_eq!(restored.snapshot, Some(second));
        assert_eq!(restored.history.len(), 2);
        assert_eq!(restored.history[0].before_user, 0);
        assert_eq!(restored.history[1].before_user, 1);
        assert_eq!(restored.history[1].messages[0].role(), Some("developer"));
        assert!(restored.history[1].messages[0].text().contains("second"));
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
        first
            .context
            .push(agent_protocol::ResponseItem::user_text("changed context"));
        assert_ne!(snapshot(&first).unwrap(), snapshot(&second).unwrap());
    }

    #[test]
    fn ignores_legacy_world_state_and_honors_full_replacement() {
        let value = snapshot(&prompt("current")).unwrap();
        let full = rollout_update(None, &value, 0).unwrap();
        let legacy = RolloutItem::WorldState(serde_json::json!({"cwd": "/tmp"}));
        let replacement = RolloutItem::WorldState(serde_json::json!({
            "full": true,
            "state": {"other.namespace": {"enabled": true}},
        }));

        assert_eq!(restore(&[legacy, full.clone()]).snapshot, Some(value));
        let restored = restore(&[full, replacement]);
        assert!(restored.snapshot.is_none());
        assert!(restored.history.is_empty());
    }

    #[test]
    fn diffs_only_the_changed_source_and_resets_after_compaction() {
        let mut first = prompt("mode-one");
        first.context.push(agent_protocol::ResponseItem::user_text(
            "time-one\n\nproject",
        ));
        first.context_sections.push(PromptContextSection {
            id: "timestamp".into(),
            role: PromptContextRole::User,
            content: "time-one".into(),
        });
        first.context_sections.push(PromptContextSection {
            id: "agents".into(),
            role: PromptContextRole::User,
            content: "project".into(),
        });
        let mut second = first.clone();
        second.context[1] = agent_protocol::ResponseItem::user_text("time-two\n\nproject");
        second.context_sections[1].content = "time-two".into();

        let first = snapshot(&first).unwrap();
        let second = snapshot(&second).unwrap();
        let updates = model_updates(Some(&first), &second);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].role(), Some("user"));
        assert!(updates[0].text().contains("time-two"));
        assert!(!updates[0].text().contains("project"));

        let restored = restore(&[
            rollout_update(None, &first, 0).unwrap(),
            rollout_update(Some(&first), &second, 1).unwrap(),
            RolloutItem::Compacted(serde_json::json!({"summary": "done"})),
        ]);
        assert!(restored.snapshot.is_none());
        assert!(restored.history.is_empty());
    }

    #[test]
    fn version_one_snapshot_falls_back_to_role_replacement() {
        let mut first = snapshot(&prompt("first")).unwrap();
        let mut second = snapshot(&prompt("second")).unwrap();
        for value in [&mut first, &mut second] {
            value["version"] = 1.into();
            value.as_object_mut().unwrap().remove("sections");
        }

        let updates = model_updates(Some(&first), &second);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].role(), Some("developer"));
        assert!(updates[0].text().contains("second"));
    }

    #[test]
    fn restores_legacy_positions_at_the_preceding_user_boundary() {
        let first = snapshot(&prompt("first")).unwrap();
        let second = snapshot(&prompt("second")).unwrap();
        let mut full = rollout_update(None, &first, 99).unwrap();
        let mut patch = rollout_update(Some(&first), &second, 99).unwrap();
        for item in [&mut full, &mut patch] {
            let RolloutItem::WorldState(value) = item else {
                unreachable!();
            };
            value.as_object_mut().unwrap().remove("before_user");
        }

        let response = |item| RolloutItem::ResponseItem(item);
        let restored = restore(&[
            response(agent_protocol::ResponseItem::user_text("first")),
            full,
            response(agent_protocol::ResponseItem::assistant_text("answer")),
            response(agent_protocol::ResponseItem::user_text("second")),
            patch,
        ]);

        assert_eq!(restored.history.len(), 2);
        assert_eq!(restored.history[0].before_user, 0);
        assert_eq!(restored.history[1].before_user, 1);
    }
}
