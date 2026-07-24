//! `switch_mode`：请求切换 Agent ↔ Plan（流结束后由前端授权条确认）。
//!
//! 与 `ask_user`（同回合 HITL park）不同：本工具产出 `astro_mode_switch`，不走 HitlGate。

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::context::ToolContext;
use crate::schema::schema_for_args;

/// 允许切换的目标模式（不含 ask / multitask）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum ModeSwitchTarget {
    Plan,
    Agent,
}

impl ModeSwitchTarget {
    fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Agent => "agent",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ModeSwitchArgs {
    /// Target mode: only `plan` or `agent`.
    to: ModeSwitchTarget,
    /// Why the switch is needed (shown on the authorize bar).
    reason: String,
    /// Required when `to=agent`: plan summary injected into the next turn after approval.
    #[serde(default)]
    summary: Option<String>,
}

pub fn register(registry: &mut crate::registry::ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "switch_mode".to_string(),
        toolset: "switch_mode".to_string(),
        description: "Request switching chat interaction mode between Agent and Plan only \
(not ask/multitask). Use for Agent→Plan when a complex task needs a written plan first, \
or Plan→Agent when the plan is ready to execute. \
User confirms via a post-stream countdown authorize bar — do not assume the switch until approved. \
Always set `reason`; when `to=\"agent\"`, `summary` is required (confirmed plan text). \
Do not use ask_user(confirm) for mode changes; do not use this tool for clarifying questions or location."
            .to_string(),
        schema: schema_for_args::<ModeSwitchArgs>(),
        check_fn: None,
        icon: "arrow-left-right",
        stop_after_tool_call: true,
        ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["switch_mode"],
    sync_ctx: dispatch,
}

pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: ModeSwitchArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("switch_mode 参数无效: {e}"))?;
    let reason = parsed.reason.trim();
    if reason.is_empty() {
        anyhow::bail!("switch_mode.reason is required");
    }
    let summary = parsed
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if parsed.to == ModeSwitchTarget::Agent && summary.is_none() {
        anyhow::bail!("switch_mode to=\"agent\" requires non-empty summary (the plan)");
    }

    let payload = json!({
        "astro_mode_switch": true,
        "to": parsed.to.as_str(),
        "reason": reason,
        "summary": summary,
    });
    Ok(payload.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ImageGenTargets, ToolContext};
    use providers::registry::ProviderRegistry;
    use serde_json::json;
    use tempfile::TempDir;

    fn with_ctx(f: impl FnOnce(&ToolContext<'_>)) {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
        let providers = ProviderRegistry::new();
        let targets = ImageGenTargets::default();
        let ctx = ToolContext {
            memory: &mut memory,
            sessions: &sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: workspace,
            project_root: None,
            image_gen_targets: &targets,
            providers: &providers,
            session_id: "t".into(),
            turn_id: None,
            chat_api_key: String::new(),
            chat_base_url: String::new(),
            chat_provider: String::new(),
            chat_model: String::new(),
            chat_targets: vec![],
            execution: None,
            hook_bus: None,
        };
        f(&ctx);
    }

    #[test]
    fn agent_requires_summary() {
        with_ctx(|ctx| {
            let err = dispatch(ctx, &json!({ "to": "agent", "reason": "ready" }))
                .unwrap_err()
                .to_string();
            assert!(err.contains("summary"), "{err}");
        });
    }

    #[test]
    fn plan_ok_without_summary() {
        with_ctx(|ctx| {
            let raw = dispatch(ctx, &json!({ "to": "plan", "reason": "need a plan" })).unwrap();
            let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(v["to"], "plan");
            assert!(v["summary"].is_null());
        });
    }

    #[test]
    fn rejects_ask_target() {
        with_ctx(|ctx| {
            let err = dispatch(ctx, &json!({ "to": "ask", "reason": "x" })).unwrap_err();
            assert!(
                err.to_string().contains("参数无效"),
                "expected deserialize error, got {err}"
            );
        });
    }

    #[test]
    fn agent_with_summary_ok() {
        with_ctx(|ctx| {
            let raw = dispatch(
                ctx,
                &json!({
                    "to": "agent",
                    "reason": "plan ready",
                    "summary": "1. do A\n2. do B"
                }),
            )
            .unwrap();
            let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(v["to"], "agent");
            assert_eq!(v["summary"], "1. do A\n2. do B");
        });
    }
}
