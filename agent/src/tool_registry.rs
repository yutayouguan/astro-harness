//! 工具注册表（遗留本地实现）。
//!
//! 维护工具元数据、工具集启用状态，并导出 Provider 所需的 function schema 列表。
//! `lib.rs` 已 re-export `tools::ToolRegistry`；本文件保留旧版实现供 `image_gen_tools` /
//! `cron_tools` 等模块引用，行为与 `tools` crate 应对齐。

use std::collections::HashMap;

/// 单个可调用工具的静态描述与可选运行时门禁。
pub struct ToolEntry {
    /// Provider function 名称，与 executor 分发键一致。
    pub name: String,
    /// 所属工具集，用于批量启用/禁用（对应 `tools-enabled.json` 键）。
    pub toolset: String,
    /// 注入模型 system / tools 列表的自然语言说明。
    pub description: String,
    /// JSON Schema 形参定义（OpenAI function `parameters` 格式）。
    pub schema: serde_json::Value,
    /// 可选运行时检查；返回 `false` 时该工具不出现在 `available_tools` 中。
    pub check_fn: Option<Box<dyn Fn() -> bool + Send + Sync>>,
    /// UI 或日志中展示的 emoji 标识。
    pub emoji: &'static str,
}

/// 工具名 → 条目映射，叠加工具集启用过滤。
pub struct ToolRegistry {
    /// 已注册工具表，键为 `ToolEntry::name`。
    tools: HashMap<String, ToolEntry>,
    /// 工具集启用表（与 ~/.astro/tools-enabled.json 对齐）；缺失视为启用
    enabled: HashMap<String, bool>,
}

impl ToolRegistry {
    /// 创建空注册表；启用表默认为空（全部工具集视为开启）。
    pub fn new() -> Self {
        ToolRegistry {
            tools: HashMap::new(),
            enabled: HashMap::new(),
        }
    }

    /// 用外部加载的映射覆盖当前工具集启用状态。
    pub fn set_enabled_map(&mut self, enabled: HashMap<String, bool>) {
        self.enabled = enabled;
    }

    /// 从磁盘（按 Agent 或全局）重新加载启用表；读失败时清空为默认全启用语义。
    pub fn reload_enabled_from_disk(&mut self, agent_id: Option<&str>) {
        self.enabled =
            memory::sync_tools_enabled_defaults_for_agent(agent_id).unwrap_or_default();
    }

    /// 查询工具集是否启用；未出现在映射中则视为 `true`。
    pub fn is_toolset_enabled(&self, toolset: &str) -> bool {
        self.enabled.get(toolset).copied().unwrap_or(true)
    }

    /// 按工具名解析所属工具集并检查是否允许调用。
    pub fn is_tool_allowed(&self, name: &str) -> bool {
        let toolset = memory::tool_name_to_toolset(name);
        self.is_toolset_enabled(toolset)
    }

    /// 注册或覆盖同名工具条目。
    pub fn register(&mut self, entry: ToolEntry) {
        self.tools.insert(entry.name.clone(), entry);
    }

    /// 返回当前可用工具引用列表：工具集已启用且 `check_fn` 通过（或无门禁）。
    pub fn available_tools(&self) -> Vec<&ToolEntry> {
        self.tools
            .values()
            .filter(|e| self.is_toolset_enabled(&e.toolset))
            .filter(|e| e.check_fn.as_ref().map(|f| f()).unwrap_or(true))
            .collect()
    }

    /// 将可用工具转为 OpenAI Chat Completions `tools` 数组元素。
    pub fn schemas_for_api(&self) -> Vec<serde_json::Value> {
        self.available_tools()
            .iter()
            .map(|e| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": e.name,
                        "description": e.description,
                        "parameters": e.schema,
                    }
                })
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    /// 等价于 `ToolRegistry::new()`。
    fn default() -> Self {
        Self::new()
    }
}
