//! Codex Code Mode 的模型可见入口。
//!
//! 真正的 V8 cell 生命周期位于 `agent-core::runtime::code_mode`，因为嵌套调用必须
//! 回到同一 Turn 的审批、沙箱与 hook 执行链。这里仅注册原生 Responses 工具规格，
//! 并为脱离 AgentLoop 的误调用提供 fail-closed handler。

use crate::registry::{ToolEntry, ToolRegistry};
use types::tool_entry::FreeformToolFormat;

pub const EXEC_TOOL_NAME: &str = "exec";
pub const WAIT_TOOL_NAME: &str = "wait";

const EXEC_LARK_GRAMMAR: &str = r#"start: pragma_source | plain_source
pragma_source: PRAGMA_LINE NEWLINE SOURCE
plain_source: SOURCE

PRAGMA_LINE: /[ \t]*\/\/ @exec:[^\r\n]*/
NEWLINE: /\r?\n/
SOURCE: /[\s\S]+/"#;

const EXEC_DESCRIPTION: &str = r#"Run JavaScript code to orchestrate/compose tool calls
- Evaluates the provided JavaScript code in a fresh V8 isolate as an async module.
- All nested tools are available on the global `tools` object, for example `await tools.exec_command(...)`.
- Nested tool methods take either a string or an object as their input argument.
- Runs raw JavaScript -- no Node globals, no file system, no network access, no console.
- Accepts raw JavaScript source text, not JSON, quoted strings, or markdown code fences.
- You may optionally start with `// @exec: {"yield_time_ms": 10000, "max_output_tokens": 1000}`.
- `text(value)` appends text output. `store(key, value)` and `load(key)` share serializable values across cells in this session.
- `notify(value)` immediately appends output. `yield_control()` yields accumulated output while the script remains alive.
- `ALL_TOOLS` lists enabled nested tools as `{ name, description }` entries.
- Pending timers do not keep a completed script alive; await work that must finish."#;

const WAIT_DESCRIPTION: &str = r#"Waits on a yielded `exec` cell and returns new output or completion.
- Use `wait` only after `exec` returns `Script running with cell ID ...`.
- `cell_id` identifies the running exec cell.
- `yield_time_ms` controls how long to wait for more output before yielding again. Defaults to 10000 ms.
- `max_tokens` limits how much new output this wait call returns. Defaults to 10000 tokens.
- `terminate: true` stops the running cell; false or omitted waits for output."#;

pub fn register(registry: &mut ToolRegistry) {
    let node_available = || {
        std::process::Command::new("node")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    };
    registry.register(ToolEntry {
        name: EXEC_TOOL_NAME.into(),
        toolset: "code_mode".into(),
        description: EXEC_DESCRIPTION.into(),
        check_fn: Some(Box::new(node_available)),
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
        check_fn: Some(Box::new(node_available)),
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
    fn registers_native_exec_and_wait_contracts() {
        let mut registry = ToolRegistry::new();
        register(&mut registry);
        let specs = registry.schemas_for_api();
        let exec = specs.iter().find(|spec| spec["name"] == "exec").unwrap();
        assert_eq!(exec["type"], "custom");
        assert_eq!(exec["format"]["syntax"], "lark");
        let wait = specs.iter().find(|spec| spec["name"] == "wait").unwrap();
        assert_eq!(wait["type"], "function");
        assert_eq!(
            wait["parameters"]["required"],
            serde_json::json!(["cell_id"])
        );
    }
}
