//! 插件安装请求：请求安装指定的 skill/plugin。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// Arguments for the `request_plugin_install` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RequestPluginInstallArgs {
    /// The skill/plugin identifier to install.
    pub skill_id: String,
    /// Optional reason for requesting the installation.
    #[serde(default)]
    pub reason: Option<String>,
}

/// 向注册表注册 `request_plugin_install` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "request_plugin_install".to_string(),
        toolset: "system".to_string(),
        description: "Request installation of a skill or plugin by its identifier.".to_string(),
        schema: schema_for_args::<RequestPluginInstallArgs>(),
        check_fn: None,
        icon: "download",
        ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["request_plugin_install"],
    async_ctx: dispatch,
    args: RequestPluginInstallArgs,
}

/// 请求安装插件并返回确认信息。
pub async fn dispatch(
    _ctx: &ToolContext<'_>,
    args: &RequestPluginInstallArgs,
) -> anyhow::Result<String> {
    Ok(format!(
        "Plugin install requested: {}. {}",
        args.skill_id,
        args.reason.as_deref().unwrap_or("No reason provided.")
    ))
}
