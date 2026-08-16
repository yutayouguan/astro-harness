//! 工具注册元数据。

/// 单个可注册工具的完整元数据条目。
pub struct ToolEntry {
    pub name: String,
    pub toolset: String,
    pub description: String,
    pub schema: serde_json::Value,
    pub check_fn: Option<Box<dyn Fn() -> bool + Send + Sync>>,
    pub icon: &'static str,
    pub needs_confirmation: bool,
    pub stop_after_tool_call: bool,
    pub exclusive_access: bool,
}

impl ToolEntry {
    pub fn lifecycle_defaults() -> Self {
        Self {
            name: String::new(),
            toolset: String::new(),
            description: String::new(),
            schema: serde_json::json!({ "type": "object", "properties": {} }),
            check_fn: None,
            icon: "wrench",
            needs_confirmation: false,
            stop_after_tool_call: false,
            exclusive_access: false,
        }
    }

    pub fn with_confirmation(mut self) -> Self {
        self.needs_confirmation = true;
        self
    }

    pub fn stop_after(mut self) -> Self {
        self.stop_after_tool_call = true;
        self
    }

    pub fn exclusive(mut self) -> Self {
        self.exclusive_access = true;
        self
    }
}
