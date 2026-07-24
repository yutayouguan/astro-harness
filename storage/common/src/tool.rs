//! 工具元数据（名称、描述、JSON Schema），供目录与协议层使用。

use serde::{Deserialize, Serialize};

/// 描述一个可调用工具（不含实现）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// 工具名。
    pub name: String,
    /// 给人 / 模型看的说明。
    pub description: String,
    /// 参数 JSON Schema。
    pub schema: serde_json::Value,
}
