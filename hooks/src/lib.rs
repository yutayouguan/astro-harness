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
