//! 权限请求：向用户请求额外的文件系统或网络权限。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// Network permission request.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct NetworkPermission {
    /// True requests network access; false or omitted requests none.
    #[serde(default)]
    pub enabled: bool,
}

/// Filesystem permission request.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct FileSystemPermission {
    /// Absolute paths to grant read access.
    #[serde(default)]
    pub read: Vec<String>,
    /// Absolute paths to grant write access.
    #[serde(default)]
    pub write: Vec<String>,
}

/// Permission profile requested by the model.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PermissionRequest {
    /// Network access request.
    #[serde(default)]
    pub network: Option<NetworkPermission>,
    /// Filesystem access request.
    #[serde(default)]
    pub file_system: Option<FileSystemPermission>,
}

/// Arguments for the `request_permissions` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RequestPermissionsArgs {
    /// Optional short explanation for why additional permissions are needed.
    #[serde(default)]
    pub reason: Option<String>,
    /// The permissions being requested.
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
