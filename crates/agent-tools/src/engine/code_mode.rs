//! Code Mode 工具声明渲染。
//!
//! `ALL_TOOLS` 仍只暴露 `name` 与 `description`，其中 description 会附带由
//! JSON Schema 转换得到的 TypeScript 调用声明。Direct 工具可复用该声明
//! 预置到 `exec` 描述，Deferred 工具也不需要另一套 Schema 查询 API。

use std::collections::HashSet;

use serde_json::Value;
use types::FreeformToolFormat;

const MAX_SCHEMA_DEPTH: usize = 12;

/// 为 Code Mode 工具生成包含 TypeScript 调用签名的描述。
pub fn render_tool_description(
    name: &str,
    description: &str,
    parameters: &Value,
    freeform_format: Option<&FreeformToolFormat>,
) -> String {
    let input = if freeform_format.is_some() {
        "input: string".to_string()
    } else {
        format!("args: {}", render_schema(parameters, 0))
    };
    let declaration = format!("declare const tools: {{ {name}({input}): Promise<unknown>; }};");
    let description = description.trim();
    if description.is_empty() {
        format!("exec tool declaration:\n```ts\n{declaration}\n```")
    } else {
        format!("{description}\n\nexec tool declaration:\n```ts\n{declaration}\n```")
    }
}

fn render_schema(schema: &Value, depth: usize) -> String {
    if depth >= MAX_SCHEMA_DEPTH {
        return "unknown".to_string();
    }

    if let Some(value) = schema.get("const") {
        return render_literal(value);
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return render_union(values.iter().map(render_literal));
    }
    for keyword in ["anyOf", "oneOf"] {
        if let Some(variants) = schema.get(keyword).and_then(Value::as_array) {
            return render_union(
                variants
                    .iter()
                    .map(|variant| render_schema(variant, depth + 1)),
            );
        }
    }
    if let Some(variants) = schema.get("allOf").and_then(Value::as_array) {
        let rendered = variants
            .iter()
            .map(|variant| render_schema(variant, depth + 1))
            .collect::<Vec<_>>();
        return if rendered.is_empty() {
            "unknown".to_string()
        } else {
            rendered.join(" & ")
        };
    }

    match schema.get("type") {
        Some(Value::Array(types)) => render_union(
            types
                .iter()
                .filter_map(Value::as_str)
                .map(|type_name| render_schema_type(type_name, schema, depth)),
        ),
        Some(Value::String(type_name)) => render_schema_type(type_name, schema, depth),
        _ if schema.get("properties").is_some() => render_object(schema, depth),
        _ => "unknown".to_string(),
    }
}

fn render_schema_type(type_name: &str, schema: &Value, depth: usize) -> String {
    match type_name {
        "object" => render_object(schema, depth),
        "array" => {
            let item = schema
                .get("items")
                .map(|items| render_schema(items, depth + 1))
                .unwrap_or_else(|| "unknown".to_string());
            format!("Array<{item}>")
        }
        "integer" | "number" => "number".to_string(),
        "string" => "string".to_string(),
        "boolean" => "boolean".to_string(),
        "null" => "null".to_string(),
        _ => "unknown".to_string(),
    }
}

fn render_object(schema: &Value, depth: usize) -> String {
    let properties = schema.get("properties").and_then(Value::as_object);
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<HashSet<_>>();

    let mut fields = Vec::new();
    if let Some(properties) = properties {
        for (name, property) in properties {
            if let Some(description) = property.get("description").and_then(Value::as_str) {
                fields.extend(render_comment(description));
            }
            let rendered_name = render_property_name(name);
            let optional = if required.contains(name.as_str()) {
                ""
            } else {
                "?"
            };
            fields.push(format!(
                "{rendered_name}{optional}: {};",
                render_schema(property, depth + 1)
            ));
        }
    }

    match schema.get("additionalProperties") {
        Some(Value::Object(additional)) => fields.push(format!(
            "[key: string]: {};",
            render_schema(&Value::Object(additional.clone()), depth + 1)
        )),
        Some(Value::Bool(true)) => fields.push("[key: string]: unknown;".to_string()),
        _ => {}
    }

    if fields.is_empty() {
        return "{}".to_string();
    }
    let body = fields
        .into_iter()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{{\n{body}\n}}")
}

fn render_comment(description: &str) -> Vec<String> {
    description
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| format!("// {line}"))
        .collect()
}

fn render_property_name(name: &str) -> String {
    let mut chars = name.chars();
    let valid_start = chars
        .next()
        .is_some_and(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphabetic());
    let valid_rest = chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric());
    if valid_start && valid_rest {
        name.to_string()
    } else {
        serde_json::to_string(name).unwrap_or_else(|_| "\"\"".to_string())
    }
}

fn render_literal(value: &Value) -> String {
    match value {
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null => {
            serde_json::to_string(value).unwrap_or_else(|_| "unknown".to_string())
        }
        Value::Array(_) | Value::Object(_) => "unknown".to_string(),
    }
}

fn render_union(values: impl IntoIterator<Item = String>) -> String {
    let mut seen = HashSet::new();
    let values = values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect::<Vec<_>>();
    if values.is_empty() {
        "unknown".to_string()
    } else {
        values.join(" | ")
    }
}

#[cfg(test)]
#[path = "code_mode_tests.rs"]
mod tests;
