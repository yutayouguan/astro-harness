//! Plugin Hooks：Agent 生命周期总线。

use std::collections::HashMap;
use std::sync::Arc;

use tracing::warn;

use crate::outcome::{HookOutcome, HookPayload};

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
        let callbacks = match self.hooks.lock() {
            Ok(map) => map.get(name).cloned().unwrap_or_default(),
            Err(_) => return HookOutcome::Continue,
        };
        for cb in callbacks {
            let outcome =
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb(payload))) {
                    Ok(o) => o,
                    Err(_) => {
                        warn!(hook = name, "plugin hook panicked; ignoring");
                        HookOutcome::Continue
                    }
                };
            match &outcome {
                HookOutcome::Continue | HookOutcome::Allow => continue,
                _ => return outcome,
            }
        }
        HookOutcome::Continue
    }

    /// 已注册的钩子名列表（测试用）。
    pub fn registered_names(&self) -> Vec<String> {
        self.hooks
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::{PRE_LLM_CALL, PRE_TOOL_CALL, PRE_VERIFY, TRANSFORM_TOOL_RESULT};
    use serde_json::json;

    #[test]
    fn fire_order_and_block() {
        let bus = PluginHookBus::new();
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log1 = Arc::clone(&log);
        bus.register(PRE_TOOL_CALL, move |_| {
            log1.lock().unwrap().push("a");
            HookOutcome::Continue
        });
        let log2 = Arc::clone(&log);
        bus.register(PRE_TOOL_CALL, move |_| {
            log2.lock().unwrap().push("b");
            HookOutcome::Block("nope".into())
        });
        let log3 = Arc::clone(&log);
        bus.register(PRE_TOOL_CALL, move |_| {
            log3.lock().unwrap().push("c");
            HookOutcome::Continue
        });
        let out = bus.fire(
            PRE_TOOL_CALL,
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
        bus.register(PRE_TOOL_CALL, |_| HookOutcome::Modify(json!({"x": 1})));
        let out = bus.fire(PRE_TOOL_CALL, &HookPayload::default());
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
        bus.register(PRE_VERIFY, |_| HookOutcome::KeepGoing("retry".into()));
        let out = bus.fire(PRE_VERIFY, &HookPayload::default());
        assert!(matches!(out, HookOutcome::KeepGoing(ref s) if s == "retry"));
    }
}
