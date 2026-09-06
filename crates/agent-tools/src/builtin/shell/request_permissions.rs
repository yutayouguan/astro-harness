//! 权限请求：向用户请求额外的文件系统或网络权限。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// 网络权限请求。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct NetworkPermission {
    /// true 表示请求网络访问；false 或省略表示不请求。
    #[serde(default)]
    pub enabled: bool,
}

/// 文件系统权限请求。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct FileSystemPermission {
    /// 授予读取权限的绝对路径。
    #[serde(default)]
    pub read: Vec<String>,
    /// 授予写入权限的绝对路径。
    #[serde(default)]
    pub write: Vec<String>,
}

/// 模型请求的权限配置。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PermissionRequest {
    /// 网络访问请求。
    #[serde(default)]
    pub network: Option<NetworkPermission>,
    /// 文件系统访问请求。
    #[serde(default)]
    pub file_system: Option<FileSystemPermission>,
}

/// `request_permissions` 工具的参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RequestPermissionsArgs {
    /// 可选的简短说明，解释为何需要额外权限。
    #[serde(default)]
    pub reason: Option<String>,
    /// 请求的权限。
    pub permissions: PermissionRequest,
}

/// 向注册表注册 `request_permissions` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "request_permissions".to_string(),
        toolset: "exec_command".to_string(),
        description:
            "Request additional filesystem or network permissions from the user. Granted permissions apply to later commands in the current session."
                .to_string(),
        schema: schema_for_args::<RequestPermissionsArgs>(),
        check_fn: None,
        icon: "shield",
        ..ToolEntry::lifecycle_defaults().deferred()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["request_permissions"],
    async_ctx: dispatch,
    args: RequestPermissionsArgs,
}

/// 处理权限请求（stub 实现）。
pub async fn dispatch(
    _ctx: &ToolContext<'_>,
    args: &RequestPermissionsArgs,
) -> anyhow::Result<String> {
    let reason = args.reason.as_deref().unwrap_or("No reason provided.");
    let mut parts = vec![format!("Permission request noted. Reason: {reason}")];

    if let Some(ref net) = args.permissions.network {
        parts.push(format!(
            "Network access: {}",
            if net.enabled {
                "requested"
            } else {
                "not requested"
            }
        ));
    }
    if let Some(ref fs) = args.permissions.file_system {
        if !fs.read.is_empty() {
            parts.push(format!("Read paths: {:?}", fs.read));
        }
        if !fs.write.is_empty() {
            parts.push(format!("Write paths: {:?}", fs.write));
        }
    }

    Ok(parts.join("\n"))
}
