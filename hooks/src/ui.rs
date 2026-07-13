//! 将 Plugin 钩子推送到 UI 时间线（mpsc）。

use crate::names::{
    ON_SESSION_END, ON_SESSION_FINALIZE, ON_SESSION_RESET, ON_SESSION_START, POST_API_REQUEST,
    POST_LLM_CALL, POST_TOOL_CALL, PRE_API_REQUEST, PRE_GATEWAY_DISPATCH, PRE_LLM_CALL,
    PRE_TOOL_CALL, SUBAGENT_STOP,
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

/// 在 bus 上注册观察型推送（不影响 Continue）。
pub fn install_ui_timeline(
    bus: &PluginHookBus,
    tx: tokio::sync::mpsc::UnboundedSender<UiHookEvent>,
) {
    const NAMES: &[&str] = &[
        ON_SESSION_START,
        PRE_LLM_CALL,
        PRE_API_REQUEST,
        POST_API_REQUEST,
        PRE_TOOL_CALL,
        POST_TOOL_CALL,
        POST_LLM_CALL,
        ON_SESSION_END,
        ON_SESSION_FINALIZE,
        ON_SESSION_RESET,
        SUBAGENT_STOP,
        PRE_GATEWAY_DISPATCH,
    ];
    for &name in NAMES {
        let tx = tx.clone();
        let hook_name = name.to_string();
        bus.register(name, move |payload: &HookPayload| {
            let detail = if !payload.detail.is_empty() {
                payload.detail.clone()
            } else if let Some(t) = &payload.tool_name {
                format!(
                    "{t} {}",
                    payload
                        .tool_args
                        .as_ref()
                        .map(|v| v.to_string())
                        .unwrap_or_default()
                )
            } else if let Some(n) = payload.system_prompt_chars {
                format!("system_prompt_chars={n}")
            } else if let Some(n) = payload.assistant_chars {
                format!("assistant_chars={n}")
            } else if let Some(t) = payload.turn {
                format!("turn={t}")
            } else {
                String::new()
            };
            let _ = tx.send(UiHookEvent {
                name: hook_name.clone(),
                detail,
                outcome: "continue".into(),
            });
            HookOutcome::Continue
        });
    }
}

/// 测试用：记录触发过的钩子名。
pub fn install_recording(
    bus: &PluginHookBus,
    log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) {
    const NAMES: &[&str] = &[
        ON_SESSION_START,
        PRE_LLM_CALL,
        PRE_API_REQUEST,
        POST_API_REQUEST,
        PRE_TOOL_CALL,
        POST_TOOL_CALL,
        POST_LLM_CALL,
        ON_SESSION_END,
        ON_SESSION_FINALIZE,
        ON_SESSION_RESET,
        SUBAGENT_STOP,
        PRE_GATEWAY_DISPATCH,
    ];
    for &name in NAMES {
        let log = std::sync::Arc::clone(&log);
        let hook_name = name.to_string();
        bus.register(name, move |payload: &HookPayload| {
            let label = match (&payload.tool_name, payload.assistant_chars, payload.system_prompt_chars) {
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
