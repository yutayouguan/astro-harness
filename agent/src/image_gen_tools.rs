//! 图像生成工具注册（遗留路径）。
//!
//! 向本地 `ToolRegistry` 注册 `image_gen` 工具定义与 JSON Schema。
//! 新代码应优先使用 `tools` crate 中的统一注册；本模块保留以兼容旧调用链。

use crate::tool_registry::{ToolEntry, ToolRegistry};

/// 将 `image_gen` 工具条目写入注册表。
///
/// 工具集名为 `image_gen`；启用 Google 时用 Gemini 图像模型，否则回退 OpenAI `gpt-image-2`。
/// 运行时实际路由由 executor 层决定，此处仅声明名称、描述与参数 schema。
///
/// # 参数
///
/// - `registry`：可变工具注册表，同名工具会被覆盖。
pub fn register_image_gen_tools(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "image_gen".to_string(),
        toolset: "image_gen".to_string(),
        description: "Generate an image from a text prompt. Uses Gemini image model when Google is enabled, otherwise OpenAI gpt-image-2."
            .to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Detailed description of the image to generate"
                },
                "size": {
                    "type": "string",
                    "description": "Optional size: 1024x1024, 1024x1536, 1536x1024",
                    "enum": ["1024x1024", "1024x1536", "1536x1024", "512x512"]
                }
            },
            "required": ["prompt"]
        }),
        check_fn: None,
        emoji: "🎨",
    });
}
