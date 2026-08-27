//! 工具注册元数据。

use std::fmt;

use serde::{Deserialize, Serialize};

/// Codex 对齐的工具名——支持普通名和命名空间。
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ToolName {
    Plain(String),
    Namespaced { namespace: String, name: String },
}

impl ToolName {
    pub fn plain(name: impl Into<String>) -> Self {
        Self::Plain(name.into())
    }

    pub fn namespaced(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self::Namespaced {
            namespace: namespace.into(),
            name: name.into(),
        }
    }

    pub fn wire_name(&self) -> String {
        match self {
            Self::Plain(name) => name.clone(),
            Self::Namespaced { namespace, name } => format!("{namespace}.{name}"),
        }
    }

    pub fn parse(wire: &str) -> Self {
        match wire.split_once('.') {
            Some((ns, name)) if !ns.is_empty() && !name.is_empty() => Self::Namespaced {
                namespace: ns.to_string(),
                name: name.to_string(),
            },
            _ => Self::Plain(wire.to_string()),
        }
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.wire_name())
    }
}

/// Codex 对齐的工具规格分类。
#[derive(Debug, Clone)]
pub enum ToolSpec {
    /// 标准 JSON function 工具（绝大多数）。
    Function { schema: serde_json::Value },
    /// 命名空间工具组（如 `clock.curr_time`、`clock.sleep`）。
    Namespace { tools: Vec<NamespacedToolDef> },
    /// 自定义语法工具（如 apply_patch 的 LARK grammar）。
    Freeform {
        grammar: String,
        description: String,
    },
}

/// 命名空间下的单个子工具定义。
#[derive(Debug, Clone)]
pub struct NamespacedToolDef {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}

impl Default for ToolSpec {
    fn default() -> Self {
        Self::Function {
            schema: serde_json::json!({ "type": "object", "properties": {} }),
        }
    }
}

/// Codex 对齐的工具执行审批需求声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecApprovalRequirement {
    /// 不需要审批（只读工具、纯计算等）。
    #[default]
    Skip,
    /// 需要用户/Guardian 审批后才能执行。
    NeedsApproval,
    /// 绝对禁止执行（硬线拦截）。
    Forbidden,
}

/// Tool-level preference for process sandbox selection, aligned with Codex.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SandboxablePreference {
    /// Let the orchestrator select a sandbox from the active permission profile.
    Auto,
    /// Require a platform sandbox even when the ambient profile is unrestricted.
    Require,
    /// This tool does not launch a process through the command sandbox.
    #[default]
    Forbid,
}

/// Tool visibility level for LLM context injection (aligned with Codex ToolExposure).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExposure {
    /// Always injected into LLM tools parameter.
    #[default]
    Direct,
    /// Not injected; discoverable via tool_search, callable once discovered.
    Deferred,
    /// Completely hidden from LLM and tool_search. Internal use only.
    Hidden,
    /// Deferred but only surfaced to the model, not the user.
    DeferredModelOnly,
    /// Direct but only surfaced to the model, not the user.
    DirectModelOnly,
    /// Only available in code mode.
    CodeModeOnly,
}

impl ToolExposure {
    pub fn is_direct(&self) -> bool {
        matches!(self, Self::Direct | Self::DirectModelOnly)
    }
    pub fn is_deferred(&self) -> bool {
        matches!(self, Self::Deferred | Self::DeferredModelOnly)
    }
    pub fn is_hidden(&self) -> bool {
        matches!(self, Self::Hidden | Self::CodeModeOnly)
    }
}

/// Format descriptor for freeform (non-JSON) tools like apply_patch.
/// The model outputs raw text matching the grammar instead of JSON arguments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreeformToolFormat {
    /// Format type, e.g. "grammar".
    pub r#type: String,
    /// Grammar syntax, e.g. "lark".
    pub syntax: String,
    /// Grammar definition string.
    pub definition: String,
}

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
    pub sandbox_preference: SandboxablePreference,
    pub mcp_approval: Option<McpToolApproval>,
    pub approval_requirement: ExecApprovalRequirement,
    /// Tool visibility level: Direct (default), Deferred (discoverable via tool_search),
    /// or Hidden (internal only). Replaces the former `deferred: bool` flag.
    pub exposure: ToolExposure,
    /// Tool namespace for grouping (e.g., "shell", "media", "system", "mcp").
    /// Default empty string means the default namespace.
    pub namespace: String,
    /// Optional freeform format descriptor for non-JSON tools (e.g. apply_patch grammar).
    pub freeform_format: Option<FreeformToolFormat>,
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
            sandbox_preference: SandboxablePreference::Forbid,
            mcp_approval: None,
            approval_requirement: ExecApprovalRequirement::Skip,
            exposure: ToolExposure::Direct,
            namespace: String::new(),
            freeform_format: None,
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

    pub fn sandboxable(mut self) -> Self {
        self.sandbox_preference = SandboxablePreference::Auto;
        self
    }

    /// 标记为延迟加载工具——不注入 LLM tools 列表，需经 `tool_search` 发现后可用。
    pub fn deferred(mut self) -> Self {
        self.exposure = ToolExposure::Deferred;
        self
    }

    /// 标记为隐藏工具——对 LLM 和 tool_search 均不可见，仅供内部使用。
    pub fn hidden(mut self) -> Self {
        self.exposure = ToolExposure::Hidden;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_plain_roundtrips() {
        let name = ToolName::plain("terminal");
        assert_eq!(name.wire_name(), "terminal");
        assert_eq!(ToolName::parse("terminal"), name);
    }

    #[test]
    fn tool_name_namespaced_roundtrips() {
        let name = ToolName::namespaced("clock", "curr_time");
        assert_eq!(name.wire_name(), "clock.curr_time");
        assert_eq!(ToolName::parse("clock.curr_time"), name);
        assert_eq!(format!("{name}"), "clock.curr_time");
    }

    #[test]
    fn tool_name_parse_edge_cases() {
        assert_eq!(ToolName::parse(""), ToolName::Plain(String::new()));
        assert_eq!(ToolName::parse(".name"), ToolName::Plain(".name".into()));
        assert_eq!(ToolName::parse("ns."), ToolName::Plain("ns.".into()));
    }

    #[test]
    fn tool_spec_default_is_function() {
        assert!(matches!(ToolSpec::default(), ToolSpec::Function { .. }));
    }

    #[test]
    fn exec_approval_requirement_default_is_skip() {
        assert_eq!(
            ExecApprovalRequirement::default(),
            ExecApprovalRequirement::Skip
        );
    }

    #[test]
    fn tool_entry_defaults_include_skip_approval() {
        let entry = ToolEntry::lifecycle_defaults();
        assert_eq!(entry.approval_requirement, ExecApprovalRequirement::Skip);
    }

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
