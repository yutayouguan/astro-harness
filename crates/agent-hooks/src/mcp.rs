//! MCP hook execution boundary.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value};

/// One MCP tool invocation requested by a configured hook handler.
#[derive(Debug, Clone, PartialEq)]
pub struct HookMcpCall {
    pub server: String,
    pub tool: String,
    pub input: Map<String, Value>,
    pub timeout: Duration,
}

/// Executes already-connected MCP tools without coupling `agent-hooks` to the MCP crate.
pub trait HookMcpExecutor: Send + Sync {
    /// The returned text is parsed with the same event-specific rules as command stdout.
    fn execute(
        &self,
        call: HookMcpCall,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + '_>>;
}

#[derive(Default)]
pub(crate) struct UnavailableHookMcpExecutor;

impl HookMcpExecutor for UnavailableHookMcpExecutor {
    fn execute(
        &self,
        call: HookMcpCall,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + '_>> {
        Box::pin(async move {
            anyhow::bail!(
                "MCP hook executor is unavailable for {}:{}",
                call.server,
                call.tool
            )
        })
    }
}

pub(crate) fn unavailable_executor() -> Arc<dyn HookMcpExecutor> {
    Arc::new(UnavailableHookMcpExecutor)
}

/// Expand `${field.nested}` placeholders against the serialized hook input.
pub(crate) fn expand_argument_template(
    template: &Map<String, Value>,
    hook_input: &Value,
) -> anyhow::Result<Map<String, Value>> {
    template
        .iter()
        .map(|(key, value)| Ok((key.clone(), resolve_value(value, hook_input)?)))
        .collect()
}

fn resolve_value(value: &Value, hook_input: &Value) -> anyhow::Result<Value> {
    match value {
        Value::Object(value) => Ok(Value::Object(expand_argument_template(value, hook_input)?)),
        Value::Array(values) => values
            .iter()
            .map(|value| resolve_value(value, hook_input))
            .collect::<anyhow::Result<Vec<_>>>()
            .map(Value::Array),
        Value::String(value) => resolve_string(value, hook_input),
        _ => Ok(value.clone()),
    }
}

fn resolve_string(value: &str, hook_input: &Value) -> anyhow::Result<Value> {
    let pattern = regex::Regex::new(r"\$\{([^{}]+)\}")?;
    let captures = pattern.captures_iter(value).collect::<Vec<_>>();
    if captures.is_empty() {
        return Ok(Value::String(value.to_string()));
    }
    if let [capture] = captures.as_slice() {
        if let Some(placeholder) = capture.get(0) {
            if placeholder.start() == 0 && placeholder.end() == value.len() {
                return resolve_path(hook_input, &capture[1]).cloned();
            }
        }
    }

    let mut resolved = String::new();
    let mut previous_end = 0;
    for capture in captures {
        let Some(placeholder) = capture.get(0) else {
            continue;
        };
        resolved.push_str(&value[previous_end..placeholder.start()]);
        match resolve_path(hook_input, &capture[1])? {
            Value::String(value) => resolved.push_str(value),
            value => resolved.push_str(&serde_json::to_string(value)?),
        }
        previous_end = placeholder.end();
    }
    resolved.push_str(&value[previous_end..]);
    Ok(Value::String(resolved))
}

fn resolve_path<'a>(hook_input: &'a Value, path: &str) -> anyhow::Result<&'a Value> {
    path.split('.').try_fold(hook_input, |value, field| {
        value
            .get(field)
            .ok_or_else(|| anyhow::anyhow!("hook input placeholder `${{{path}}}` was not found"))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn expands_nested_mcp_arguments_and_preserves_json_types() {
        let template = serde_json::from_value(json!({
            "query": "${tool_input.query}",
            "label": "tool=${tool_name}",
            "fixed": true
        }))
        .unwrap();
        let input = json!({"tool_name":"search", "tool_input":{"query":{"limit":3}}});

        assert_eq!(
            expand_argument_template(&template, &input).unwrap(),
            serde_json::from_value(json!({
                "query": {"limit":3},
                "label": "tool=search",
                "fixed": true
            }))
            .unwrap()
        );
    }

    #[test]
    fn missing_mcp_argument_placeholder_fails_closed() {
        let template = serde_json::from_value(json!({"query":"${missing}"})).unwrap();
        let error = expand_argument_template(&template, &json!({})).unwrap_err();
        assert!(error.to_string().contains("`${missing}`"));
    }
}
