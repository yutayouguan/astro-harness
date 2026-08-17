//! 工具注册元数据。

use serde::{Deserialize, Serialize};

/// MCP 工具审批模式，对齐 Codex `approval_mode` 配置。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpToolApprovalMode {
    /// 由宿主根据工具风险与当前审批设置决定。
    #[default]
    Auto,
    /// 每次调用都要求用户确认。
    Prompt,
    /// 只对未明确标记为只读的工具要求用户确认。
    Writes,
    /// MCP 策略层直接放行；仍受沙箱和其他权限检查约束。
    Approve,
}

impl McpToolApprovalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Prompt => "prompt",
            Self::Writes => "writes",
            Self::Approve => "approve",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "prompt" => Ok(Self::Prompt),
            "writes" => Ok(Self::Writes),
            "approve" => Ok(Self::Approve),
            other => Err(format!(
                "unsupported MCP tool approval mode {other:?}; expected auto, prompt, writes, or approve"
            )),
        }
    }
}

/// MCP Server 声明的工具 annotations。
///
/// 这些字段只是不可信提示，不能用来扩大沙箱或基础权限。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

/// 注册到统一工具表的 MCP 审批上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolApproval {
    pub server_id: String,
    pub native_name: String,
    pub mode: McpToolApprovalMode,
    pub annotations: McpToolAnnotations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpToolApprovalRoute {
    Allow,
    GlobalReview,
    UserReview,
}

impl McpToolApproval {
    /// 计算 MCP 策略层路由；基础沙箱和权限检查始终在此之后继续执行。
    pub fn route(&self) -> McpToolApprovalRoute {
        match self.mode {
            McpToolApprovalMode::Approve => McpToolApprovalRoute::Allow,
            McpToolApprovalMode::Prompt => McpToolApprovalRoute::UserReview,
            McpToolApprovalMode::Writes => {
                if self.annotations.read_only_hint == Some(true) {
                    McpToolApprovalRoute::Allow
                } else {
                    McpToolApprovalRoute::UserReview
                }
            }
            McpToolApprovalMode::Auto => {
                let explicitly_low_risk = self.annotations.read_only_hint == Some(true)
                    && self.annotations.destructive_hint == Some(false)
                    && self.annotations.open_world_hint == Some(false);
                if explicitly_low_risk {
                    McpToolApprovalRoute::Allow
                } else {
                    McpToolApprovalRoute::GlobalReview
                }
            }
        }
    }

    pub fn needs_review(&self) -> bool {
        self.route() != McpToolApprovalRoute::Allow
    }
}

/// 单个可注册工具的完整元数据条目。
pub struct ToolEntry {
    pub name: String,
    pub toolset: String,
    pub description: String,
    pub schema: serde_json::Value,
    pub check_fn: Option<Box<dyn Fn() -> bool + Send + Sync>>,
    pub icon: &'static str,
    pub needs_confirmation: bool,
    pub stop_after_tool_call: bool,
    pub exclusive_access: bool,
    pub mcp_approval: Option<McpToolApproval>,
}

impl ToolEntry {
    pub fn lifecycle_defaults() -> Self {
        Self {
            name: String::new(),
            toolset: String::new(),
            description: String::new(),
            schema: serde_json::json!({ "type": "object", "properties": {} }),
            check_fn: None,
            icon: "wrench",
            needs_confirmation: false,
            stop_after_tool_call: false,
            exclusive_access: false,
            mcp_approval: None,
        }
    }

    pub fn with_confirmation(mut self) -> Self {
        self.needs_confirmation = true;
        self
    }

    pub fn stop_after(mut self) -> Self {
        self.stop_after_tool_call = true;
        self
    }

    pub fn exclusive(mut self) -> Self {
        self.exclusive_access = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval(mode: McpToolApprovalMode, annotations: McpToolAnnotations) -> McpToolApproval {
        McpToolApproval {
            server_id: "server".into(),
            native_name: "tool".into(),
            mode,
            annotations,
        }
    }

    #[test]
    fn mcp_approval_routes_are_fail_safe() {
        assert_eq!(
            approval(McpToolApprovalMode::Approve, Default::default()).route(),
            McpToolApprovalRoute::Allow
        );
        assert_eq!(
            approval(McpToolApprovalMode::Prompt, Default::default()).route(),
            McpToolApprovalRoute::UserReview
        );
        assert_eq!(
            approval(McpToolApprovalMode::Writes, Default::default()).route(),
            McpToolApprovalRoute::UserReview
        );
        assert_eq!(
            approval(
                McpToolApprovalMode::Writes,
                McpToolAnnotations {
                    read_only_hint: Some(true),
                    ..Default::default()
                }
            )
            .route(),
            McpToolApprovalRoute::Allow
        );
        assert_eq!(
            approval(McpToolApprovalMode::Auto, Default::default()).route(),
            McpToolApprovalRoute::GlobalReview
        );
        assert_eq!(
            approval(
                McpToolApprovalMode::Auto,
                McpToolAnnotations {
                    read_only_hint: Some(true),
                    destructive_hint: Some(false),
                    open_world_hint: Some(false),
                    ..Default::default()
                }
            )
            .route(),
            McpToolApprovalRoute::Allow
        );
    }
}
