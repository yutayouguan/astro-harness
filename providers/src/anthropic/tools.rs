//! 将 OpenAI function 格式的 tools 转为 Anthropic `tools` 请求体字段。

use serde_json::{json, Value};

pub fn openai_tools_to_anthropic(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            let f = t.get("function")?;
            Some(json!({
                "name": f.get("name")?,
                "description": f.get("description").cloned().unwrap_or(json!("")),
                "input_schema": f.get("parameters").cloned().unwrap_or(json!({
                    "type": "object",
                    "properties": {}
                })),
            }))
        })
        .collect()
}
