//! 文本转语音：Google Gemini TTS 优先，OpenAI `audio/speech` 备用。
//!
//! Google 凭证来自 [`ToolContext::image_gen_targets`]（与出图共用 Google key）；
//! OpenAI 依次尝试 targets、当前聊天 OpenAI、`OPENAI_API_KEY`。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use providers::http_stream::openai_compatible_base;
use providers::media_http::{default_tts_model, google_tts_generate};
use providers::trait_::ProviderConfig;

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `tts` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TtsArgs {
    /// 待合成的文本（不可为空）。
    pub text: String,
    /// 音色：Google 预置如 `Kore`；OpenAI 路径可用 `alloy` / `echo` 等；缺省按供应商默认。
    #[serde(default)]
    pub voice: Option<String>,
}

/// 向注册表登记 `tts` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "tts".to_string(),
        toolset: "tts".to_string(),
        description: "Convert text to speech. Uses Gemini TTS when Google is enabled, otherwise OpenAI audio/speech."
            .to_string(),
        schema: schema_for_args::<TtsArgs>(),
        check_fn: None,
        icon: "mic",
    });
}

/// 请求 TTS，将音频保存到工作区并返回路径。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: TtsArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("tts 参数无效: {e}"))?;
    let text = parsed.text.trim();
    if text.is_empty() {
        anyhow::bail!("tts 需要 text");
    }
    let voice = parsed.voice.as_deref().unwrap_or("");

    let mut errors = Vec::new();
    if let Some(creds) = ctx.image_gen_targets.google() {
        match synthesize_google(ctx, text, voice, creds).await {
            Ok(msg) => return Ok(msg),
            Err(e) => errors.push(format!("google: {e}")),
        }
    }

    match synthesize_openai(ctx, text, if voice.is_empty() { "alloy" } else { voice }).await
    {
        Ok(msg) => Ok(msg),
        Err(e) => {
            errors.push(format!("openai: {e}"));
            anyhow::bail!(
                "tts 失败：{}。请配置 Google 或 OpenAI API Key。",
                errors.join("；")
            )
        }
    }
}

async fn synthesize_google(
    ctx: &ToolContext<'_>,
    text: &str,
    voice: &str,
    creds: &crate::context::ImageGenCreds,
) -> anyhow::Result<String> {
    let config = ProviderConfig {
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        model: default_tts_model().to_string(),
        ..ProviderConfig::default()
    };
    let client = reqwest::Client::new();
    let wav = google_tts_generate(&client, text, voice, &config).await?;
    let dir = ctx.workspace_dir.join("generated");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!(
        "tts-{}-{}.wav",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    std::fs::write(&path, &wav)?;
    Ok(format!(
        "语音已生成：{}\nprovider=google\nmodel={}",
        path.display(),
        default_tts_model()
    ))
}

async fn synthesize_openai(
    ctx: &ToolContext<'_>,
    text: &str,
    voice: &str,
) -> anyhow::Result<String> {
    let (api_key, base) = resolve_openai_tts(ctx)?;
    let url = format!("{base}/audio/speech");
    let body = serde_json::json!({
        "model": "gpt-4o-mini-tts",
        "input": text,
        "voice": voice,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI TTS HTTP {status}: {err}");
    }
    let bytes = resp.bytes().await?;
    let dir = ctx.workspace_dir.join("generated");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!(
        "tts-{}-{}.mp3",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    std::fs::write(&path, &bytes)?;
    Ok(format!(
        "语音已生成：{}\nprovider=openai\nmodel=gpt-4o-mini-tts",
        path.display()
    ))
}

fn resolve_openai_tts(ctx: &ToolContext<'_>) -> anyhow::Result<(String, String)> {
    if let Some(creds) = ctx.image_gen_targets.openai() {
        let base = if creds.base_url.trim().is_empty() {
            "https://api.openai.com/v1".to_string()
        } else {
            openai_compatible_base(&creds.base_url)
        };
        return Ok((creds.api_key.clone(), base));
    }
    if !ctx.chat_api_key.is_empty() && ctx.chat_provider == "openai" {
        let base = if ctx.chat_base_url.trim().is_empty() {
            "https://api.openai.com/v1".to_string()
        } else {
            openai_compatible_base(&ctx.chat_base_url)
        };
        return Ok((ctx.chat_api_key.clone(), base));
    }
    let env_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if env_key.is_empty() {
        anyhow::bail!("无可用 OpenAI API Key");
    }
    Ok((env_key, "https://api.openai.com/v1".to_string()))
}
