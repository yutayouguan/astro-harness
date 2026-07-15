//! 视频生成：Google OpenAI 兼容 `…/videos`（Veo），写入工作区 `generated/`。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use providers::media_http::{default_video_model, google_openai_generate_video};
use providers::trait_::ProviderConfig;

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `video_gen` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct VideoGenArgs {
    /// 视频画面的文字描述（不可为空）。
    pub prompt: String,
    /// 宽高比，如 `16:9` / `9:16`（可选）。
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    /// 时长秒数（可选，取决于 Veo 支持范围）。
    #[serde(default)]
    pub duration_seconds: Option<u32>,
}

/// 向注册表登记 `video_gen` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "video_gen".to_string(),
        toolset: "video_gen".to_string(),
        description: "Generate a short video from a text prompt via Google Veo (OpenAI-compatible videos API). Requires an enabled Google provider with API key. May take several minutes."
            .to_string(),
        schema: schema_for_args::<VideoGenArgs>(),
        check_fn: None,
        icon: "clapperboard",
    });
}

/// 调用 Google 视频接口并落盘。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: VideoGenArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("video_gen 参数无效: {e}"))?;
    let prompt = parsed.prompt.trim();
    if prompt.is_empty() {
        anyhow::bail!("video_gen 需要 prompt");
    }

    let creds = ctx.image_gen_targets.google().ok_or_else(|| {
        anyhow::anyhow!(
            "未找到可用的 Google 提供商。请在「模型提供商」中开启 Google 并配置 API Key。"
        )
    })?;

    let model = if creds.video_model.trim().is_empty() {
        default_video_model().to_string()
    } else {
        creds.video_model.trim().to_string()
    };
    let config = ProviderConfig {
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        model: model.clone(),
        ..ProviderConfig::default()
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let video = google_openai_generate_video(
        &client,
        prompt,
        &config,
        parsed.aspect_ratio.as_deref(),
        parsed.duration_seconds,
    )
    .await?;

    let dir = ctx.workspace_dir.join("generated");
    std::fs::create_dir_all(&dir)?;
    let filename = format!(
        "vid-{}-{}.mp4",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let path = dir.join(&filename);
    std::fs::write(&path, &video.data)?;
    Ok(format!(
        "视频已生成：{}\nprovider=google\nmodel={model}\noperation_id={}",
        path.display(),
        video.operation_id
    ))
}
