//! 进程内插件注册上下文（对齐 `register(ctx)` / `ctx.register_hook`）。

use crate::gateway::GatewayHookRegistry;
use crate::outcome::{HookOutcome, HookPayload};
use crate::plugin::PluginHookBus;

/// 启动期注册工具：Plugin 钩子 + Gateway handler。
pub struct PluginContext<'a> {
    pub plugin: &'a PluginHookBus,
    pub gateway: &'a GatewayHookRegistry,
}

impl<'a> PluginContext<'a> {
    pub fn new(plugin: &'a PluginHookBus, gateway: &'a GatewayHookRegistry) -> Self {
        Self { plugin, gateway }
    }

    /// `ctx.register_hook("PostToolUse", callback)`
    pub fn register_hook<F>(&self, name: &str, f: F)
    where
        F: Fn(&HookPayload) -> HookOutcome + Send + Sync + 'static,
    {
        self.plugin.register(name, f);
    }

    /// 绑定目录清单名到进程内 Gateway handler。
    pub fn register_gateway_handler<F>(&self, name: &str, f: F)
    where
        F: Fn(&str, &HookPayload) + Send + Sync + 'static,
    {
        self.gateway.register_handler(name, f);
    }
}
