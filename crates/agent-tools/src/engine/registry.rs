//! 工具注册表：集中管理内置与 MCP 工具的元数据、schema 与启用状态。
//!
//! `ToolRegistry` 是 Agent 与 LLM API 之间的桥梁，持有所有可调用工具的
//! [`ToolEntry`]，并按当前 Agent 的 `tools_enabled`（或全局
//! `~/.astro/tools-enabled.json`）与运行时 `check_fn` 过滤出当前会话实际可用的
//! 工具列表，供 `schemas_for_api` 下发给模型。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::context::ToolContext;

pub use types::tool_entry::ToolEntry;

/// 内置工具统一异步 handler：可包 sync/async、`&mut ToolContext`、按 name 路由。
///
/// 不要求 `Send`：`ToolContext`（含 `SessionStore`/`RefCell`）本身非 `Send`，
/// handler future 会捕获 `&mut ToolContext`。
pub type BuiltinToolHandler =
    for<'a, 'b> fn(
        &'a mut ToolContext<'b>,
        &'a str,
        &'a serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<types::ToolOutput>> + 'a>>;

/// 内置工具自注册钩子：各工具模块通过 `inventory::submit!` / [`crate::submit_builtin_tool!`] 报名。
///
/// 同时携带元数据 `register`、可分发名称列表与统一 [`BuiltinToolHandler`]。
pub struct BuiltinToolRegistrar {
    /// 向注册表写入本模块工具条目。
    pub register: fn(&mut ToolRegistry),
    /// 本模块可分发的工具名（应与 metadata 注册名对齐）。
    pub names: &'static [&'static str],
    /// 统一执行入口（按 `names` 中的 name 查表后调用）。
    pub handler: BuiltinToolHandler,
}

inventory::collect!(BuiltinToolRegistrar);

/// 工具注册表：以工具名为键的全局索引。
///
/// 同时维护 toolset 级别的启用映射；MCP 工具（`mcp__` 前缀）的开关在注册阶段
/// 已过滤，不走 `tools-enabled.json`。
/// 动态工具 handler（MCP 工具等运行时注册的异步调用闭包）。
///
/// 与 `BuiltinToolHandler` 不同，这不需要 `ToolContext` — MCP 工具通过
/// 捕获的 `Arc<McpHub>` 自行完成调用。
pub type DynToolHandler = Arc<
    dyn Fn(
            &str,
            &serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<types::ToolOutput>> + Send>>
        + Send
        + Sync,
>;

pub struct ToolRegistry {
    /// 已注册的全部工具条目。
    tools: HashMap<String, ToolEntry>,
    /// 运行时动态注册的 handler（MCP 工具等）— 按工具名查找。
    dynamic_handlers: HashMap<String, DynToolHandler>,
    /// 与当前 Agent / 全局 `tools-enabled` 对齐的 toolset 开关；缺失键视为启用。
    enabled: HashMap<String, bool>,
    /// Skill 加载后 additive 放宽的 toolset（即使 enabled 映射为 false 也允许）。
    skill_override_enabled: std::collections::HashSet<String>,
}

fn namespace_child_name(entry: &ToolEntry) -> String {
    entry
        .name
        .strip_prefix(&format!("{}_", entry.namespace))
        .or_else(|| entry.name.strip_prefix(&format!("{}.", entry.namespace)))
        .unwrap_or(&entry.name)
        .to_string()
}

fn entry_api_spec(entry: &ToolEntry, defer_loading: bool) -> serde_json::Value {
    if entry.name == "tool_search" {
        return serde_json::json!({
            "type": "tool_search",
            "execution": "client",
            "description": entry.description,
            "parameters": crate::schema::sanitize_tool_schema(entry.schema.clone()),
        });
    }
    if let Some(format) = &entry.freeform_format {
        return serde_json::json!({
            "type": "custom",
            "name": entry.name,
            "description": entry.description,
            "defer_loading": defer_loading.then_some(true),
            "format": format,
        });
    }
    serde_json::json!({
        "type": "function",
        "name": entry.name,
        "description": entry.description,
        "strict": false,
        "defer_loading": defer_loading.then_some(true),
        "parameters": crate::schema::sanitize_tool_schema(entry.schema.clone()),
    })
}

fn api_specs<'a>(entries: impl IntoIterator<Item = &'a ToolEntry>) -> Vec<serde_json::Value> {
    let mut plain = Vec::new();
    let mut namespaces = std::collections::BTreeMap::<String, Vec<serde_json::Value>>::new();
    for entry in entries {
        let defer_loading = entry.exposure.is_deferred();
        if entry.namespace.is_empty() {
            plain.push((entry.name.clone(), entry_api_spec(entry, defer_loading)));
            continue;
        }

        let mut child = entry_api_spec(entry, defer_loading);
        if let Some(object) = child.as_object_mut() {
            object.insert(
                "name".to_string(),
                serde_json::json!(namespace_child_name(entry)),
            );
        }
        namespaces
            .entry(entry.namespace.clone())
            .or_default()
            .push(child);
    }
    plain.sort_by(|left, right| left.0.cmp(&right.0));
    let mut specs: Vec<_> = plain.into_iter().map(|(_, spec)| spec).collect();
    specs.extend(namespaces.into_iter().map(|(name, mut tools)| {
        tools.sort_by(|left, right| {
            left.get("name")
                .and_then(serde_json::Value::as_str)
                .cmp(&right.get("name").and_then(serde_json::Value::as_str))
        });
        serde_json::json!({
            "type": "namespace",
            "name": name,
            "description": format!("Tools in the {name} namespace."),
            "tools": tools,
        })
    }));
    specs
}

impl ToolRegistry {
    /// 创建空注册表。
    pub fn new() -> Self {
        ToolRegistry {
            tools: HashMap::new(),
            dynamic_handlers: HashMap::new(),
            enabled: HashMap::new(),
            skill_override_enabled: std::collections::HashSet::new(),
        }
    }

    /// 注册一个动态工具（含 handler 闭包）。MCP 工具用此方法注册。
    pub fn register_dynamic(&mut self, entry: ToolEntry, handler: DynToolHandler) {
        let name = entry.name.clone();
        self.tools.insert(name.clone(), entry);
        self.dynamic_handlers.insert(name, handler);
    }

    /// 获取动态 handler 的共享快照（供释放注册表锁后执行）。
    pub fn dynamic_handler(&self, name: &str) -> Option<DynToolHandler> {
        self.dynamic_handlers.get(name).cloned()
    }

    /// 克隆动态 handler 句柄，供调用方在释放注册表锁后执行。
    pub fn dynamic_handler_cloned(&self, name: &str) -> Option<DynToolHandler> {
        self.dynamic_handlers.get(name).cloned()
    }

    /// 用外部加载的 toolset 启用映射覆盖当前状态（通常来自 Tauri 或磁盘同步）。
    pub fn set_enabled_map(&mut self, enabled: HashMap<String, bool>) {
        self.enabled = enabled;
    }

    /// 从磁盘重新加载指定 Agent 的 toolset 启用表并覆盖 `enabled` 映射。
    ///
    /// `agent_id` 为 `None` 时读全局 `~/.astro/tools-enabled.json`；否则读
    /// `AgentRuntimeConfig.tools_enabled`（缺失时回退全局），与前端
    /// `save_tools_enabled_for_agent` 对齐。
    pub fn reload_enabled_from_disk(&mut self, agent_id: Option<&str>) {
        self.enabled = home::sync_tools_enabled_defaults_for_agent(agent_id).unwrap_or_default();
    }

    /// 判断指定 toolset 是否启用；未在映射中出现时默认返回 `true`。
    ///
    /// Skill 激活的 `skill_override_enabled` 可 additive 放宽被禁用的 toolset。
    pub fn is_toolset_enabled(&self, toolset: &str) -> bool {
        if self.skill_override_enabled.contains(toolset) {
            return true;
        }
        self.enabled.get(toolset).copied().unwrap_or(true)
    }

    /// 将 skill 声明的 toolset 并入 additive 放宽集合。
    pub fn activate_skill_toolsets(&mut self, toolsets: &[String]) {
        for ts in toolsets {
            let t = ts.trim();
            if !t.is_empty() {
                self.skill_override_enabled.insert(t.to_string());
            }
        }
    }

    /// 当前 skill 放宽的 toolset 列表（测试 / 观测）。
    pub fn skill_override_toolsets(&self) -> Vec<String> {
        let mut v: Vec<_> = self.skill_override_enabled.iter().cloned().collect();
        v.sort();
        v
    }

    /// 判断指定工具名当前是否允许调用。
    ///
    /// MCP 工具（`mcp__` 前缀）以是否已注册为准；其余工具按名称映射到 toolset 后检查开关。
    pub fn is_tool_allowed(&self, name: &str) -> bool {
        // MCP：已注册即允许（注册时已按 server/tool 开关过滤）
        if name.starts_with("mcp__") {
            return self.tools.contains_key(name);
        }
        let toolset = home::tool_name_to_toolset(name);
        self.is_toolset_enabled(toolset)
    }

    /// 注册或覆盖一个工具条目（以 `entry.name` 为键）。
    pub fn register(&mut self, entry: ToolEntry) {
        self.tools.insert(entry.name.clone(), entry);
    }

    /// 移除指定 toolset 下的全部条目。
    ///
    /// 主要用于 MCP 热重载：先 `unregister_toolset("mcp")` 再重新注册最新工具列表。
    pub fn unregister_toolset(&mut self, toolset: &str) {
        let removed: Vec<String> = self
            .tools
            .iter()
            .filter(|(_, e)| e.toolset == toolset)
            .map(|(k, _)| k.clone())
            .collect();
        for name in &removed {
            self.tools.remove(name);
            self.dynamic_handlers.remove(name);
        }
    }

    /// 按名称移除单个工具（不存在则 no-op）。
    pub fn unregister(&mut self, name: &str) {
        self.tools.remove(name);
    }

    /// 检查是否已注册指定名称的工具。
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// 按名称查找工具元数据。
    pub fn get(&self, name: &str) -> Option<&ToolEntry> {
        self.tools.get(name)
    }

    /// 任一工具标记 `needs_confirmation`（未注册时回落 false）。
    pub fn any_needs_confirmation(&self, names: &[&str]) -> bool {
        names
            .iter()
            .any(|n| self.get(n).map(|e| e.needs_confirmation).unwrap_or(false))
    }

    /// 任一工具标记 `stop_after_tool_call`。
    pub fn any_stop_after(&self, names: &[&str]) -> bool {
        names
            .iter()
            .any(|n| self.get(n).map(|e| e.stop_after_tool_call).unwrap_or(false))
    }

    /// 任一工具标记 `exclusive_access`（未注册时回落 false）。
    pub fn any_exclusive_access(&self, names: &[&str]) -> bool {
        names
            .iter()
            .any(|n| self.get(n).map(|e| e.exclusive_access).unwrap_or(false))
    }

    /// 返回所有已注册工具条目的引用（不过滤启用状态与 `check_fn`）。
    pub fn all_tools(&self) -> Vec<&ToolEntry> {
        self.tools.values().collect()
    }

    /// 返回当前会话可用的工具：toolset 已启用且 `check_fn` 通过（若有）。
    ///
    /// MCP toolset 不受 `tools-enabled.json` 门控，仅依赖注册时的过滤逻辑。
    pub fn available_tools(&self) -> Vec<&ToolEntry> {
        self.tools
            .values()
            .filter(|e| {
                // MCP 门控只在 attach 时过滤，不走 tools-enabled.json
                if e.toolset == "mcp" {
                    return true;
                }
                self.is_toolset_enabled(&e.toolset)
            })
            .filter(|e| e.check_fn.as_ref().map(|f| f()).unwrap_or(true))
            .collect()
    }

    /// 返回当前可搜索但尚未注入模型的工具。
    pub fn searchable_deferred_tools(&self) -> Vec<&ToolEntry> {
        self.available_tools()
            .into_iter()
            .filter(|entry| entry.exposure.is_deferred())
            .collect()
    }

    /// 将可用工具序列化为 Responses API 原生工具载荷。
    ///
    /// 每个条目的 `parameters` 会经 [`crate::schema::sanitize_tool_schema`] 清理，
    /// 确保不含 `$ref`、`$defs` 等厂商不友好结构。
    ///
    /// **非 Direct 工具不包含在返回列表中**。Deferred 工具由
    /// `tool_search` 以原生 output 形式返回，不改写注册表中的 exposure。
    pub fn schemas_for_api(&self) -> Vec<serde_json::Value> {
        self.schemas_for_api_with_mode(types::ToolMode::Direct)
    }

    /// 按 Codex 的 Direct / CodeMode / CodeModeOnly 策略返回模型可见 schema。
    /// CodeMode 在宿主不可用时回退 Direct；CodeModeOnly 则 fail-closed。
    pub fn schemas_for_api_with_mode(
        &self,
        requested_mode: types::ToolMode,
    ) -> Vec<serde_json::Value> {
        let available = self.available_tools();
        let code_mode_available = available.iter().any(|entry| entry.name == "exec");
        let mode = if requested_mode == types::ToolMode::CodeMode && !code_mode_available {
            types::ToolMode::Direct
        } else {
            requested_mode
        };
        api_specs(available.into_iter().filter(|entry| {
            if !entry.exposure.is_direct() {
                return false;
            }
            let is_code_mode_control = matches!(entry.name.as_str(), "exec" | "wait");
            match mode {
                types::ToolMode::Direct => !is_code_mode_control,
                types::ToolMode::CodeMode => true,
                types::ToolMode::CodeModeOnly => is_code_mode_control,
            }
        }))
    }

    /// 执行路由可调用的全部 schema，包含仅 Code Mode 可用的工具。
    pub fn all_callable_tool_schemas(&self) -> Vec<serde_json::Value> {
        api_specs(
            self.available_tools()
                .into_iter()
                .filter(|entry| entry.exposure != types::ToolExposure::Hidden),
        )
    }
}

impl Default for ToolRegistry {
    /// 等价于 [`ToolRegistry::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn schema_names(reg: &ToolRegistry) -> Vec<String> {
        reg.schemas_for_api()
            .iter()
            .flat_map(|schema| {
                if schema.get("type").and_then(serde_json::Value::as_str) == Some("namespace") {
                    let namespace = schema
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    return schema
                        .get("tools")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|tool| tool.get("name").and_then(serde_json::Value::as_str))
                        .map(|name| format!("{namespace}.{name}"))
                        .collect::<Vec<_>>();
                }
                let name = if schema.get("type").and_then(serde_json::Value::as_str)
                    == Some("tool_search")
                {
                    Some("tool_search")
                } else {
                    schema
                        .get("name")
                        .or_else(|| schema.pointer("/function/name"))
                        .and_then(serde_json::Value::as_str)
                };
                name.map(str::to_string).into_iter().collect()
            })
            .collect()
    }

    fn assert_vendor_safe_parameters(schema: &serde_json::Value) {
        if let Some(children) = schema.get("tools").and_then(serde_json::Value::as_array) {
            for child in children {
                assert_vendor_safe_parameters(child);
            }
            return;
        }
        let Some(params) = schema
            .get("parameters")
            .or_else(|| schema.pointer("/function/parameters"))
        else {
            return;
        };
        assert_eq!(
            params.get("type").and_then(|value| value.as_str()),
            Some("object"),
            "tool parameters 应为 object: {params}"
        );
        if let Some(hazard) = crate::schema::schema_has_vendor_hazards(params) {
            panic!("tool parameters 仍含厂商不友好结构 `{hazard}`: {params}");
        }
    }

    /// 断言所有内置工具（含 deferred）的 parameters schema 不含厂商不友好结构。
    #[test]
    fn all_registered_tools_have_vendor_safe_parameters() {
        let mut reg = ToolRegistry::new();
        crate::register_all(&mut reg);
        let schemas = reg.all_callable_tool_schemas();
        assert!(!schemas.is_empty());
        for s in schemas {
            assert_vendor_safe_parameters(&s);
        }
    }

    #[test]
    fn disabled_toolset_excluded_from_schemas_for_api() {
        let mut reg = ToolRegistry::new();
        reg.register(ToolEntry {
            name: "memory".into(),
            toolset: "memory".into(),
            description: "add".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "brain",
            ..ToolEntry::lifecycle_defaults()
        });
        reg.register(ToolEntry {
            name: "cron".into(),
            toolset: "cron".into(),
            description: "list".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "clock",
            ..ToolEntry::lifecycle_defaults()
        });
        let mut enabled = HashMap::new();
        enabled.insert("memory".into(), false);
        enabled.insert("cron".into(), true);
        reg.set_enabled_map(enabled);

        let names = schema_names(&reg);
        assert!(!names.iter().any(|n| n == "memory"));
        assert!(names.iter().any(|n| n == "cron"));
        assert!(!reg.is_tool_allowed("memory"));
        assert!(reg.is_tool_allowed("cron"));
    }

    #[test]
    fn reload_reads_non_default_agent_tools_enabled() {
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let mut global = HashMap::new();
        global.insert("memory".into(), true);
        global.insert("cron".into(), true);
        home::save_tools_enabled(&global).unwrap();

        let mut alice = HashMap::new();
        alice.insert("memory".into(), false);
        alice.insert("cron".into(), true);
        home::save_tools_enabled_for_agent(Some("alice"), &alice).unwrap();

        let mut reg = ToolRegistry::new();
        reg.register(ToolEntry {
            name: "memory".into(),
            toolset: "memory".into(),
            description: "add".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "brain",
            ..ToolEntry::lifecycle_defaults()
        });
        reg.register(ToolEntry {
            name: "cron".into(),
            toolset: "cron".into(),
            description: "list".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "clock",
            ..ToolEntry::lifecycle_defaults()
        });

        // 全局仍启用 memory
        reg.reload_enabled_from_disk(None);
        assert!(reg.is_tool_allowed("memory"));

        // 非默认 agent 读取专属配置
        reg.reload_enabled_from_disk(Some("alice"));
        assert!(!reg.is_tool_allowed("memory"));
        assert!(reg.is_tool_allowed("cron"));
        let names = schema_names(&reg);
        assert!(!names.iter().any(|n| n == "memory"));
        assert!(names.iter().any(|n| n == "cron"));
    }

    #[test]
    fn skill_override_reenables_disabled_toolset() {
        let mut reg = ToolRegistry::new();
        reg.register(ToolEntry {
            name: "memory".into(),
            toolset: "memory".into(),
            description: "add".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "brain",
            ..ToolEntry::lifecycle_defaults()
        });
        let mut enabled = HashMap::new();
        enabled.insert("memory".into(), false);
        reg.set_enabled_map(enabled);
        assert!(!reg.is_tool_allowed("memory"));
        reg.activate_skill_toolsets(&["memory".into()]);
        assert!(reg.is_tool_allowed("memory"));
        assert_eq!(reg.skill_override_toolsets(), vec!["memory".to_string()]);
    }

    #[test]
    fn skill_activation_only_relaxes_the_toolset_gate() {
        let mut reg = ToolRegistry::new();
        reg.register(ToolEntry {
            name: "image_gen".into(),
            toolset: "image_gen".into(),
            description: "generate an image".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "palette",
            ..ToolEntry::lifecycle_defaults().deferred()
        });
        let mut enabled = HashMap::new();
        enabled.insert("image_gen".into(), false);
        reg.set_enabled_map(enabled);

        assert!(!schema_names(&reg).iter().any(|name| name == "image_gen"));
        reg.activate_skill_toolsets(&["image_gen".into()]);

        assert!(reg.is_tool_allowed("image_gen"));
        assert!(!schema_names(&reg).iter().any(|name| name == "image_gen"));
        assert!(reg
            .all_callable_tool_schemas()
            .iter()
            .any(|spec| spec["name"] == "image_gen"));
    }

    #[test]
    fn registered_builtins_mark_exclusive_tools() {
        let mut reg = ToolRegistry::new();
        crate::register_all(&mut reg);
        assert!(reg.any_exclusive_access(&["memory", "spawn_agent", "pin_context"]));
        assert!(!reg.any_exclusive_access(&["web_search"]));
        assert!(reg.get("persona_create").is_none());
        assert!(reg.get("context_search").is_some());
        assert!(reg.get("pin_context").unwrap().exclusive_access);
    }

    #[tokio::test]
    async fn dynamic_handler_snapshot_survives_registry_reload() {
        let mut reg = ToolRegistry::new();
        reg.register_dynamic(
            ToolEntry {
                name: "dynamic".into(),
                toolset: "mcp".into(),
                description: "dynamic test tool".into(),
                schema: serde_json::json!({"type": "object", "properties": {}}),
                check_fn: None,
                icon: "plug",
                ..ToolEntry::lifecycle_defaults()
            },
            std::sync::Arc::new(|_name, _args| {
                Box::pin(async { Ok(types::ToolOutput::from("snapshot")) })
            }),
        );

        let handler = reg.dynamic_handler("dynamic").expect("handler snapshot");
        reg.unregister_toolset("mcp");

        let output = handler("dynamic", &serde_json::json!({})).await.unwrap();
        assert_eq!(output.text(), "snapshot");
    }
}
