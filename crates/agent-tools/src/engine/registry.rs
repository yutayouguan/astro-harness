//! 工具注册表：集中管理内置与 MCP 工具的元数据、schema 与启用状态。
//!
//! `ToolRegistry` 是 Agent 与 LLM API 之间的桥梁，同时持有可调用工具的
//! [`ToolEntry`] 和 [`crate::CoreToolRuntime`]，并按当前 Agent 的 `tools_enabled`（或全局
//! `~/.astro/tools-enabled.json`）与运行时 `check_fn` 过滤出当前会话实际可用的
//! 工具列表，供 `schemas_for_api` 下发给模型。

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::context::ToolContext;

pub use types::tool_entry::ToolEntry;

/// 内置工具自注册钩子：各工具模块通过 `inventory::submit!` / [`crate::submit_builtin_tool!`] 报名。
///
/// `register` 会原子地注册元数据与宏生成的 [`crate::CoreToolRuntime`]。
pub struct BuiltinToolRegistrar {
    /// 向注册表写入本模块工具元数据与原生运行时。
    pub register: fn(&mut ToolRegistry),
    /// 本模块可分发的工具名（用于全局重名检查）。
    pub names: &'static [&'static str],
}

inventory::collect!(BuiltinToolRegistrar);

/// 工具注册表：以工具名为键的全局索引。
///
/// 同时维护 toolset 级别的启用映射；MCP 工具（`mcp__` 前缀）的开关在注册阶段
/// 已过滤，不走 `tools-enabled.json`。
/// 动态工具 handler（MCP 工具等运行时注册的异步调用闭包）。
///
/// 动态工具不需要 `ToolContext` — MCP 工具通过捕获的
/// `Arc<McpHub>` 自行完成调用。
pub type DynToolHandler = Arc<
    dyn Fn(
            &str,
            &serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<types::ToolOutput>> + Send>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct ToolRegistry {
    /// 已注册的全部工具条目。
    tools: HashMap<String, ToolEntry>,
    /// 工具执行运行时；与元数据使用同一 registered name 索引。
    runtimes: HashMap<String, Arc<dyn crate::engine::executor::CoreToolRuntime>>,
    /// 与当前 Agent / 全局 `tools-enabled` 对齐的 toolset 开关；缺失键视为启用。
    enabled: HashMap<String, bool>,
    /// Skill 加载后 additive 放宽的 toolset（即使 enabled 映射为 false 也允许）。
    skill_override_enabled: std::collections::HashSet<String>,
    /// 当前 turn 的 ExtensionSnapshot 声明的 toolset；每次发布 snapshot 时整体替换。
    extension_override_enabled: std::collections::HashSet<String>,
}

fn namespace_child_name(entry: &ToolEntry) -> String {
    match entry.tool_name() {
        types::ToolName::Plain(name) | types::ToolName::Namespaced { name, .. } => name,
    }
}

fn is_code_mode_control(name: &str) -> bool {
    matches!(name, "exec" | "wait")
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
    fn is_entry_available(&self, entry: &ToolEntry) -> bool {
        let toolset_enabled = if entry.toolset == "mcp" {
            true
        } else {
            self.is_toolset_enabled(&entry.toolset)
        };
        toolset_enabled && entry.check_fn.as_ref().map(|check| check()).unwrap_or(true)
    }

    /// 创建空注册表。
    pub fn new() -> Self {
        ToolRegistry {
            tools: HashMap::new(),
            runtimes: HashMap::new(),
            enabled: HashMap::new(),
            skill_override_enabled: std::collections::HashSet::new(),
            extension_override_enabled: std::collections::HashSet::new(),
        }
    }

    /// 注册一个动态工具（含 handler 闭包）。MCP 工具用此方法注册。
    pub fn register_dynamic(&mut self, entry: ToolEntry, handler: DynToolHandler) {
        let name = entry.name.clone();
        let runtime = Arc::new(crate::engine::executor::DynamicToolAdapter::new(
            entry.clone(),
            Arc::clone(&handler),
        ));
        self.tools.insert(name.clone(), entry);
        self.runtimes
            .insert(runtime.registered_name().to_string(), runtime);
    }

    /// 注册一个原生执行器及其元数据。
    pub fn register_runtime(
        &mut self,
        entry: ToolEntry,
        runtime: Arc<dyn crate::engine::executor::CoreToolRuntime>,
    ) {
        assert_eq!(
            runtime.tool_name(),
            entry.tool_name(),
            "CoreToolRuntime identity must match ToolEntry identity"
        );
        let name = entry.name.clone();
        self.tools.insert(name.clone(), entry);
        self.runtimes.insert(name, runtime);
    }

    /// 获取可跨 Step 快照共享的执行器。
    pub fn runtime(&self, name: &str) -> Option<Arc<dyn crate::engine::executor::CoreToolRuntime>> {
        self.runtimes.get(name).cloned()
    }

    /// 通过注册表中的 [`crate::CoreToolRuntime`] 执行工具。
    pub async fn dispatch(
        &self,
        ctx: &mut ToolContext<'_>,
        name: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<types::ToolOutput> {
        let runtime = self.runtime(name);
        let skill_runtime = (runtime.is_none() && name != "skills")
            .then(|| self.runtime("skills"))
            .flatten();
        crate::dispatch::dispatch_runtime(
            self.is_tool_allowed(name),
            runtime.as_ref(),
            skill_runtime.as_ref(),
            ctx,
            name,
            args,
        )
        .await
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
        if self.skill_override_enabled.contains(toolset)
            || self.extension_override_enabled.contains(toolset)
        {
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

    /// 原子替换当前 turn 的扩展 toolset 贡献。
    ///
    /// Skill 激活集合是会话级 additive 状态；扩展集合则跟随
    /// `ExtensionSnapshot`，因此必须可在下一 turn 撤销。
    pub fn set_extension_toolsets(&mut self, toolsets: &[String]) {
        self.extension_override_enabled = toolsets
            .iter()
            .map(|toolset| toolset.trim())
            .filter(|toolset| !toolset.is_empty())
            .map(str::to_string)
            .collect();
    }

    /// 当前 ExtensionSnapshot 放宽的 toolset 列表（测试 / 观测）。
    pub fn extension_toolsets(&self) -> Vec<String> {
        let mut values = self
            .extension_override_enabled
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        values.sort();
        values
    }

    /// 判断指定工具名当前是否允许调用。
    ///
    /// MCP 工具（`mcp__` 前缀）以是否已注册为准；其余工具按名称映射到 toolset 后检查开关。
    pub fn is_tool_allowed(&self, name: &str) -> bool {
        if name.starts_with("mcp__") {
            return self.tools.contains_key(name);
        }
        if let Some(entry) = self.tools.get(name) {
            // MCP broker 等不使用 `mcp__` 前缀的动态工具仍以 toolset 判定。
            return entry.toolset == "mcp" || self.is_toolset_enabled(&entry.toolset);
        }
        let toolset = home::tool_name_to_toolset(name);
        self.is_toolset_enabled(toolset)
    }

    /// 注册或覆盖一个工具条目（以 `entry.name` 为键）。
    pub fn register(&mut self, entry: ToolEntry) {
        let name = entry.name.clone();
        // 元数据被替换后，旧 runtime 不得继续与新 schema 组合。
        self.runtimes.remove(&name);
        self.tools.insert(name, entry);
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
            self.runtimes.remove(name);
        }
    }

    /// 按名称移除单个工具（不存在则 no-op）。
    pub fn unregister(&mut self, name: &str) {
        self.tools.remove(name);
        self.runtimes.remove(name);
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
            .filter(|entry| self.is_entry_available(entry))
            .collect()
    }

    /// 返回当前可搜索但尚未注入模型的工具。
    pub fn searchable_deferred_tools(&self) -> Vec<&ToolEntry> {
        self.tools
            .values()
            .filter(|entry| entry.exposure.is_deferred() && !is_code_mode_control(&entry.name))
            .filter(|entry| self.is_entry_available(entry))
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
            .expect("Direct 模式不依赖 Code Mode 运行时")
    }

    /// 解析 Code Mode 可用性，并应用仅允许混合模式降级的规则。
    pub fn effective_tool_mode(
        &self,
        requested: types::ToolMode,
    ) -> anyhow::Result<types::ToolMode> {
        let controls_available = ["exec", "wait"].into_iter().all(|name| {
            self.tools
                .get(name)
                .is_some_and(|entry| self.is_entry_available(entry))
        });
        match (requested, controls_available) {
            (types::ToolMode::CodeMode, false) => Ok(types::ToolMode::Direct),
            (types::ToolMode::CodeModeOnly, false) => {
                anyhow::bail!(
                    "CodeModeOnly requested but the embedded Code Mode runtime is unavailable"
                )
            }
            _ => Ok(requested),
        }
    }

    /// 按模型选择的工具模式生成模型可见 schema。
    pub fn schemas_for_api_with_mode(
        &self,
        requested: types::ToolMode,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let mode = self.effective_tool_mode(requested)?;
        Ok(api_specs(
            self.tools
                .values()
                .filter(|entry| entry.exposure.is_direct())
                .filter(|entry| self.is_entry_available(entry))
                .filter(|entry| match mode {
                    types::ToolMode::Direct => !is_code_mode_control(&entry.name),
                    types::ToolMode::CodeMode => true,
                    types::ToolMode::CodeModeOnly => is_code_mode_control(&entry.name),
                }),
        ))
    }

    /// 一次性构建当前 Step 的模型可见 schema 与额外可路由 Deferred schema。
    ///
    /// Deferred 工具只有已经出现在可信 `tool_search_output` 中时才进入路由，
    /// 防止模型仅凭猜测名称绕过发现流程。单次扫描也避免重复运行工具的
    /// `check_fn`，其中浏览器等探测可能涉及文件系统查询。
    pub fn schemas_for_step(
        &self,
        discovered_deferred: &HashSet<types::ToolName>,
    ) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
        let (visible, callable, _) = self
            .schemas_for_step_with_mode(types::ToolMode::Direct, discovered_deferred)
            .expect("Direct 模式不依赖 Code Mode 运行时");
        (visible, callable)
    }

    /// 构建某个工具模式下的模型可见、模型额外可调用和 Code Mode 嵌套路由。
    ///
    /// 第三个返回值只供 `exec` 内部使用，不能并入模型直接调用集合，否则模型可
    /// 通过猜测名称绕过 `CodeModeOnly` 或 Deferred 发现边界。
    pub fn schemas_for_step_with_mode(
        &self,
        requested: types::ToolMode,
        discovered_deferred: &HashSet<types::ToolName>,
    ) -> anyhow::Result<(
        Vec<serde_json::Value>,
        Vec<serde_json::Value>,
        Vec<serde_json::Value>,
    )> {
        let mode = self.effective_tool_mode(requested)?;
        let mut direct = Vec::new();
        let mut discovered = Vec::new();
        let mut nested = Vec::new();
        for entry in self.tools.values() {
            let is_control = is_code_mode_control(&entry.name);
            let is_direct = entry.exposure.is_direct()
                && match mode {
                    types::ToolMode::Direct => !is_control,
                    types::ToolMode::CodeMode => true,
                    types::ToolMode::CodeModeOnly => is_control,
                };
            let is_discovered = mode != types::ToolMode::CodeModeOnly
                && !is_control
                && entry.exposure.is_deferred()
                && discovered_deferred.contains(&entry.tool_name());
            let is_nested = mode != types::ToolMode::Direct
                && !is_control
                && entry.name != "tool_search"
                && !entry.exposure.is_hidden()
                && (!entry.exposure.is_deferred()
                    || entry.allow_eager_fallback
                    || discovered_deferred.contains(&entry.tool_name()));
            if (!is_direct && !is_discovered && !is_nested) || !self.is_entry_available(entry) {
                continue;
            }
            if is_direct {
                direct.push(entry);
            } else if is_discovered {
                discovered.push(entry);
            }
            if is_nested {
                nested.push(entry);
            }
        }
        Ok((api_specs(direct), api_specs(discovered), api_specs(nested)))
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
        let discovered = reg
            .searchable_deferred_tools()
            .into_iter()
            .map(ToolEntry::tool_name)
            .collect();
        let (mut schemas, deferred) = reg.schemas_for_step(&discovered);
        schemas.extend(deferred);
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
    fn extension_toolsets_are_replaceable_at_turn_boundary() {
        let mut reg = ToolRegistry::new();
        reg.register(ToolEntry {
            name: "image_gen".into(),
            toolset: "image_gen".into(),
            description: "generate".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "palette",
            ..ToolEntry::lifecycle_defaults().deferred()
        });
        let mut enabled = HashMap::new();
        enabled.insert("image_gen".into(), false);
        reg.set_enabled_map(enabled);

        reg.set_extension_toolsets(&["image_gen".into()]);
        assert!(reg.is_tool_allowed("image_gen"));
        assert_eq!(reg.extension_toolsets(), vec!["image_gen".to_string()]);

        reg.set_extension_toolsets(&[]);
        assert!(!reg.is_tool_allowed("image_gen"));
        assert!(reg.extension_toolsets().is_empty());
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
        let discovered = HashSet::from([types::ToolName::plain("image_gen")]);
        assert!(reg
            .schemas_for_step(&discovered)
            .1
            .iter()
            .any(|spec| spec["name"] == "image_gen"));
    }

    #[test]
    fn step_schema_snapshot_only_routes_discovered_deferred_tools_and_checks_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let checks = Arc::new(AtomicUsize::new(0));
        let mut reg = ToolRegistry::new();
        for (name, deferred) in [("direct", false), ("found", true), ("unseen", true)] {
            let checks = Arc::clone(&checks);
            let mut entry = ToolEntry {
                name: name.into(),
                toolset: "core".into(),
                check_fn: Some(Arc::new(move || {
                    checks.fetch_add(1, Ordering::Relaxed);
                    true
                })),
                ..ToolEntry::lifecycle_defaults()
            };
            if deferred {
                entry = entry.deferred();
            }
            reg.register(entry);
        }

        let discovered = HashSet::from([types::ToolName::plain("found")]);
        let (visible, routable_deferred) = reg.schemas_for_step(&discovered);
        let names = |specs: &[serde_json::Value]| {
            specs
                .iter()
                .filter_map(|spec| spec.get("name").and_then(serde_json::Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        };

        assert_eq!(names(&visible), vec!["direct".to_string()]);
        assert_eq!(names(&routable_deferred), vec!["found".to_string()]);
        assert_eq!(checks.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn removed_code_mode_controls_cannot_reenter_through_deferred_search() {
        let mut reg = ToolRegistry::new();
        for name in ["exec", "wait"] {
            reg.register(ToolEntry {
                name: name.into(),
                toolset: "plugin".into(),
                ..ToolEntry::lifecycle_defaults().deferred()
            });
        }

        assert!(reg.searchable_deferred_tools().is_empty());
        let discovered = HashSet::from([
            types::ToolName::plain("exec"),
            types::ToolName::plain("wait"),
        ]);
        let (visible, routable_deferred) = reg.schemas_for_step(&discovered);
        assert!(visible.is_empty());
        assert!(routable_deferred.is_empty());
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

    #[test]
    fn dynamic_runtime_snapshot_survives_registry_reload() {
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

        let snapshot = reg.clone();
        reg.unregister_toolset("mcp");

        assert!(reg.runtime("dynamic").is_none());
        let runtime = snapshot.runtime("dynamic").expect("runtime snapshot");
        assert_eq!(runtime.tool_name(), types::ToolName::plain("dynamic"));
    }

    #[test]
    fn replacing_metadata_invalidates_the_previous_runtime() {
        let mut reg = ToolRegistry::new();
        reg.register_dynamic(
            ToolEntry {
                name: "dynamic".into(),
                toolset: "mcp".into(),
                description: "old".into(),
                ..ToolEntry::lifecycle_defaults()
            },
            Arc::new(|_name, _args| Box::pin(async { Ok(types::ToolOutput::from("old")) })),
        );
        assert!(reg.runtime("dynamic").is_some());

        reg.register(ToolEntry {
            name: "dynamic".into(),
            toolset: "mcp".into(),
            description: "new metadata without runtime".into(),
            ..ToolEntry::lifecycle_defaults()
        });

        assert!(reg.runtime("dynamic").is_none());
    }
}
