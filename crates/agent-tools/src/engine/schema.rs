//! 工具参数 JSON Schema：手写 / schemars 派生 / 宏
//!
//! `schema_for_args` 会 **sanitize** 成厂商友好的简化 schema：
//! 去掉 `$schema`/`title`/`$defs`，inline `$ref`，`type:[T,null]` → `T`。

use schemars::JsonSchema;
use serde_json::{json, Map, Value};

/// 从实现了 `JsonSchema` 的参数类型生成 JSON Schema（object），并做厂商兼容清理。
pub fn schema_for_args<T: JsonSchema>() -> Value {
    let root = schemars::schema_for!(T);
    let value = serde_json::to_value(root).unwrap_or_else(|_| {
        json!({
            "type": "object",
            "properties": {}
        })
    });
    sanitize_tool_schema(value)
}

/// 将 schemars / 复杂 JSON Schema 压成 OpenAI / Anthropic / Google 更易接受的形态。
pub fn sanitize_tool_schema(mut value: Value) -> Value {
    let defs = extract_defs(&mut value);
    let mut resolved = resolve_refs(value, &defs, 0);
    strip_meta(&mut resolved);
    normalize_null_unions(&mut resolved);
    ensure_object_type(&mut resolved);
    resolved
}

/// 从 schema 根节点提取 `$defs` / `definitions` 映射并移除原字段，供后续 inline `$ref`。
fn extract_defs(value: &mut Value) -> Map<String, Value> {
    let mut out = Map::new();
    if let Some(obj) = value.as_object_mut() {
        for key in ["$defs", "definitions"] {
            if let Some(Value::Object(map)) = obj.remove(key) {
                for (k, v) in map {
                    out.insert(k, v);
                }
            }
        }
    }
    out
}

/// 递归解析并内联 `$ref` 引用；深度超过 12 层时停止展开以防循环引用。
fn resolve_refs(value: Value, defs: &Map<String, Value>, depth: usize) -> Value {
    if depth > 12 {
        return value;
    }
    match value {
        Value::Object(obj) => {
            if let Some(Value::String(r)) = obj.get("$ref").cloned() {
                if let Some(name) = ref_name(&r) {
                    if let Some(def) = defs.get(name) {
                        return resolve_refs(def.clone(), defs, depth + 1);
                    }
                }
            }
            let mut next = Map::new();
            for (k, v) in obj {
                if k == "$ref" {
                    continue;
                }
                next.insert(k, resolve_refs(v, defs, depth + 1));
            }
            Value::Object(next)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(|v| resolve_refs(v, defs, depth + 1))
                .collect(),
        ),
        other => other,
    }
}

/// 从 `$ref` 字符串（如 `#/$defs/Foo`）提取定义名（最后一段路径）。
fn ref_name(r: &str) -> Option<&str> {
    r.rsplit('/').next().filter(|s| !s.is_empty())
}

/// 递归移除 `$schema`、`title`、`examples`、`$id` 等元数据字段；保留 `description`。
///
/// `properties` 下的键是实际参数名，不是 schema 元数据。例如工具参数可以合法地
/// 叫作 `title`；不能把 `properties.title` 删除后仍在 `required` 中保留它。
fn strip_meta(value: &mut Value) {
    strip_meta_inner(value, false);
}

fn strip_meta_inner(value: &mut Value, is_properties_map: bool) {
    match value {
        Value::Object(obj) => {
            if !is_properties_map {
                obj.remove("$schema");
                obj.remove("title");
                // 保留 description；去掉 examples 等噪声
                obj.remove("examples");
                obj.remove("$id");
            }
            for (key, child) in obj.iter_mut() {
                if is_properties_map {
                    // 子节点是各参数的 schema，不是 properties map 本身
                    strip_meta_inner(child, false);
                } else {
                    strip_meta_inner(child, key == "properties");
                }
            }
        }
        Value::Array(arr) => {
            for v in arr {
                strip_meta_inner(v, false);
            }
        }
        _ => {}
    }
}

/// `type: [T, "null"]` / `anyOf: [{type:T}, {type:null}]` → 纯 `T`（optional 由 required 表达）
fn normalize_null_unions(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            if let Some(simplified) = simplify_type_field(obj) {
                *obj = simplified;
            }
            if let Some(simplified) = simplify_any_of_null(obj) {
                *obj = simplified;
            }
            for v in obj.values_mut() {
                normalize_null_unions(v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                normalize_null_unions(v);
            }
        }
        _ => {}
    }
}

/// 尝试将 `type: [T, "null"]` 简化为单一 `type: T`。
fn simplify_type_field(obj: &Map<String, Value>) -> Option<Map<String, Value>> {
    let Value::Array(types) = obj.get("type")? else {
        return None;
    };
    let non_null: Vec<&Value> = types
        .iter()
        .filter(|t| t.as_str() != Some("null"))
        .collect();
    if non_null.len() != 1 {
        return None;
    }
    let mut next = obj.clone();
    next.insert("type".into(), non_null[0].clone());
    Some(next)
}

/// 尝试将 `anyOf`/`oneOf` 中含 `null` 的联合类型简化为单一非 null 分支。
fn simplify_any_of_null(obj: &Map<String, Value>) -> Option<Map<String, Value>> {
    let key = if obj.contains_key("anyOf") {
        "anyOf"
    } else if obj.contains_key("oneOf") {
        "oneOf"
    } else {
        return None;
    };
    let Value::Array(opts) = obj.get(key)? else {
        return None;
    };
    let mut non_null = Vec::new();
    let mut saw_null = false;
    for opt in opts {
        if opt.get("type").and_then(|t| t.as_str()) == Some("null") {
            saw_null = true;
            continue;
        }
        non_null.push(opt.clone());
    }
    if !saw_null || non_null.len() != 1 {
        return None;
    }
    let mut next = match non_null.pop()? {
        Value::Object(m) => m,
        other => {
            let mut m = Map::new();
            m.insert("type".into(), other);
            m
        }
    };
    // 保留外层 description（若有）
    if let Some(desc) = obj.get("description") {
        next.entry("description".to_string())
            .or_insert(desc.clone());
    }
    Some(next)
}

/// 若存在 `properties` 但缺少 `type`，补全为 `"object"`。
fn ensure_object_type(value: &mut Value) {
    if let Some(obj) = value.as_object_mut() {
        if obj.get("type").is_none() && obj.get("properties").is_some() {
            obj.insert("type".into(), Value::String("object".into()));
        }
    }
}

/// 递归检查 schema 是否仍含厂商不友好结构（测试用）
pub fn schema_has_vendor_hazards(value: &Value) -> Option<&'static str> {
    match value {
        Value::Object(obj) => {
            if obj.contains_key("$ref") {
                return Some("$ref");
            }
            if obj.contains_key("$defs") || obj.contains_key("definitions") {
                return Some("$defs");
            }
            if obj.contains_key("$schema") {
                return Some("$schema");
            }
            if let Some(Value::Array(_)) = obj.get("type") {
                return Some("type-array");
            }
            for v in obj.values() {
                if let Some(h) = schema_has_vendor_hazards(v) {
                    return Some(h);
                }
            }
            None
        }
        Value::Array(arr) => {
            for v in arr {
                if let Some(h) = schema_has_vendor_hazards(v) {
                    return Some(h);
                }
            }
            None
        }
        _ => None,
    }
}

/// 手写 JSON Schema 的便捷宏：语义同 `serde_json::json!`，但自动经 [`sanitize_tool_schema`] 清理。
///
/// 适用于字段较少、无需 `schemars` 派生的工具参数定义。
#[macro_export]
macro_rules! tool_schema {
    ($($json:tt)+) => {{
        $crate::schema::sanitize_tool_schema(serde_json::json!($($json)+))
    }};
}

/// 用 `schemars` 派生的 Args 类型快速注册工具到 [`ToolRegistry`]。
///
/// 自动生成 `schema_for_args::<Args>()` 并构造 [`ToolEntry`]；`check_fn` 固定为 `None`。
///
/// ```ignore
/// register_tool_schemars!(
///     registry,
///     name = "ask_user",
///     toolset = "ask_user",
///     description = "Ask the user a clarifying question",
///     icon = "circle-help",
///     args = AskUserArgs,
/// );
/// ```
#[macro_export]
macro_rules! register_tool_schemars {
    (
        $registry:expr,
        name = $name:expr,
        toolset = $toolset:expr,
        description = $description:expr,
        icon = $icon:expr,
        args = $Args:ty $(,)?
    ) => {{
        $registry.register($crate::registry::ToolEntry {
            name: ($name).to_string(),
            toolset: ($toolset).to_string(),
            description: ($description).to_string(),
            schema: $crate::schema::schema_for_args::<$Args>(),
            check_fn: None,
            icon: $icon,
            ..$crate::registry::ToolEntry::lifecycle_defaults()
        });
    }};
}

/// 定义工具参数结构体并自动派生 `JsonSchema`（Rig 风格精简宏）。
///
/// 展开后生成带 `Deserialize`、`Serialize`、`JsonSchema` 的 `pub struct`；
/// 需配合 `register` 函数或 `register_tool_schemars!` 完成注册。
#[macro_export]
macro_rules! define_tool_args {
    (
        $(#[$meta:meta])*
        pub struct $ArgsName:ident {
            $(
                $(#[$fmeta:meta])*
                pub $field:ident : $ty:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
        pub struct $ArgsName {
            $(
                $(#[$fmeta])*
                pub $field : $ty,
            )*
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    #[derive(Deserialize, Serialize, JsonSchema)]
    struct DemoArgs {
        path: String,
        #[serde(default)]
        content: Option<String>,
    }

    #[derive(Deserialize, Serialize, JsonSchema)]
    struct NestedArgs {
        name: String,
        #[serde(default)]
        profile: Option<NestedProfile>,
    }

    #[derive(Deserialize, Serialize, JsonSchema)]
    struct NestedProfile {
        style: Option<String>,
    }

    #[test]
    fn schemars_produces_object_schema() {
        let s = schema_for_args::<DemoArgs>();
        assert_eq!(s.get("type").and_then(|t| t.as_str()), Some("object"));
        assert!(s.get("properties").and_then(|p| p.get("path")).is_some());
        let content = s.pointer("/properties/content").expect("content");
        assert_eq!(content.get("type").and_then(|t| t.as_str()), Some("string"));
        assert!(schema_has_vendor_hazards(&s).is_none(), "{s}");
    }

    #[test]
    fn nested_option_inlines_without_ref() {
        let s = schema_for_args::<NestedArgs>();
        assert!(schema_has_vendor_hazards(&s).is_none(), "{s}");
        assert!(s.get("$defs").is_none());
        let profile = s.pointer("/properties/profile").expect("profile");
        assert!(
            profile.get("properties").is_some() || profile.get("type").is_some(),
            "{profile}"
        );
    }

    #[test]
    fn sanitize_strips_schema_title() {
        let raw = json!({
            "$schema": "https://example.com",
            "title": "X",
            "type": "object",
            "properties": {
                "q": { "type": ["string", "null"] }
            }
        });
        let s = sanitize_tool_schema(raw);
        assert!(s.get("$schema").is_none());
        assert!(s.get("title").is_none());
        assert_eq!(
            s.pointer("/properties/q/type").and_then(|t| t.as_str()),
            Some("string")
        );
    }

    #[test]
    fn sanitize_preserves_property_named_title() {
        let raw = json!({
            "title": "ConfirmArgs",
            "type": "object",
            "properties": {
                "title": {
                    "title": "Title",
                    "type": "string"
                },
                "examples": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            },
            "required": ["title", "examples"]
        });
        let s = sanitize_tool_schema(raw);
        assert!(s.get("title").is_none());
        assert!(s.pointer("/properties/title").is_some(), "{s}");
        assert!(s.pointer("/properties/title/title").is_none(), "{s}");
        assert!(s.pointer("/properties/examples").is_some(), "{s}");
        assert_eq!(s["required"], json!(["title", "examples"]));
    }
}
