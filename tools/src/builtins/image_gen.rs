//! 图片生成工具：按文本提示调用 Google / OpenAI 等图片 Provider。
//!
//! 凭据来自 [`ToolContext::image_gen_targets`]：先试 primary，失败再试 fallback。
//! 成功图片写入工作区 `generated/`。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use providers::trait_::ProviderConfig;

use crate::context::{ImageGenCreds, ToolContext};
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `image_gen` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImageGenArgs {
    /// 图片的详细文字描述（不可为空）。
    pub prompt: String,
}

/// 向注册表登记 `image_gen` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "image_gen".to_string(),
        toolset: "image_gen".to_string(),
        description: "Generate an image from a text prompt. Uses Gemini image model when Google is enabled, otherwise OpenAI gpt-image-2."
            .to_string(),
        schema: schema_for_args::<ImageGenArgs>(),
        check_fn: None,
        icon: "palette",
    });
}

/// 依次尝试 primary / fallback 凭据生成图片，返回本地路径与所用模型信息。
///
/// # 错误
/// 无可用 Provider、全部尝试失败，或缺少 `prompt`。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let prompt = args
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("image_gen 需要 prompt 参数"))?;

    if ctx.image_gen_targets.is_empty() {
        anyhow::bail!(
            "未找到可用的图片生成提供商。请在「模型提供商」中开启 Google 或 OpenAI，并配置 API Key。"
        );
    }

    let mut errors = Vec::new();
    for creds in [
        ctx.image_gen_targets.primary.as_ref(),
        ctx.image_gen_targets.fallback.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        match generate_one(ctx, prompt, creds).await {
            Ok(path) => {
                return Ok(format!(
                    "图片已生成：{path}\nprovider={}\nmodel={}",
                    creds.provider, creds.model
                ));
            }
            Err(err) => {
                errors.push(format!("{} ({}): {err}", creds.provider, creds.model));
            }
        }
    }

    anyhow::bail!("图片生成失败：{}", errors.join("；"))
}

/// 使用单一凭据调用 Provider，并将首张图片落盘。
///
/// # 返回
/// 生成文件的绝对/显示路径字符串。
async fn generate_one(
    ctx: &ToolContext<'_>,
    prompt: &str,
    creds: &ImageGenCreds,
) -> anyhow::Result<String> {
    let provider = ctx
        .providers
        .get(&creds.provider)
        .ok_or_else(|| anyhow::anyhow!("未知 Provider: {}", creds.provider))?;

    let config = ProviderConfig {
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        model: creds.model.clone(),
        ..ProviderConfig::default()
    };

    let images = provider.generate_image(prompt, &config).await?;
    let img = images
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("未返回图片数据"))?;

    let dir = ctx.workspace_dir.join("generated");
    std::fs::create_dir_all(&dir)?;
    let ext = if img.mime_type.contains("jpeg") || img.mime_type.contains("jpg") {
        "jpg"
    } else if img.mime_type.contains("webp") {
        "webp"
    } else {
        "png"
    };
    let filename = format!(
        "img-{}-{}.{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8],
        ext
    );
    let path = dir.join(filename);
    std::fs::write(&path, &img.data)?;
    Ok(path.display().to_string())
}
