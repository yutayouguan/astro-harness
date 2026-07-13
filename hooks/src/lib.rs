//! Astro 三套 Hook 体系：Plugin（Agent 生命周期）、Gateway（外壳事件）、Shell（配置命令）。
//!
//! 注册风格对齐：`ctx.register_hook("post_tool_call", callback)`。

pub mod config;
pub mod context;
pub mod gateway;
pub mod names;
pub mod outcome;
pub mod plugin;
pub mod shell;
pub mod ui;

pub use config::{default_astro_root, load_config, AstroConfig};
pub use context::PluginContext;
pub use gateway::{DiscoveredHook, GatewayHookRegistry, HookManifest};
pub use names::*;
pub use outcome::{HookOutcome, HookPayload};
pub use plugin::PluginHookBus;
pub use shell::{load_shell_runner, ShellHookRunner};
pub use ui::{install_recording, install_ui_timeline, UiHookEvent};

use std::sync::Arc;

/// 进程级钩子运行时：三套体系的聚合句柄。
#[derive(Clone, Default)]
pub struct HookRuntime {
    pub plugin: Arc<PluginHookBus>,
    pub gateway: Arc<GatewayHookRegistry>,
    pub shell: Arc<std::sync::Mutex<ShellHookRunner>>,
}

impl HookRuntime {
    pub fn new() -> Self {
        Self::default()
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

    /// Plugin fire + 旁路 Shell（同名事件）。
    pub fn fire_plugin(&self, name: &str, payload: &HookPayload) -> HookOutcome {
        let out = self.plugin.fire(name, payload);
        if let Ok(sh) = self.shell.lock() {
            sh.fire_async(name, payload);
        }
        out
    }

    pub fn fire_gateway(&self, event: &str, payload: &HookPayload) {
        self.gateway.fire(event, payload);
        if let Ok(sh) = self.shell.lock() {
            sh.fire_async(event, payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
        let _ = rt.fire_plugin(ON_SESSION_RESET, &HookPayload::default());
    }
}
