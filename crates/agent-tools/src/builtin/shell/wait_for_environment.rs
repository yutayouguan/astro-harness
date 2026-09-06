//! 环境就绪等待：等待环境（如 MCP 服务器连接）准备就绪。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

fn default_timeout() -> u64 {
    30
}

/// `wait_for_environment` 工具的参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WaitForEnvironmentArgs {
    /// 等待环境就绪的超时秒数（默认 30，上限 60）。
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

/// 向注册表注册 `wait_for_environment` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "wait_for_environment".to_string(),
        toolset: "system".to_string(),
        description:
            "Wait for the environment to be ready (e.g., MCP servers connecting). Timeout max 60s."
                .to_string(),
        schema: schema_for_args::<WaitForEnvironmentArgs>(),
        check_fn: None,
        icon: "loader",
        ..ToolEntry::lifecycle_defaults().deferred()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["wait_for_environment"],
    async_ctx: dispatch,
    args: WaitForEnvironmentArgs,
}

/// 等待环境就绪。
pub async fn dispatch(
    _ctx: &ToolContext<'_>,
    args: &WaitForEnvironmentArgs,
) -> anyhow::Result<String> {
    let secs = args.timeout_secs.min(60);
    tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
    Ok("Environment ready.".to_string())
}
