//! 将 Plugin 钩子推送到 UI 时间线（mpsc）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::names::{
    AGENT_END, PERMISSION_REQUEST, POST_API_REQUEST, POST_APPROVAL_RESPONSE, POST_LLM_CALL,
    POST_TOOL_USE, PRE_API_REQUEST, PRE_GATEWAY_DISPATCH, PRE_LLM_CALL, PRE_TOOL_USE,
    SESSION_FINALIZE, SESSION_RESET, SESSION_START, STOP, SUBAGENT_START, SUBAGENT_STOP,
    TRANSFORM_LLM_OUTPUT, TRANSFORM_TERMINAL_OUTPUT, TRANSFORM_TOOL_RESULT,
};
use crate::outcome::{HookOutcome, HookPayload};
use crate::plugin::PluginHookBus;

/// UI 可消费的钩子事件。
#[derive(Debug, Clone)]
pub struct UiHookEvent {
    pub name: String,
    pub detail: String,
    pub outcome: String,
}

const UI_HOOK_NAMES: &[&str] = &[
    SESSION_START,
    PRE_LLM_CALL,
    PRE_API_REQUEST,
    POST_API_REQUEST,
    PRE_TOOL_USE,
    POST_TOOL_USE,
    POST_LLM_CALL,
    AGENT_END,
    SESSION_FINALIZE,
    SESSION_RESET,
    SUBAGENT_STOP,
    PRE_GATEWAY_DISPATCH,
    STOP,
    SUBAGENT_START,
    PERMISSION_REQUEST,
    POST_APPROVAL_RESPONSE,
    TRANSFORM_TOOL_RESULT,
    TRANSFORM_TERMINAL_OUTPUT,
    TRANSFORM_LLM_OUTPUT,
];

fn detail_from_payload(payload: &HookPayload) -> String {
    if !payload.detail.is_empty() {
        payload.detail.clone()
    } else if let Some(t) = &payload.tool_name {
        format!(
            "{t} {}",
            payload
                .tool_input
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_default()
        )
    } else if let Some(prompt) = &payload.prompt {
        prompt.clone()
    } else if let Some(message) = &payload.last_assistant_message {
        message.clone()
    } else if let Some(n) = payload.system_prompt_chars {
        format!("system_prompt_chars={n}")
    } else if let Some(n) = payload.assistant_chars {
        format!("assistant_chars={n}")
    } else if let Some(t) = payload.turn {
        format!("turn={t}")
    } else {
        String::new()
    }
}

/// 可热替换 sender 的 UI 时间线槽：在 bus 上**只注册一次**，每轮 chat 换 `tx`。
#[derive(Clone, Default)]
pub struct UiTimelineSlot {
    state: Arc<Mutex<UiTimelineState>>,
}

#[derive(Default)]
struct UiTimelineState {
    next_generation: u64,
    active: HashMap<
        String,
        (
            UiTimelineGeneration,
            tokio::sync::mpsc::UnboundedSender<UiHookEvent>,
        ),
    >,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiTimelineGeneration(u64);

impl UiTimelineState {
    fn install(
        &mut self,
        session_id: &str,
        tx: tokio::sync::mpsc::UnboundedSender<UiHookEvent>,
    ) -> UiTimelineGeneration {
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = UiTimelineGeneration(self.next_generation);
        self.active.insert(session_id.to_string(), (generation, tx));
        generation
    }
}

impl UiTimelineSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置或清空当前 chat 的推送通道。
    pub fn set_tx(&self, tx: Option<tokio::sync::mpsc::UnboundedSender<UiHookEvent>>) {
        if let Ok(mut state) = self.state.lock() {
            match tx {
                Some(tx) => {
                    state.install("", tx);
                }
                None => {
                    state.active.remove("");
                }
            }
        }
    }

    pub fn install_tx(
        &self,
        session_id: &str,
        tx: tokio::sync::mpsc::UnboundedSender<UiHookEvent>,
    ) -> UiTimelineGeneration {
        self.state
            .lock()
            .expect("UI timeline slot mutex poisoned")
            .install(session_id, tx)
    }

    pub fn clear_if(&self, session_id: &str, generation: UiTimelineGeneration) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state
            .active
            .get(session_id)
            .is_some_and(|(active, _)| *active == generation)
        {
            state.active.remove(session_id);
            true
        } else {
            false
        }
    }

    /// 在 bus 上注册观察型推送（幂等：每个 slot 只应调用一次）。
    pub fn install(&self, bus: &PluginHookBus) {
        for &name in UI_HOOK_NAMES {
            let slot = Arc::clone(&self.state);
            let hook_name = name.to_string();
            bus.register(name, move |payload: &HookPayload| {
                if let Ok(g) = slot.lock() {
                    if let Some((_, tx)) = g.active.get(&payload.session_id) {
                        let _ = tx.send(UiHookEvent {
                            name: hook_name.clone(),
                            detail: detail_from_payload(payload),
                            outcome: "continue".into(),
                        });
                    }
                }
                HookOutcome::Continue
            });
        }
    }
}

/// 在 bus 上注册观察型推送（不影响 Continue）。
///
/// **注意：** 每次调用都会再注册一组 handler。进程级 UI 请用 [`UiTimelineSlot`]。
pub fn install_ui_timeline(
    bus: &PluginHookBus,
    tx: tokio::sync::mpsc::UnboundedSender<UiHookEvent>,
) {
    for &name in UI_HOOK_NAMES {
        let tx = tx.clone();
        let hook_name = name.to_string();
        bus.register(name, move |payload: &HookPayload| {
            let _ = tx.send(UiHookEvent {
                name: hook_name.clone(),
                detail: detail_from_payload(payload),
                outcome: "continue".into(),
            });
            HookOutcome::Continue
        });
    }
}

/// 测试用：记录触发过的钩子名。
pub fn install_recording(bus: &PluginHookBus, log: std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    for &name in UI_HOOK_NAMES {
        let log = std::sync::Arc::clone(&log);
        let hook_name = name.to_string();
        bus.register(name, move |payload: &HookPayload| {
            let label = match (
                &payload.tool_name,
                payload.assistant_chars,
                payload.system_prompt_chars,
            ) {
                (Some(t), _, _) => format!("{hook_name}:{t}"),
                (_, Some(n), _) => format!("{hook_name}:{n}"),
                (_, _, Some(n)) => format!("{hook_name}:{n}"),
                _ => hook_name.clone(),
            };
            if let Ok(mut g) = log.lock() {
                g.push(label);
            }
            HookOutcome::Continue
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn timeline_emits_canonical_name_for_canonical_fire() {
        let bus = PluginHookBus::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        install_ui_timeline(&bus, tx);

        bus.fire(crate::names::PRE_TOOL_USE, &crate::HookInput::default());

        assert_eq!(rx.try_recv().unwrap().name, crate::names::PRE_TOOL_USE);
    }

    #[test]
    fn detail_uses_canonical_input_fields() {
        let tool = HookPayload {
            tool_name: Some("exec_command".into()),
            tool_input: Some(json!({"command": "pwd"})),
            ..Default::default()
        };
        let prompt = HookPayload {
            prompt: Some("canonical prompt".into()),
            ..Default::default()
        };
        let assistant = HookPayload {
            last_assistant_message: Some("canonical response".into()),
            ..Default::default()
        };

        assert_eq!(detail_from_payload(&tool), "terminal {\"command\":\"pwd\"}");
        assert_eq!(detail_from_payload(&prompt), "canonical prompt");
        assert_eq!(detail_from_payload(&assistant), "canonical response");
    }

    #[test]
    fn slot_replaces_sender_without_restacking() {
        let bus = PluginHookBus::new();
        let slot = UiTimelineSlot::new();
        slot.install(&bus);

        let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel();
        slot.set_tx(Some(tx1));
        let _ = bus.fire(
            PRE_LLM_CALL,
            &HookPayload {
                system_prompt_chars: Some(3),
                ..Default::default()
            },
        );
        let ev = rx1.try_recv().expect("first tx");
        assert_eq!(ev.name, PRE_LLM_CALL);

        let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();
        slot.set_tx(Some(tx2));
        let _ = bus.fire(
            PRE_LLM_CALL,
            &HookPayload {
                system_prompt_chars: Some(9),
                ..Default::default()
            },
        );
        assert!(rx1.try_recv().is_err(), "old tx must be inactive");
        let ev2 = rx2.try_recv().expect("second tx");
        assert!(ev2.detail.contains('9'));
    }

    #[test]
    fn stale_generation_cannot_clear_replacement_sender() {
        let bus = PluginHookBus::new();
        let slot = UiTimelineSlot::new();
        slot.install(&bus);

        let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel();
        let first = slot.install_tx("", tx1);
        let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();
        let second = slot.install_tx("", tx2);

        assert!(!slot.clear_if("", first));
        let _ = bus.fire(
            PRE_LLM_CALL,
            &HookPayload {
                system_prompt_chars: Some(11),
                ..Default::default()
            },
        );
        assert!(rx1.try_recv().is_err());
        assert_eq!(rx2.try_recv().unwrap().name, PRE_LLM_CALL);
        assert!(slot.clear_if("", second));
    }

    #[test]
    fn concurrent_sessions_route_hooks_to_independent_generations() {
        let bus = PluginHookBus::new();
        let slot = UiTimelineSlot::new();
        slot.install(&bus);
        let (tx_a, mut rx_a) = tokio::sync::mpsc::unbounded_channel();
        let (tx_b, mut rx_b) = tokio::sync::mpsc::unbounded_channel();
        let generation_a = slot.install_tx("session-a", tx_a);
        let generation_b = slot.install_tx("session-b", tx_b);

        let _ = bus.fire(
            PRE_LLM_CALL,
            &HookPayload {
                session_id: "session-a".into(),
                system_prompt_chars: Some(7),
                ..Default::default()
            },
        );
        assert_eq!(rx_a.try_recv().unwrap().name, PRE_LLM_CALL);
        assert!(rx_b.try_recv().is_err());

        let _ = bus.fire(
            PRE_LLM_CALL,
            &HookPayload {
                session_id: "session-b".into(),
                system_prompt_chars: Some(11),
                ..Default::default()
            },
        );
        assert_eq!(rx_b.try_recv().unwrap().name, PRE_LLM_CALL);
        assert!(rx_a.try_recv().is_err());

        assert!(slot.clear_if("session-a", generation_a));
        let _ = bus.fire(
            PRE_LLM_CALL,
            &HookPayload {
                session_id: "session-b".into(),
                ..Default::default()
            },
        );
        assert_eq!(rx_b.try_recv().unwrap().name, PRE_LLM_CALL);
        assert!(slot.clear_if("session-b", generation_b));
    }
}
