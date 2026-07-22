use anyhow::{bail, Result};
use async_trait::async_trait;

use providers::ProviderClient;
use providers::image_http::openai_generate_image;
use providers::trait_::ProviderConfig;

use crate::engine::executor::{NodeExecutor, NodeResult};
use crate::engine::variables::VariableContext;
use crate::model::WorkflowNode;

fn build_media_config(node: &WorkflowNode) -> Result<(String, ProviderConfig, reqwest::Client)> {
    let provider_id = node.config.get("provider_id").and_then(|v| v.as_str()).unwrap_or("openai");
    let model = node.config.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let client = ProviderClient::from_env(provider_id)?;
    let config = ProviderConfig {
        api_key: client.api_key.clone(),
        base_url: client.base_url.clone(),
        model: model.to_string(),
        temperature: 0.7,
        max_tokens: 4096,
        thinking_enabled: false,
        reasoning_effort: String::new(),
        additional_params: serde_json::json!({}),
        previous_interaction_id: None,
    };
    Ok((provider_id.to_string(), config, client.http.clone()))
}

// ── Image Generation ────────────────────────────────────────────────

pub struct ImageGenExec;

#[async_trait]
impl NodeExecutor for ImageGenExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let prompt_tpl = node.config.get("prompt_template").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        if prompt.trim().is_empty() {
            bail!("图片生成节点的提示词(prompt_template)为空");
        }

        let (_provider_id, config, http) = build_media_config(node)?;
        let images = openai_generate_image(&http, &prompt, &config).await?;

        if images.is_empty() {
            bail!("图片生成未返回结果");
        }

        let artifacts_dir = home::default_memory_dir().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir)?;

        let mut paths = Vec::new();
        for (i, img) in images.iter().enumerate() {
            let ext = if img.mime_type.contains("png") { "png" } else { "jpg" };
            let filename = format!("img_{}_{}.{}", chrono::Local::now().format("%Y%m%d_%H%M%S"), i, ext);
            let path = artifacts_dir.join(&filename);
            std::fs::write(&path, &img.data)?;
            paths.push(path.to_string_lossy().to_string());
        }

        Ok(NodeResult::Success(serde_json::json!({
            "images": paths,
            "count": images.len(),
        })))
    }
}

// ── Video Generation ────────────────────────────────────────────────

pub struct VideoGenExec;

#[async_trait]
impl NodeExecutor for VideoGenExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let prompt_tpl = node.config.get("prompt_template").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        bail!("视频生成功能正在开发中 — 待接入 provider 视频 API。提示词: {}", &prompt[..prompt.len().min(100)])
    }
}

// ── Music Generation ────────────────────────────────────────────────

pub struct MusicGenExec;

#[async_trait]
impl NodeExecutor for MusicGenExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let prompt_tpl = node.config.get("prompt_template").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        bail!("音乐生成功能正在开发中 — 待接入 provider 音乐 API。提示词: {}", &prompt[..prompt.len().min(100)])
    }
}

// ── Text to Speech ──────────────────────────────────────────────────

pub struct TtsExec;

#[async_trait]
impl NodeExecutor for TtsExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let text_tpl = node.config.get("text_template").and_then(|v| v.as_str()).unwrap_or("");
        let text = ctx.interpolate(text_tpl);
        bail!("TTS 功能正在开发中 — 待接入 provider TTS API。文本: {}", &text[..text.len().min(100)])
    }
}

// ── Subtitle Generation ─────────────────────────────────────────────

pub struct SubtitleGenExec;

#[async_trait]
impl NodeExecutor for SubtitleGenExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let src = node.config.get("audio_source").and_then(|v| v.as_str()).unwrap_or("");
        let resolved = ctx.interpolate(src);
        bail!("字幕生成功能正在开发中 — 待接入 Whisper API。音频源: {}", &resolved[..resolved.len().min(100)])
    }
}
