//! Astro 三套 Hook 体系：Plugin（Agent 生命周期）、Gateway（外壳事件）、Shell（配置命令）。
//!
//! 注册风格对齐：`ctx.register_hook("PostToolUse", callback)`。

pub mod config;
pub mod context;
pub mod event;
pub mod gateway;
pub mod names;
pub mod outcome;
pub mod plugin;
pub mod shell;
pub mod ui;

pub use config::{default_astro_root, load_config, load_config_or_default, AstroConfig};
pub use context::PluginContext;
pub use event::HookEvent;
pub use gateway::{DiscoveredHook, GatewayHookRegistry, HookManifest};
pub use names::*;
pub use outcome::{HookInput, HookOutcome, HookPayload};
pub use plugin::PluginHookBus;
pub use shell::{load_shell_runner, ShellHookRunner};
pub use ui::{
    install_recording, install_ui_timeline, UiHookEvent, UiTimelineGeneration, UiTimelineSlot,
};

use std::sync::Arc;

/// 进程级钩子运行时：三套体系的聚合句柄。
#[derive(Clone)]
pub struct HookRuntime {
    pub plugin: Arc<PluginHookBus>,
    pub gateway: Arc<GatewayHookRegistry>,
    pub shell: Arc<std::sync::Mutex<ShellHookRunner>>,
    /// 进程 plugin bus 上的 UI 时间线槽（每轮 chat 热替换 sender）。
    pub ui_slot: UiTimelineSlot,
}

impl Default for HookRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl HookRuntime {
    pub fn new() -> Self {
        Self::with_plugin_bus(Arc::new(PluginHookBus::new()))
    }

    pub fn with_plugin_bus(plugin: Arc<PluginHookBus>) -> Self {
        let ui_slot = UiTimelineSlot::new();
        ui_slot.install(&plugin);
        Self {
            plugin,
            gateway: Arc::new(GatewayHookRegistry::default()),
            shell: Arc::new(std::sync::Mutex::new(ShellHookRunner::default())),
            ui_slot,
        }
    }

    /// 从数据根加载 config + 发现 gateway 清单。
    pub fn bootstrap_from_root(root: &std::path::Path) -> anyhow::Result<Self> {
        let cfg = load_config(root)?;
        let rt = Self::new();
        rt.gateway.discover(root)?;
        rt.gateway.install_logging_fallbacks();
        if let Ok(mut sh) = rt.shell.lock() {
            *sh = ShellHookRunner::from_map(cfg.hooks);
        }
        Ok(rt)
    }

    pub fn plugin_context(&self) -> PluginContext<'_> {
        PluginContext::new(&self.plugin, &self.gateway)
    }

    /// 统一向 Plugin、Gateway、Shell 三套 transport 投递事件。
    pub fn dispatch(&self, name: &str, payload: &HookPayload) -> HookOutcome {
        let payload = payload.for_event(name);
        let out = self.plugin.fire(name, &payload);
        self.gateway.fire(name, &payload);
        if let Ok(sh) = self.shell.lock() {
            sh.fire_async(name, &payload);
        }
        out
    }

    /// 兼容包装：统一向三套 transport 投递事件。
    pub fn fire_plugin(&self, name: &str, payload: &HookPayload) -> HookOutcome {
        self.dispatch(name, payload)
    }

    /// 兼容包装：统一向三套 transport 投递事件。
    pub fn fire_gateway(&self, event: &str, payload: &HookPayload) {
        let _ = self.dispatch(event, payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn runtime_does_not_bridge_legacy_shell_keys_to_canonical_events() {
        let rt = HookRuntime::new();
        let seen = Arc::new(std::sync::Mutex::new(None));
        let capture = Arc::clone(&seen);
        rt.plugin.register(PRE_TOOL_USE, move |input| {
            *capture.lock().unwrap() = Some(input.hook_event_name.clone());
            HookOutcome::Continue
        });
        *rt.shell.lock().unwrap() = ShellHookRunner::new(std::collections::HashMap::from([(
            "pre_tool_call".to_string(),
            "true".to_string(),
        )]));

        let _ = rt.fire_plugin(PRE_TOOL_USE, &HookInput::default());

        assert_eq!(seen.lock().unwrap().as_deref(), Some(PRE_TOOL_USE));
        let shell_schedule = rt.shell.lock().unwrap().scheduled();
        assert!(shell_schedule.is_empty());
    }

    #[tokio::test]
    async fn dispatch_reaches_all_transports_with_canonical_name() {
        let dir = tempfile::tempdir().unwrap();
        let hook_dir = dir.path().join("hooks").join("audit");
        std::fs::create_dir_all(&hook_dir).unwrap();
        std::fs::write(
            hook_dir.join("HOOK.yaml"),
            "name: audit\nevents:\n  - PreToolUse\n",
        )
        .unwrap();

        let rt = HookRuntime::new();
        rt.gateway.discover(dir.path()).unwrap();
        let gateway_hits = Arc::new(AtomicUsize::new(0));
        let gateway_hit_count = Arc::clone(&gateway_hits);
        rt.gateway.register_handler("audit", move |event, input| {
            assert_eq!(event, PRE_TOOL_USE);
            assert_eq!(input.hook_event_name, PRE_TOOL_USE);
            gateway_hit_count.fetch_add(1, Ordering::SeqCst);
        });
        let plugin_hits = Arc::new(AtomicUsize::new(0));
        let plugin_hit_count = Arc::clone(&plugin_hits);
        rt.plugin.register(PRE_TOOL_USE, move |input| {
            assert_eq!(input.hook_event_name, PRE_TOOL_USE);
            plugin_hit_count.fetch_add(1, Ordering::SeqCst);
            HookOutcome::Block("blocked".into())
        });
        *rt.shell.lock().unwrap() = ShellHookRunner::new(std::collections::HashMap::from([(
            PRE_TOOL_USE.to_string(),
            "true".to_string(),
        )]));

        let out = rt.dispatch(PRE_TOOL_USE, &HookInput::default());

        assert!(matches!(out, HookOutcome::Block(ref reason) if reason == "blocked"));
        assert_eq!(plugin_hits.load(Ordering::SeqCst), 1);
        assert_eq!(gateway_hits.load(Ordering::SeqCst), 1);
        let shell_schedule = rt.shell.lock().unwrap().scheduled();
        assert_eq!(shell_schedule.len(), 1);
        assert_eq!(shell_schedule[0].0, PRE_TOOL_USE);
        assert_eq!(shell_schedule[0].1.hook_event_name, PRE_TOOL_USE);
    }

    #[tokio::test]
    async fn runtime_routes_gateway_and_shell_by_canonical_name() {
        let dir = tempfile::tempdir().unwrap();
        let hook_dir = dir.path().join("hooks").join("audit");
        std::fs::create_dir_all(&hook_dir).unwrap();
        std::fs::write(
            hook_dir.join("HOOK.yaml"),
            "name: audit\nevents:\n  - GatewayStartup\n",
        )
        .unwrap();
        let rt = HookRuntime::new();
        rt.gateway.discover(dir.path()).unwrap();
        let seen = Arc::new(std::sync::Mutex::new(None));
        let capture = Arc::clone(&seen);
        rt.gateway.register_handler("audit", move |event, input| {
            assert_eq!(input.hook_event_name, GATEWAY_STARTUP);
            *capture.lock().unwrap() = Some(event.to_owned());
        });
        *rt.shell.lock().unwrap() = ShellHookRunner::new(std::collections::HashMap::from([(
            GATEWAY_STARTUP.to_string(),
            "true".to_string(),
        )]));

        rt.fire_gateway(GATEWAY_STARTUP, &HookInput::default());

        assert_eq!(seen.lock().unwrap().as_deref(), Some(GATEWAY_STARTUP));
        assert!(rt.shell.lock().unwrap().has_event(GATEWAY_STARTUP));
    }

    #[test]
    fn fire_gateway_invokes_handler_and_shell_map() {
        let rt = HookRuntime::new();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        // 无清单时 fire 不会调 handler；先手工注入 discovered+handler 路径：
        // 用 register_handler + 直接 gateway.fire 已在 gateway 模块测过。
        // 这里验证 HookRuntime 聚合：plugin 旁路不误伤 + shell 不 panic。
        rt.gateway.register_handler("noop", move |_, _| {
            h.fetch_add(1, Ordering::SeqCst);
        });
        // 无 discovered 清单时 handler 不会被调；仅确保 fire_gateway 可调用。
        rt.fire_gateway(
            GATEWAY_STARTUP,
            &HookPayload {
                detail: "test".into(),
                ..Default::default()
            },
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        let _ = rt.fire_plugin(SESSION_RESET, &HookPayload::default());
    }

    #[test]
    fn pre_gateway_dispatch_skip_short_circuits() {
        let rt = HookRuntime::new();
        rt.plugin.register(PRE_GATEWAY_DISPATCH, |_| {
            HookOutcome::Skip("blocked-by-test".into())
        });
        let out = rt.fire_plugin(
            PRE_GATEWAY_DISPATCH,
            &HookPayload {
                prompt: Some("hello".into()),
                ..Default::default()
            },
        );
        assert!(matches!(out, HookOutcome::Skip(ref s) if s == "blocked-by-test"));
    }

    #[test]
    fn pre_gateway_dispatch_rewrite() {
        let rt = HookRuntime::new();
        rt.plugin.register(PRE_GATEWAY_DISPATCH, |_| {
            HookOutcome::Rewrite("rewritten".into())
        });
        let out = rt.fire_plugin(PRE_GATEWAY_DISPATCH, &HookPayload::default());
        assert!(matches!(out, HookOutcome::Rewrite(ref s) if s == "rewritten"));
    }
}
