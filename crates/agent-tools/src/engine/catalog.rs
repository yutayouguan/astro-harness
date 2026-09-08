//! 工具目录：按 toolset 聚合，供 UI / Tauri 展示 schema 参数
//!
//! `icon` 字段存 Lucide 图标 id（kebab-case，如 `calendar-check`），
//! 由前端渲染成对应 Lucide 图标，不是 Unicode emoji。

use serde::Serialize;
use serde_json::Value;

use crate::registry::ToolRegistry;

/// 单个工具参数的 UI 展示信息，从 JSON Schema `properties` 提取。
#[derive(Debug, Clone, Serialize)]
pub struct ToolParamInfo {
    /// 参数名，与 schema `properties` 键名一致。
    pub name: String,
    /// JSON Schema 类型字符串，如 `"string"`、`"integer"`、`"any"`。
    #[serde(rename = "type")]
    pub type_name: String,
    /// 是否可选；不在 `required` 数组中则为 `true`。
    pub optional: bool,
    /// 参数说明，来自 schema `description` 字段。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// 单个可调用函数的 UI 展示信息；`icon` 为 Lucide 图标 id（如 `calendar-check`）。
#[derive(Debug, Clone, Serialize)]
pub struct ToolFunctionInfo {
    /// 模型可见调用名；namespace 工具为 `namespace.child`。
    pub name: String,
    /// Responses API 原生 namespace；默认 `functions` 域为 `None`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Rust Registry / handler 使用的内部名。
    #[serde(rename = "registeredName")]
    pub registered_name: String,
    /// 面向用户的工具说明。
    pub description: String,
    /// Lucide 图标 id（kebab-case），例如 `"calendar-check"` / `"folder-kanban"`。
    pub icon: String,
    /// 该函数的全部参数列表。
    pub params: Vec<ToolParamInfo>,
    /// `direct` / `deferred` / `hidden`，用于界面展示加载策略。
    pub exposure: String,
}

/// 按 toolset 聚合的工具目录项，供前端设置面板与工具列表展示。
#[derive(Debug, Clone, Serialize)]
pub struct ToolCatalogItem {
    /// 与前端开关对齐的 toolset id。
    pub id: String,
    /// 代表性的模型可见调用名。
    pub name: String,
    /// 代表性工具的 Responses API 原生 namespace。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// 代表性工具的内部注册名。
    #[serde(rename = "registeredName")]
    pub registered_name: String,
    /// 代表性工具的说明文本。
    pub description: String,
    /// Lucide 图标 id（kebab-case）。
    pub icon: String,
    /// 代表性工具的参数列表（与 `functions[0]` 可能不同，取决于 id 匹配）。
    pub params: Vec<ToolParamInfo>,
    /// 该 toolset 下全部工具名。
    pub tools: Vec<String>,
    /// 每个函数的完整说明与参数。
    pub functions: Vec<ToolFunctionInfo>,
    pub exposure: String,
}

fn exposure_name(exposure: types::ToolExposure) -> &'static str {
    match exposure {
        types::ToolExposure::Direct => "direct",
        types::ToolExposure::Deferred => "deferred",
        types::ToolExposure::Hidden => "hidden",
        types::ToolExposure::DeferredModelOnly => "deferred_model_only",
        types::ToolExposure::DirectModelOnly => "direct_model_only",
    }
}

/// 从 JSON Schema object 提取参数列表。
///
/// 读取 `properties` 与 `required`；结果按名称排序，必填项排在可选项之前。
/// 非 object 类型或缺少 `properties` 时返回空列表。
pub fn params_from_schema(schema: &Value) -> Vec<ToolParamInfo> {
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return Vec::new();
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut params: Vec<ToolParamInfo> = props
        .iter()
        .map(|(name, prop)| {
            let type_name = json_schema_type(prop);
            let description = prop
                .get("description")
                .and_then(|d| d.as_str())
                .map(str::to_string);
            ToolParamInfo {
                name: name.clone(),
                type_name,
                optional: !required.contains(&name.as_str()),
                description,
            }
        })
        .collect();
    params.sort_by(|a, b| a.name.cmp(&b.name));
    params.sort_by_key(|p| p.optional);
    params
}

/// 从单个 schema 属性节点推断类型字符串。
///
/// 支持 `type` 字符串/数组（忽略 `"null"`）、`anyOf`/`oneOf`、`enum`、`$ref`、
/// `array` 的 `items` 等常见形态；尽量给出可读的短标签供 UI 展示。
fn json_schema_type(prop: &Value) -> String {
    if let Some(enums) = prop.get("enum").and_then(|e| e.as_array()) {
        let vals: Vec<String> = enums
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                Value::Bool(b) => Some(b.to_string()),
                _ => None,
            })
            .take(6)
            .collect();
        if !vals.is_empty() {
            let more = if enums.len() > vals.len() { ",…" } else { "" };
            return format!("enum({})", vals.join("|") + more);
        }
    }

    let base = match prop.get("type") {
        Some(Value::String(t)) => t.clone(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|t| t.as_str())
            .find(|t| *t != "null")
            .unwrap_or("any")
            .to_string(),
        _ => {
            if prop.get("anyOf").is_some() || prop.get("oneOf").is_some() {
                "any".to_string()
            } else if prop.get("$ref").is_some() {
                "object".to_string()
            } else {
                "any".to_string()
            }
        }
    };

    if base == "array" {
        if let Some(items) = prop.get("items") {
            let inner = json_schema_type(items);
            if inner != "any" {
                return format!("array<{inner}>");
            }
        }
    }

    base
}

/// 将同一 toolset 下的多个 [`ToolEntry`] 聚合为一个 [`ToolCatalogItem`]。
///
/// 优先选取 `name == id` 的条目作为代表性工具（决定顶层 `params` 与 `description`）。
fn entries_to_item(id: String, mut entries: Vec<&crate::registry::ToolEntry>) -> ToolCatalogItem {
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let primary = entries
        .iter()
        .find(|e| e.name == id)
        .or_else(|| entries.first())
        .copied()
        .expect("entries non-empty");

    let functions: Vec<ToolFunctionInfo> = entries
        .iter()
        .map(|entry| {
            let tool_name = entry.tool_name();
            ToolFunctionInfo {
                name: tool_name.wire_name(),
                namespace: tool_name.namespace().map(str::to_string),
                registered_name: entry.name.clone(),
                description: entry.description.clone(),
                icon: entry.icon.to_string(),
                params: params_from_schema(&entry.schema),
                exposure: exposure_name(entry.exposure).to_string(),
            }
        })
        .collect();
    let tools: Vec<String> = functions.iter().map(|f| f.name.clone()).collect();
    let primary_name = primary.tool_name();

    ToolCatalogItem {
        id,
        name: primary_name.wire_name(),
        namespace: primary_name.namespace().map(str::to_string),
        registered_name: primary.name.clone(),
        description: primary.description.clone(),
        icon: primary.icon.to_string(),
        params: params_from_schema(&primary.schema),
        tools,
        functions,
        exposure: exposure_name(primary.exposure).to_string(),
    }
}

/// 按已知 toolset 顺序输出目录；顶层 `params` 取代表性工具的 schema。
///
/// 先输出 `home::KNOWN_TOOLSET_IDS` 中定义的顺序，其余未知 toolset 追加在后。
pub fn catalog_for_ui(registry: &ToolRegistry) -> Vec<ToolCatalogItem> {
    use std::collections::BTreeMap;

    let mut by_set: BTreeMap<String, Vec<&crate::registry::ToolEntry>> = BTreeMap::new();
    for entry in registry.all_tools() {
        if entry.exposure.is_model_only() {
            continue;
        }
        by_set.entry(entry.toolset.clone()).or_default().push(entry);
    }

    let mut out = Vec::new();
    for &id in home::KNOWN_TOOLSET_IDS {
        let Some(entries) = by_set.remove(id) else {
            continue;
        };
        out.push(entries_to_item(id.to_string(), entries));
    }

    for (id, entries) in by_set {
        out.push(entries_to_item(id, entries));
    }

    out
}

/// 构建并返回完整内置工具目录（无需外部注册表，内部临时注册全部内置工具）。
pub fn builtin_catalog() -> Vec<ToolCatalogItem> {
    let mut reg = ToolRegistry::new();
    crate::register_all(&mut reg);
    catalog_for_ui(&reg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_required_and_optional() {
        let schema = json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "q" },
                "limit": { "type": "integer" }
            },
            "required": ["query"]
        });
        let params = params_from_schema(&schema);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "query");
        assert!(!params[0].optional);
        assert!(params.iter().any(|p| p.name == "limit" && p.optional));
    }

    #[test]
    fn type_labels_enum_and_array_items() {
        let schema = json!({
            "type": "object",
            "properties": {
                "mode": { "enum": ["a", "b", "c"] },
                "tags": { "type": "array", "items": { "type": "string" } }
            }
        });
        let params = params_from_schema(&schema);
        let mode = params.iter().find(|p| p.name == "mode").unwrap();
        let tags = params.iter().find(|p| p.name == "tags").unwrap();
        assert!(mode.type_name.starts_with("enum("), "{}", mode.type_name);
        assert_eq!(tags.type_name, "array<string>");
    }

    #[test]
    fn extracts_enum_and_array_item_types() {
        let schema = json!({
            "type": "object",
            "properties": {
                "mode": { "enum": ["a", "b", "c"] },
                "tags": { "type": "array", "items": { "type": "string" } }
            }
        });
        let params = params_from_schema(&schema);
        let mode = params.iter().find(|p| p.name == "mode").unwrap();
        assert!(mode.type_name.starts_with("enum("), "{}", mode.type_name);
        let tags = params.iter().find(|p| p.name == "tags").unwrap();
        assert_eq!(tags.type_name, "array<string>");
    }

    #[test]
    fn builtin_catalog_covers_known_toolsets() {
        let cat = builtin_catalog();
        // workflow 是运行时目录，不由 builtin_catalog 静态注册。
        assert!(cat.len() + 1 >= home::KNOWN_TOOLSET_IDS.len());
        let web = cat
            .iter()
            .find(|c| c.id == "web_search")
            .expect("web_search");
        assert!(
            web.functions.iter().any(|f| f.name == "web_fetch"),
            "web_search toolset should include web_fetch"
        );
    }

    #[test]
    fn memory_has_per_function_params() {
        let cat = builtin_catalog();
        let memory = cat.iter().find(|c| c.id == "memory").expect("memory");
        assert_eq!(memory.functions.len(), 1);
        let mem = memory
            .functions
            .iter()
            .find(|f| f.name == "memory")
            .expect("memory");
        assert!(mem.params.iter().any(|p| p.name == "action"));
        assert!(mem.params.iter().any(|p| p.name == "target"));
        // Lucide kebab-case id，不是 Unicode emoji
        assert_eq!(mem.icon, "brain");
        assert!(mem
            .icon
            .chars()
            .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit()));
    }

    #[test]
    fn cron_namespace_has_five_tools() {
        let cat = builtin_catalog();
        let cron = cat.iter().find(|c| c.id == "cron").expect("cron");
        assert_eq!(cron.functions.len(), 5);
        let names: Vec<&str> = cron.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(cron.namespace.as_deref(), Some("cron"));
        assert!(names.contains(&"cron.add"));
        assert!(names.contains(&"cron.list"));
        assert!(names.contains(&"cron.remove"));
        assert_eq!(
            cron.functions
                .iter()
                .find(|function| function.name == "cron.add")
                .map(|function| function.registered_name.as_str()),
            Some("cron_add")
        );
    }

    #[test]
    fn catalog_serializes_frontend_namespace_contract_in_camel_case() {
        let catalog = builtin_catalog();
        let browser = catalog.iter().find(|item| item.id == "browser").unwrap();
        let json = serde_json::to_value(browser).unwrap();

        assert_eq!(json["namespace"], "astro_browser");
        assert_eq!(json["registeredName"], browser.registered_name);
        assert!(json.get("registered_name").is_none());
        let open = json["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|function| function["name"] == "astro_browser.open")
            .expect("astro_browser.open catalog function");
        assert_eq!(open["namespace"], "astro_browser");
        assert_eq!(open["registeredName"], "browser_open");
        assert_eq!(open["exposure"], "direct");
    }

    #[test]
    fn browser_and_media_catalogs_expose_model_names_and_internal_names() {
        let catalog = builtin_catalog();
        let browser = catalog.iter().find(|item| item.id == "browser").unwrap();
        assert_eq!(browser.namespace.as_deref(), Some("astro_browser"));
        assert!(browser.functions.iter().any(|function| {
            function.name == "astro_browser.open" && function.registered_name == "browser_open"
        }));

        let image = catalog.iter().find(|item| item.id == "image_gen").unwrap();
        assert_eq!(image.name, "media.image_gen");
        assert_eq!(image.namespace.as_deref(), Some("media"));
        assert_eq!(image.registered_name, "image_gen");
    }

    #[test]
    fn all_builtin_icons_are_lucide_kebab() {
        let mut reg = crate::registry::ToolRegistry::new();
        crate::register_all(&mut reg);
        for e in reg.all_tools() {
            assert!(
                !e.icon.is_empty()
                    && e.icon
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit()),
                "tool `{}` icon `{}` 应为 Lucide kebab-case id",
                e.name,
                e.icon
            );
        }
    }
}
