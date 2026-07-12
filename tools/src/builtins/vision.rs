//! 视觉工具：对本地路径或 URL 图片发起描述 / 问答请求。
//!
//! 当前 chat 流仅支持纯文本：本工具做路径存在性检查并返回可读提示，
//! 真正的多模态调用需后续接入原生 vision API。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `vision` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct VisionArgs {
    /// 工作区内相对路径，或 `http(s)://` URL。
    pub image_url: String,
    /// 对图片的问题或指令；缺省为「请描述这张图片」。
    #[serde(default)]
    pub prompt: Option<String>,
}

/// 向注册表登记 `vision` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "vision".to_string(),
        toolset: "vision".to_string(),
        description: "Describe or answer questions about an image given a local path or URL."
            .to_string(),
        schema: schema_for_args::<VisionArgs>(),
        check_fn: None,
        icon: "eye",
    });
}

/// 校验图片路径/URL，并返回当前实现的占位说明。
///
/// # 错误
/// 参数无效或 `image_url` 为空。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: VisionArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("vision 参数无效: {e}"))?;
    let image_url = parsed.image_url.trim();
    if image_url.is_empty() {
        anyhow::bail!("vision 需要 image_url");
    }
    let prompt = parsed
        .prompt
        .as_deref()
        .unwrap_or("请描述这张图片");

    // 当前 chat_stream 仅支持纯文本；先返回可读提示，避免静默失败
    let exists_hint = if image_url.starts_with("http://") || image_url.starts_with("https://") {
        "远程 URL".to_string()
    } else {
        let p = ctx.workspace_dir.join(image_url);
        if p.exists() {
            format!("本地文件存在: {}", p.display())
        } else {
            format!("本地文件不存在: {}", p.display())
        }
    };

    Ok(format!(
        "vision 工具已接收请求。\nimage={image_url} ({exists_hint})\nprompt={prompt}\n\
         说明：多模态 vision 调用需 Provider 支持图片输入；当前请在对话中直接附带图片，或后续版本将走原生 vision API。"
    ))
}
