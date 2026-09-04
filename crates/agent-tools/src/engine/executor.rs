//! 工具执行器 trait——新工具的推荐实现方式。
//!
//! 现有 inventory 内置工具通过 [`LegacyToolAdapter`] 自动适配。

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::context::ToolContext;
use crate::registry::DynToolHandler;
use types::{ExecApprovalRequirement, NamespacedToolDef, ToolName, ToolOutput, ToolSpec};

/// 新工具的标准 handler future 类型。
pub type ToolExecutorFuture<'a> = Pin<Box<dyn Future<Output = anyhow::Result<ToolOutput>> + 'a>>;

/// 工具执行器 trait。
///
/// 新工具应实现此 trait 而非使用 `BuiltinToolHandler` 函数指针。
/// 现有工具通过 [`LegacyToolAdapter`] 自动桥接。
pub trait ToolExecutor: Send + Sync + 'static {
    /// 工具的规范名（支持命名空间）。
    fn tool_name(&self) -> ToolName;

    /// 工具规格（Function / Namespace / Freeform）。
    fn spec(&self) -> ToolSpec;

    /// 工具描述文本（用于 LLM schema）。
    fn description(&self) -> &str;

    /// 工具所属 toolset（控制启用/禁用）。
    fn toolset(&self) -> &str;

    /// 审批需求声明。
    fn approval_requirement(&self) -> ExecApprovalRequirement {
        ExecApprovalRequirement::Skip
    }

    /// 工具图标（UI 用）。
    fn icon(&self) -> &'static str {
        "wrench"
    }

    /// 执行工具调用。
    fn handle<'a>(
        &'a self,
        ctx: &'a mut ToolContext<'_>,
        args: &'a serde_json::Value,
    ) -> ToolExecutorFuture<'a>;
}

/// Agent Core 可持有的类型擦除工具运行时。
///
/// 当前与 [`ToolExecutor`] 共享合同；独立命名保留了 Codex
/// `CoreToolRuntime / ToolExecutor` 的边界，后续可在不改变 Registry
/// 存储类型的情况下增加 readiness、并行性等 Core 能力。
pub trait CoreToolRuntime: ToolExecutor {}

impl<T> CoreToolRuntime for T where T: ToolExecutor {}

fn tool_spec_from_entry(entry: &types::ToolEntry) -> ToolSpec {
    if let Some(format) = &entry.freeform_format {
        return ToolSpec::Freeform {
            grammar: format.definition.clone(),
            description: entry.description.clone(),
        };
    }
    if entry.namespace.is_empty() {
        return ToolSpec::Function {
            schema: entry.schema.clone(),
        };
    }
    let child_name = match entry.tool_name() {
        ToolName::Plain(name) | ToolName::Namespaced { name, .. } => name,
    };
    ToolSpec::Namespace {
        tools: vec![NamespacedToolDef {
            name: child_name,
            description: entry.description.clone(),
            schema: entry.schema.clone(),
        }],
    }
}

/// 将现有 `BuiltinToolHandler` 函数指针适配为 `ToolExecutor`。
///
/// `register_all()` 为每个 `BuiltinToolRegistrar` 自动创建此适配器，
/// 现有 28 个工具无需任何修改。
pub struct LegacyToolAdapter {
    name: String,
    entry: types::ToolEntry,
    handler: super::registry::BuiltinToolHandler,
}

impl LegacyToolAdapter {
    pub fn new(
        name: String,
        entry: types::ToolEntry,
        handler: super::registry::BuiltinToolHandler,
    ) -> Self {
        Self {
            name,
            entry,
            handler,
        }
    }
}

impl ToolExecutor for LegacyToolAdapter {
    fn tool_name(&self) -> ToolName {
        self.entry.tool_name()
    }

    fn spec(&self) -> ToolSpec {
        tool_spec_from_entry(&self.entry)
    }

    fn description(&self) -> &str {
        &self.entry.description
    }

    fn toolset(&self) -> &str {
        &self.entry.toolset
    }

    fn approval_requirement(&self) -> ExecApprovalRequirement {
        self.entry.approval_requirement
    }

    fn icon(&self) -> &'static str {
        self.entry.icon
    }

    fn handle<'a>(
        &'a self,
        ctx: &'a mut ToolContext<'_>,
        args: &'a serde_json::Value,
    ) -> ToolExecutorFuture<'a> {
        (self.handler)(ctx, &self.name, args)
    }
}

/// 运行时注册工具（MCP / Extension）的类型擦除适配器。
pub struct DynamicToolAdapter {
    entry: types::ToolEntry,
    handler: DynToolHandler,
}

impl DynamicToolAdapter {
    pub fn new(entry: types::ToolEntry, handler: DynToolHandler) -> Self {
        Self { entry, handler }
    }

    pub fn registered_name(&self) -> &str {
        &self.entry.name
    }
}

impl ToolExecutor for DynamicToolAdapter {
    fn tool_name(&self) -> ToolName {
        self.entry.tool_name()
    }

    fn spec(&self) -> ToolSpec {
        tool_spec_from_entry(&self.entry)
    }

    fn description(&self) -> &str {
        &self.entry.description
    }

    fn toolset(&self) -> &str {
        &self.entry.toolset
    }

    fn approval_requirement(&self) -> ExecApprovalRequirement {
        self.entry.approval_requirement
    }

    fn icon(&self) -> &'static str {
        self.entry.icon
    }

    fn handle<'a>(
        &'a self,
        _ctx: &'a mut ToolContext<'_>,
        args: &'a serde_json::Value,
    ) -> ToolExecutorFuture<'a> {
        let handler = Arc::clone(&self.handler);
        let name = self.entry.name.clone();
        let args = args.clone();
        Box::pin(async move { handler(&name, &args).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoTool;

    impl ToolExecutor for EchoTool {
        fn tool_name(&self) -> ToolName {
            ToolName::plain("echo")
        }

        fn spec(&self) -> ToolSpec {
            ToolSpec::Function {
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" }
                    }
                }),
            }
        }

        fn description(&self) -> &str {
            "Echo back the input text"
        }

        fn toolset(&self) -> &str {
            "test"
        }

        fn handle<'a>(
            &'a self,
            _ctx: &'a mut ToolContext<'_>,
            args: &'a serde_json::Value,
        ) -> ToolExecutorFuture<'a> {
            Box::pin(async move {
                let text = args
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(empty)");
                Ok(ToolOutput::Text(text.to_string()))
            })
        }
    }

    #[test]
    fn legacy_adapter_preserves_entry_metadata() {
        let entry = types::ToolEntry {
            name: "test_tool".into(),
            toolset: "test".into(),
            description: "A test tool".into(),
            icon: "beaker",
            approval_requirement: ExecApprovalRequirement::NeedsApproval,
            ..types::ToolEntry::lifecycle_defaults()
        };

        fn noop_handler<'a, 'b>(
            _ctx: &'a mut ToolContext<'b>,
            _name: &'a str,
            _args: &'a serde_json::Value,
        ) -> ToolExecutorFuture<'a> {
            Box::pin(async { Ok(ToolOutput::Text("ok".into())) })
        }

        let adapter = LegacyToolAdapter::new("test_tool".into(), entry, noop_handler);

        assert_eq!(adapter.tool_name(), ToolName::plain("test_tool"));
        assert_eq!(adapter.description(), "A test tool");
        assert_eq!(adapter.toolset(), "test");
        assert_eq!(adapter.icon(), "beaker");
        assert_eq!(
            adapter.approval_requirement(),
            ExecApprovalRequirement::NeedsApproval
        );
        assert!(matches!(adapter.spec(), ToolSpec::Function { .. }));
    }

    #[test]
    fn echo_tool_implements_executor_trait() {
        let tool = EchoTool;
        assert_eq!(tool.tool_name(), ToolName::plain("echo"));
        assert_eq!(tool.toolset(), "test");
        assert_eq!(tool.approval_requirement(), ExecApprovalRequirement::Skip);
    }
}
