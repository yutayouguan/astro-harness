//! Code Mode 的模型可见控制工具。
//!
//! JavaScript 由 `agent-core` 内嵌的 QuickJS 执行；这里仅声明 `exec` / `wait`
//! 的 Responses 工具协议，并对绕过 AgentLoop 的直接分发保持关闭。

use crate::registry::{ToolEntry, ToolRegistry};
use types::FreeformToolFormat;

pub const EXEC_TOOL_NAME: &str = "exec";
pub const WAIT_TOOL_NAME: &str = "wait";

const EXEC_LARK_GRAMMAR: &str = r#"start: pragma_source | plain_source
pragma_source: PRAGMA_LINE NEWLINE SOURCE
plain_source: SOURCE

PRAGMA_LINE: /[ \t]*\/\/ @exec:[^\r\n]*/
NEWLINE: /\r?\n/
SOURCE: /[\s\S]+/"#;

const EXEC_DESCRIPTION: &str = r#"Run JavaScript code to orchestrate/compose tool calls
- Evaluates the provided JavaScript code in an embedded QuickJS runtime.
- All nested tools are available on the global `tools` object, for example `await tools.exec_command(...)`.
- Runs raw JavaScript with no Node.js globals, filesystem, network, or console access.
- Accepts raw JavaScript source text, not JSON, quoted strings, or markdown code fences.
- `text(value)` appends text output. `store(key, value)` and `load(key)` share serializable values across cells.
- `notify(value)` emits output immediately. `yield_control()` yields while the script remains alive.
- `ALL_TOOLS` lists enabled nested tools; each `description` includes that tool's full declaration."#;

const WAIT_DESCRIPTION: &str = r#"Waits on a yielded `exec` cell and returns new output or completion.
- Use `wait` only after `exec` returns `Script running with cell ID ...`.
- `cell_id` identifies the running exec cell.
- `yield_time_ms` controls how long to wait for output. Defaults to 10000 ms.
- `max_tokens` limits how much new output is returned. Defaults to 10000 tokens.
- `terminate: true` stops the running cell."#;

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: EXEC_TOOL_NAME.into(),
        toolset: "code_mode".into(),
        description: EXEC_DESCRIPTION.into(),
        icon: "code",
        exclusive_access: true,
        exposure: types::ToolExposure::DirectModelOnly,
        freeform_format: Some(FreeformToolFormat {
            r#type: "grammar".into(),
            syntax: "lark".into(),
            definition: EXEC_LARK_GRAMMAR.into(),
        }),
        ..ToolEntry::lifecycle_defaults()
    });
    registry.register(ToolEntry {
        name: WAIT_TOOL_NAME.into(),
        toolset: "code_mode".into(),
        description: WAIT_DESCRIPTION.into(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "cell_id": {"type": "string", "description": "Identifier of the running exec cell."},
                "yield_time_ms": {"type": "integer", "minimum": 0},
                "max_tokens": {"type": "integer", "minimum": 1},
                "terminate": {"type": "boolean"}
            },
            "required": ["cell_id"],
            "additionalProperties": false
        }),
        icon: "clock",
        exclusive_access: true,
        exposure: types::ToolExposure::DirectModelOnly,
        ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["exec", "wait"],
    async_named: fail_closed,
}

async fn fail_closed(
    _ctx: &mut crate::context::ToolContext<'_>,
    name: &str,
    _args: &serde_json::Value,
) -> anyhow::Result<String> {
    anyhow::bail!("{name} requires the AgentLoop Code Mode runtime")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_quickjs_control_contracts() {
        let mut registry = ToolRegistry::new();
        register(&mut registry);
        let specs = registry
            .schemas_for_api_with_mode(types::ToolMode::CodeMode)
            .unwrap();
        assert_eq!(
            specs.iter().find(|spec| spec["name"] == "exec").unwrap()["type"],
            "custom"
        );
        assert_eq!(
            specs.iter().find(|spec| spec["name"] == "wait").unwrap()["type"],
            "function"
        );
    }
}
