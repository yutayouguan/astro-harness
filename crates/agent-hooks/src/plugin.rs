//! Plugin Hooks：Agent 生命周期总线。

use std::collections::HashMap;
use std::sync::Arc;

use tracing::warn;

use crate::outcome::{HookOutcome, HookPayload, PermissionRequestDecision, PostToolUseDecision};

/// 同步钩子回调（panic 会被捕获为 Continue）。
pub type HookFn = Arc<dyn Fn(&HookPayload) -> HookOutcome + Send + Sync>;

/// 按钩子名保存有序回调列表。
#[derive(Default, Clone)]
pub struct PluginHookBus {
    hooks: Arc<std::sync::Mutex<HashMap<String, Vec<HookFn>>>>,
}

impl std::fmt::Debug for PluginHookBus {
    /// 回调本身不可打印；仅展示已注册的钩子名，供上层 `Debug` 派生（如 `DelegateRunRequest`）复用。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginHookBus")
            .field("hook_names", &self.registered_names())
            .finish()
    }
}

impl PluginHookBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个命名钩子（追加顺序）。
    pub fn register<F>(&self, name: impl Into<String>, f: F)
    where
        F: Fn(&HookPayload) -> HookOutcome + Send + Sync + 'static,
    {
        let name = name.into();
        if let Ok(mut map) = self.hooks.lock() {
            map.entry(name).or_default().push(Arc::new(f));
        }
    }

    /// 触发钩子；对可短路结果返回首个非 Continue/Allow。
    pub fn fire(&self, name: &str, payload: &HookPayload) -> HookOutcome {
        let payload = payload.for_event(name);
        let callbacks = self.callbacks(name);
        for cb in callbacks {
            let outcome = invoke_callback(name, &payload, &cb);
            match &outcome {
                HookOutcome::Continue | HookOutcome::Allow => continue,
                _ => return outcome,
            }
        }
        HookOutcome::Continue
    }

    /// `PermissionRequest` aggregation: any deny wins, otherwise an
    /// explicit allow wins, otherwise the normal approval flow continues.
    pub fn fire_permission_request(&self, payload: &HookPayload) -> PermissionRequestDecision {
        let payload = payload.for_event(crate::PERMISSION_REQUEST);
        let mut decision = PermissionRequestDecision::Abstain;
        for callback in self.callbacks(crate::PERMISSION_REQUEST) {
            match invoke_callback(crate::PERMISSION_REQUEST, &payload, &callback) {
                HookOutcome::Block(reason) => {
                    return PermissionRequestDecision::Deny(reason);
                }
                HookOutcome::Allow => decision = PermissionRequestDecision::Allow,
                _ => {}
            }
        }
        decision
    }

    /// `PostToolUse` aggregation. Tool side effects have already
    /// happened, so block affects only the model-visible result.
    pub fn fire_post_tool_use(&self, payload: &HookPayload) -> PostToolUseDecision {
        let payload = payload.for_event(crate::POST_TOOL_USE);
        let mut decision = PostToolUseDecision::default();
        for callback in self.callbacks(crate::POST_TOOL_USE) {
            match invoke_callback(crate::POST_TOOL_USE, &payload, &callback) {
                HookOutcome::Block(reason) => {
                    if decision.block_reason.is_none() {
                        decision.block_reason = Some(reason);
                    }
                }
                HookOutcome::InjectContext(context) => {
                    decision.additional_contexts.push(context);
                }
                HookOutcome::ReplaceText(feedback) => {
                    decision.feedback_messages.push(feedback);
                }
                _ => {}
            }
        }
        decision
    }

    /// `SubagentStart` is context-injection-only. Stop/block outcomes
    /// are deliberately ignored, while context from every handler is kept.
    pub fn fire_subagent_start(&self, payload: &HookPayload) -> Option<String> {
        let payload = payload.for_event(crate::SUBAGENT_START);
        let contexts = self
            .callbacks(crate::SUBAGENT_START)
            .into_iter()
            .filter_map(|callback| {
                match invoke_callback(crate::SUBAGENT_START, &payload, &callback) {
                    HookOutcome::InjectContext(context) => Some(context),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        (!contexts.is_empty()).then(|| contexts.join("\n\n"))
    }

    /// `SubagentStop` evaluates every handler and combines all
    /// continuation prompts for the same terminal candidate.
    pub fn fire_subagent_stop(&self, payload: &HookPayload) -> HookOutcome {
        let payload = payload.for_event(crate::SUBAGENT_STOP);
        let prompts = self
            .callbacks(crate::SUBAGENT_STOP)
            .into_iter()
            .filter_map(|callback| {
                match invoke_callback(crate::SUBAGENT_STOP, &payload, &callback) {
                    HookOutcome::KeepGoing(prompt) => Some(prompt),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        if prompts.is_empty() {
            HookOutcome::Continue
        } else {
            HookOutcome::KeepGoing(prompts.join("\n\n"))
        }
    }

    fn callbacks(&self, name: &str) -> Vec<HookFn> {
        self.hooks
            .lock()
            .map(|map| map.get(name).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    /// 已注册的钩子名列表（测试用）。
    pub fn registered_names(&self) -> Vec<String> {
        self.hooks
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }
}

fn invoke_callback(name: &str, payload: &HookPayload, callback: &HookFn) -> HookOutcome {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(payload))) {
        Ok(outcome) => outcome,
        Err(_) => {
            warn!(hook = %name, "plugin hook panicked; ignoring");
            HookOutcome::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::{PRE_LLM_CALL, PRE_TOOL_USE, STOP, TRANSFORM_TOOL_RESULT};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn legacy_registration_does_not_receive_canonical_fire() {
        let bus = PluginHookBus::new();
        let seen = Arc::new(std::sync::Mutex::new(None));
        let hits = Arc::new(AtomicUsize::new(0));
        let capture = Arc::clone(&seen);
        let hit_count = Arc::clone(&hits);
        bus.register("pre_tool_call", move |input| {
            *capture.lock().unwrap() = Some(input.hook_event_name.clone());
            hit_count.fetch_add(1, Ordering::SeqCst);
            HookOutcome::Continue
        });

        bus.fire(PRE_TOOL_USE, &crate::HookInput::default());

        assert_eq!(seen.lock().unwrap().as_deref(), None);
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert_eq!(bus.registered_names(), vec!["pre_tool_call"]);
    }

    #[test]
    fn canonical_registration_does_not_receive_legacy_fire() {
        let bus = PluginHookBus::new();
        let seen = Arc::new(std::sync::Mutex::new(None));
        let capture = Arc::clone(&seen);
        bus.register(PRE_TOOL_USE, move |input| {
            *capture.lock().unwrap() = Some(input.hook_event_name.clone());
            HookOutcome::Continue
        });

        bus.fire("pre_tool_call", &crate::HookInput::default());

        assert_eq!(seen.lock().unwrap().as_deref(), None);
    }

    #[test]
    fn unknown_custom_event_name_is_preserved() {
        let bus = PluginHookBus::new();
        let seen = Arc::new(std::sync::Mutex::new(None));
        let capture = Arc::clone(&seen);
        bus.register("acme:custom_event", move |input| {
            *capture.lock().unwrap() = Some(input.hook_event_name.clone());
            HookOutcome::Continue
        });

        bus.fire("acme:custom_event", &crate::HookInput::default());

        assert_eq!(seen.lock().unwrap().as_deref(), Some("acme:custom_event"));
    }

    #[test]
    fn fire_order_and_block() {
        let bus = PluginHookBus::new();
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log1 = Arc::clone(&log);
        bus.register(PRE_TOOL_USE, move |_| {
            log1.lock().unwrap().push("a");
            HookOutcome::Continue
        });
        let log2 = Arc::clone(&log);
        bus.register(PRE_TOOL_USE, move |_| {
            log2.lock().unwrap().push("b");
            HookOutcome::Block("nope".into())
        });
        let log3 = Arc::clone(&log);
        bus.register(PRE_TOOL_USE, move |_| {
            log3.lock().unwrap().push("c");
            HookOutcome::Continue
        });
        let out = bus.fire(
            PRE_TOOL_USE,
            &HookPayload {
                tool_name: Some("echo".into()),
                ..Default::default()
            },
        );
        assert!(matches!(out, HookOutcome::Block(ref s) if s == "nope"));
        assert_eq!(*log.lock().unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn inject_context_short_circuits() {
        let bus = PluginHookBus::new();
        bus.register(PRE_LLM_CALL, |_| HookOutcome::InjectContext("extra".into()));
        bus.register(PRE_LLM_CALL, |_| {
            HookOutcome::InjectContext("ignored".into())
        });
        let out = bus.fire(PRE_LLM_CALL, &HookPayload::default());
        assert!(matches!(out, HookOutcome::InjectContext(ref s) if s == "extra"));
    }

    #[test]
    fn modify_args() {
        let bus = PluginHookBus::new();
        bus.register(PRE_TOOL_USE, |_| HookOutcome::Modify(json!({"x": 1})));
        let out = bus.fire(PRE_TOOL_USE, &HookPayload::default());
        assert!(matches!(out, HookOutcome::Modify(ref v) if v["x"] == 1));
    }

    #[test]
    fn transform_replace_text_short_circuits() {
        let bus = PluginHookBus::new();
        bus.register(TRANSFORM_TOOL_RESULT, |_| {
            HookOutcome::ReplaceText("a".into())
        });
        bus.register(TRANSFORM_TOOL_RESULT, |_| {
            HookOutcome::ReplaceText("b".into())
        });
        let out = bus.fire(TRANSFORM_TOOL_RESULT, &HookPayload::default());
        assert!(matches!(out, HookOutcome::ReplaceText(ref s) if s == "a"));
    }

    #[test]
    fn keep_going_short_circuits() {
        let bus = PluginHookBus::new();
        bus.register(STOP, |_| HookOutcome::KeepGoing("retry".into()));
        let out = bus.fire(STOP, &HookPayload::default());
        assert!(matches!(out, HookOutcome::KeepGoing(ref s) if s == "retry"));
    }

    #[test]
    fn permission_request_denial_wins_over_allow() {
        let bus = PluginHookBus::new();
        bus.register(crate::PERMISSION_REQUEST, |_| HookOutcome::Allow);
        bus.register(crate::PERMISSION_REQUEST, |_| {
            HookOutcome::Block("policy denied".into())
        });

        assert_eq!(
            bus.fire_permission_request(&HookPayload::default()),
            PermissionRequestDecision::Deny("policy denied".into())
        );
    }

    #[test]
    fn post_tool_use_aggregates_block_context_and_feedback() {
        let bus = PluginHookBus::new();
        bus.register(crate::POST_TOOL_USE, |_| {
            HookOutcome::InjectContext("context".into())
        });
        bus.register(crate::POST_TOOL_USE, |_| {
            HookOutcome::ReplaceText("feedback".into())
        });
        bus.register(crate::POST_TOOL_USE, |_| {
            HookOutcome::Block("blocked".into())
        });

        assert_eq!(
            bus.fire_post_tool_use(&HookPayload::default()),
            PostToolUseDecision {
                block_reason: Some("blocked".into()),
                additional_contexts: vec!["context".into()],
                feedback_messages: vec!["feedback".into()],
            }
        );
    }

    #[test]
    fn subagent_start_ignores_block_and_aggregates_context() {
        let bus = PluginHookBus::new();
        bus.register(crate::SUBAGENT_START, |_| {
            HookOutcome::Block("ignored".into())
        });
        bus.register(crate::SUBAGENT_START, |_| {
            HookOutcome::InjectContext("first".into())
        });
        bus.register(crate::SUBAGENT_START, |_| {
            HookOutcome::InjectContext("second".into())
        });

        assert_eq!(
            bus.fire_subagent_start(&HookPayload::default()).as_deref(),
            Some("first\n\nsecond")
        );
    }

    #[test]
    fn subagent_stop_aggregates_continuation_prompts() {
        let bus = PluginHookBus::new();
        bus.register(crate::SUBAGENT_STOP, |_| {
            HookOutcome::KeepGoing("first".into())
        });
        bus.register(crate::SUBAGENT_STOP, |_| {
            HookOutcome::KeepGoing("second".into())
        });

        assert!(matches!(
            bus.fire_subagent_stop(&HookPayload::default()),
            HookOutcome::KeepGoing(prompt) if prompt == "first\n\nsecond"
        ));
    }
}
