//! 工具执行器 trait。inventory 内置工具由注册宏直接生成该 trait 的实现。

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
/// 手写运行时直接实现此 trait；inventory 内置工具由
/// [`crate::submit_builtin_tool!`] 生成对应的原生实现。
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

#[doc(hidden)]
pub fn tool_spec_from_entry(entry: &types::ToolEntry) -> ToolSpec {
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
    fn echo_tool_implements_executor_trait() {
        let tool = EchoTool;
        assert_eq!(tool.tool_name(), ToolName::plain("echo"));
        assert_eq!(tool.toolset(), "test");
        assert_eq!(tool.approval_requirement(), ExecApprovalRequirement::Skip);
    }
}
