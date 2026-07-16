//! 将 OpenAI function 格式的 tools 转为各厂商请求体字段

use serde_json::{json, Value};

/// 从 `schemas_for_api()`（OpenAI tools 数组）提取 Anthropic `tools`
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

/// 从 `schemas_for_api()`（OpenAI tools 数组）提取 Gemini native `tools`
///
/// 输出格式：`[{"function_declarations": [{"name", "description", "parameters"}]}]`
/// 单个 tool 对象包含所有声明；空输入返回空数组。
pub fn openai_tools_to_gemini_native(tools: &[Value]) -> Vec<Value> {
    let decls: Vec<Value> = tools
        .iter()
        .filter_map(|t| {
            let f = t.get("function")?;
            let name = f.get("name")?.clone();
            let mut decl = serde_json::Map::new();
            decl.insert("name".into(), name);
            if let Some(desc) = f.get("description") {
                decl.insert("description".into(), desc.clone());
            }
            if let Some(params) = f.get("parameters") {
                decl.insert("parameters".into(), params.clone());
            }
            Some(Value::Object(decl))
        })
        .collect();
    if decls.is_empty() {
        return Vec::new();
    }
    vec![json!({ "function_declarations": decls })]
}
