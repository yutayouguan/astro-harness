//! OpenAI / Anthropic 原生工具格式转换。
//!
//! 支持：
//! - OpenAI `function` 类型 → Anthropic `{name, description, input_schema}`
//! - 非函数工具（computer_use / bash / text_editor / web_search / code_execution）→ 透传

use serde_json::{json, Value};

pub fn openai_tools_to_anthropic(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            if let Some(f) = t.get("function") {
                let mut tool = json!({
                    "name": f.get("name")?,
                    "description": f.get("description").cloned().unwrap_or(json!("")),
                    "input_schema": f.get("parameters").cloned().unwrap_or(json!({
                        "type": "object",
                        "properties": {}
                    })),
                });
                if let Some(cc) = t.get("cache_control") {
                    tool["cache_control"] = cc.clone();
                }
                Some(tool)
            } else if t.get("type").is_some() {
                Some(t.clone())
            } else {
                None
            }
        })
        .collect()
}
