//! 权限请求：向用户请求额外的文件系统或网络权限。
//!
//! **当前构建不授予任何权限。** 协议侧已经有 `EventMsg::RequestPermissions` 与
//! `Op::RequestPermissionsResponse`，但接通这条链路还缺三段：
//!   1. agent-core 的**生产者**：工具触发时发出 `EventMsg::RequestPermissions`
//!      （现在没有任何地方 emit 该事件）；
//!   2. Desktop 的授权卡片与响应：`thread_events` 已把该 control kind 透传并标记
//!      `waitingOnPermissions`，但前端没有卡片/回执处理；
//!   3. 会话级授权存储：`authorize_tool_call` 现在只有 per-attempt 的
//!      `workspace_write_grant`（沙箱重试路径），没有"本会话额外可写根目录"的概念，
//!      `build_command_sandbox_policy_with_roots` 也只读 permission profile。
//! 在补齐之前，本工具只回报"未生效"，避免模型误以为已获授权后继续用错误假设执行。

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
        "Permission request NOT applied: this build cannot grant extra filesystem or network access at runtime. Reason recorded: {reason}"
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

        assert!(text.contains("NOT applied"), "{text}");
        assert!(text.contains("need to write a report"), "{text}");
        assert!(text.contains("/etc"), "{text}");
        assert!(text.contains("/tmp/out"), "{text}");
        assert!(text.contains("permission preset"), "{text}");
    }

    #[test]
    fn empty_reason_is_reported_explicitly() {
        let mut args = args_with("   ");
        args.reason = None;

        let text = permission_request_message(&args);
        assert!(text.contains("Reason recorded: not provided"), "{text}");
    }
}
