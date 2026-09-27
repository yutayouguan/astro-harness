//! 权限请求：向用户请求额外的文件系统或网络权限。
//!
//! 授权由 **agent-core 的 preflight** 完成：`request_permissions` 被强制走串行
//! preflight（`tool_may_require_permission`），在那里 park 用户批准，批准后把
//! workspace-write + 额外可写根写进**会话级**授权（`Session::grant_permissions`），
//! 由 `sandbox_policy_for_call` 生效；该路径直接产出工具结果，不再进入本模块的
//! `dispatch`。
//!
//! 这里的 `dispatch` 是**兜底**（例如 code-mode 嵌套调用绕过了 preflight）：只回报
//! 请求内容，明确说明本工具自己不授权，避免模型误以为已经拿到权限。

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
            "Ask the user for additional filesystem or network permissions. Advisory only: this build never grants them at runtime — it just records the request, and permissions stay user-controlled in Settings."
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

/// 把权限请求渲染成模型可见的回报文本。
///
/// 纯函数，便于单测：内容必须明确"未生效"，并回列请求的路径，方便用户核对。
fn permission_request_message(args: &RequestPermissionsArgs) -> String {
    let reason = args
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("not provided");

    let mut parts = vec![format!(
        "Permission request NOT applied by this tool. The runtime asks the user when it routes this call through the approval flow; approved paths then apply to the current session only. Reason recorded: {reason}"
    )];

    if let Some(ref net) = args.permissions.network {
        parts.push(format!(
            "Requested network access: {}",
            if net.enabled { "yes" } else { "no" }
        ));
    }
    if let Some(ref fs) = args.permissions.file_system {
        if !fs.read.is_empty() {
            parts.push(format!("Requested read paths: {}", fs.read.join(", ")));
        }
        if !fs.write.is_empty() {
            parts.push(format!("Requested write paths: {}", fs.write.join(", ")));
        }
    }

    parts.push(
        "Continue inside the current sandbox: use paths within the workspace, or ask the user to change the permission preset in Settings.".to_string(),
    );
    parts.join("\n")
}

/// 处理权限请求。
///
/// 只回报请求内容与"未授予任何权限"。不要在这里假装成功——模型会据此继续执行，
/// 然后在真实沙箱里失败。
pub async fn dispatch(
    _ctx: &ToolContext<'_>,
    args: &RequestPermissionsArgs,
) -> anyhow::Result<String> {
    Ok(permission_request_message(args))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_with(reason: &str) -> RequestPermissionsArgs {
        RequestPermissionsArgs {
            reason: Some(reason.to_string()),
            permissions: PermissionRequest {
                network: Some(NetworkPermission { enabled: true }),
                file_system: Some(FileSystemPermission {
                    read: vec!["/etc".into()],
                    write: vec!["/tmp/out".into()],
                }),
            },
        }
    }

    #[test]
    fn permission_request_reports_that_nothing_was_granted() {
        let text = permission_request_message(&args_with("need to write a report"));

        assert!(text.contains("NOT applied by this tool"), "{text}");
        assert!(text.contains("need to write a report"), "{text}");
        assert!(text.contains("/etc"), "{text}");
        assert!(text.contains("/tmp/out"), "{text}");
        assert!(text.contains("current session only"), "{text}");
    }

    #[test]
    fn empty_reason_is_reported_explicitly() {
        let mut args = args_with("   ");
        args.reason = None;

        let text = permission_request_message(&args);
        assert!(text.contains("Reason recorded: not provided"), "{text}");
    }
}
