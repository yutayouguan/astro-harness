//! 工具注册表：集中管理内置与 MCP 工具的元数据、schema 与启用状态。
//!
//! `ToolRegistry` 是 Agent 与 LLM API 之间的桥梁，持有所有可调用工具的
//! [`ToolEntry`]，并按当前 Agent 的 `tools_enabled`（或全局
//! `~/.astro/tools-enabled.json`）与运行时 `check_fn` 过滤出当前会话实际可用的
//! 工具列表，供 `schemas_for_api` 下发给模型。

use std::collections::HashMap;

/// 单个可注册工具的完整元数据条目。
///
/// 注册后由 [`ToolRegistry`] 以 `name` 为键存储；`schema` 在导出 API 前会经
/// [`crate::schema::sanitize_tool_schema`] 清理，以兼容各 LLM 厂商的 function calling 格式。
pub struct ToolEntry {
    /// 工具唯一名称，与 `dispatch` 路由及 LLM `function.name` 对齐。
    pub name: String,
    /// 所属 toolset id，与前端开关及 `tools-enabled.json` 键名一致。
    pub toolset: String,
    /// 面向模型的自然语言说明，描述工具用途与适用场景。
    pub description: String,
    /// 参数 JSON Schema（object 类型），通常由 `schema_for_args` 或 `tool_schema!` 生成。
    pub schema: serde_json::Value,
    /// 可选运行时可用性检查；返回 `false` 时该工具不出现在 `available_tools` 中。
    pub check_fn: Option<Box<dyn Fn() -> bool + Send + Sync>>,
    /// Lucide 图标 id（kebab-case，如 `"calendar-check"`），供 UI 目录展示。
    pub icon: &'static str,
}

/// 工具注册表：以工具名为键的全局索引。
///
/// 同时维护 toolset 级别的启用映射；MCP 工具（`mcp__` 前缀）的开关在注册阶段
/// 已过滤，不走 `tools-enabled.json`。
pub struct ToolRegistry {
    /// 已注册的全部工具条目。
    tools: HashMap<String, ToolEntry>,
    /// 与当前 Agent / 全局 `tools-enabled` 对齐的 toolset 开关；缺失键视为启用。
    enabled: HashMap<String, bool>,
}

impl ToolRegistry {
    /// 创建空注册表。
    pub fn new() -> Self {
        ToolRegistry {
            tools: HashMap::new(),
            enabled: HashMap::new(),
        }
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
        self.enabled =
            home::sync_tools_enabled_defaults_for_agent(agent_id).unwrap_or_default();
    }

    /// 判断指定 toolset 是否启用；未在映射中出现时默认返回 `true`。
    pub fn is_toolset_enabled(&self, toolset: &str) -> bool {
        self.enabled.get(toolset).copied().unwrap_or(true)
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
        self.tools.retain(|_, e| e.toolset != toolset);
    }

    /// 按名称移除单个工具（不存在则 no-op）。
    pub fn unregister(&mut self, name: &str) {
        self.tools.remove(name);
    }

    /// 检查是否已注册指定名称的工具。
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
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

    /// 将可用工具序列化为 OpenAI 风格的 `tools` / `functions` API 载荷。
    ///
    /// 每个条目的 `parameters` 会经 [`crate::schema::sanitize_tool_schema`] 清理，
    /// 确保不含 `$ref`、`$defs` 等厂商不友好结构。
    pub fn schemas_for_api(&self) -> Vec<serde_json::Value> {
        self.available_tools()
            .iter()
            .map(|e| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": e.name,
                        "description": e.description,
                        "parameters": crate::schema::sanitize_tool_schema(e.schema.clone()),
                    }
                })
            })
            .collect()
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
            .filter_map(|s| {
                s.pointer("/function/name")
                    .and_then(|n| n.as_str())
                    .map(str::to_string)
            })
            .collect()
    }

    /// 断言所有内置工具的 parameters schema 不含厂商不友好结构。
    #[test]
    fn all_registered_tools_have_vendor_safe_parameters() {
        let mut reg = ToolRegistry::new();
        crate::register_all(&mut reg);
        let schemas = reg.schemas_for_api();
        assert!(!schemas.is_empty());
        for s in schemas {
            let name = s
                .pointer("/function/name")
                .and_then(|n| n.as_str())
                .unwrap_or("?");
            let params = s
                .pointer("/function/parameters")
                .expect("missing parameters");
            assert_eq!(
                params.get("type").and_then(|t| t.as_str()),
                Some("object"),
                "tool `{name}` type 应为 object: {params}"
            );
            if let Some(hazard) = crate::schema::schema_has_vendor_hazards(params) {
                panic!("tool `{name}` 仍含厂商不友好结构 `{hazard}`: {params}");
            }
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
        });
        reg.register(ToolEntry {
            name: "cron_list".into(),
            toolset: "scheduled".into(),
            description: "list".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "clock",
        });
        let mut enabled = HashMap::new();
        enabled.insert("memory".into(), false);
        enabled.insert("scheduled".into(), true);
        reg.set_enabled_map(enabled);

        let names = schema_names(&reg);
        assert!(!names.iter().any(|n| n == "memory"));
        assert!(names.iter().any(|n| n == "cron_list"));
        assert!(!reg.is_tool_allowed("memory"));
        assert!(reg.is_tool_allowed("cron_list"));
    }

    #[test]
    fn reload_reads_non_default_agent_tools_enabled() {
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let mut global = HashMap::new();
        global.insert("memory".into(), true);
        global.insert("scheduled".into(), true);
        home::save_tools_enabled(&global).unwrap();

        let mut alice = HashMap::new();
        alice.insert("memory".into(), false);
        alice.insert("scheduled".into(), true);
        home::save_tools_enabled_for_agent(Some("alice"), &alice).unwrap();

        let mut reg = ToolRegistry::new();
        reg.register(ToolEntry {
            name: "memory".into(),
            toolset: "memory".into(),
            description: "add".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "brain",
        });
        reg.register(ToolEntry {
            name: "cron_list".into(),
            toolset: "scheduled".into(),
            description: "list".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "clock",
        });

        // 全局仍启用 memory
        reg.reload_enabled_from_disk(None);
        assert!(reg.is_tool_allowed("memory"));

        // 非默认 agent 读取专属配置
        reg.reload_enabled_from_disk(Some("alice"));
        assert!(!reg.is_tool_allowed("memory"));
        assert!(reg.is_tool_allowed("cron_list"));
        let names = schema_names(&reg);
        assert!(!names.iter().any(|n| n == "memory"));
        assert!(names.iter().any(|n| n == "cron_list"));
    }
}

impl Default for ToolRegistry {
    /// 等价于 [`ToolRegistry::new`]。
    fn default() -> Self {
        Self::new()
    }
}
