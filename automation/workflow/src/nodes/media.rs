use anyhow::{bail, Result};
use async_trait::async_trait;

use providers::ProviderConfig;

use crate::engine::executor::{NodeExecutor, NodeResult};
use crate::engine::variables::VariableContext;
use crate::model::WorkflowNode;

fn build_media_config(node: &WorkflowNode) -> Result<(String, ProviderConfig)> {
    let provider_id = node.config.get("provider_id").and_then(|v| v.as_str()).unwrap_or("openai");
    let model = node.config.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let auth = providers::AuthKind::for_provider(provider_id);
    let api_key = if auth == providers::AuthKind::None {
        String::new()
    } else {
        providers::profile::read_env_api_key(provider_id).ok_or_else(|| {
            anyhow::anyhow!("未找到 {} 的 API Key 环境变量", provider_id)
        })?
    };
    let base_url = Some(providers::profile::default_base_for(provider_id).to_string())
        .filter(|s| !s.is_empty());
    Ok((provider_id.to_string(), ProviderConfig {
        api_key,
        base_url,
        model: model.to_string(),
        temperature: 0.7,
        max_tokens: 4096,
        thinking_enabled: false,
        reasoning_effort: String::new(),
        additional_params: serde_json::json!({}),
        previous_interaction_id: None,
    }))
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

        let (provider_id, config) = build_media_config(node)?;
        let images = providers::dispatch::generate_image(&provider_id, &prompt, &config).await?;
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
            "provider": provider_id,
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
        if prompt.trim().is_empty() {
            bail!("视频生成节点的提示词为空");
        }

        let (provider_id, config) = build_media_config(node)?;
        let result = providers::dispatch::generate_video(&provider_id, &prompt, &config).await?;

        Ok(NodeResult::Success(serde_json::json!({
            "mime_type": result.mime_type,
            "width": result.width,
            "height": result.height,
            "size_bytes": result.data.len(),
            "provider": provider_id,
        })))
    }
}

// ── Music Generation ────────────────────────────────────────────────

pub struct MusicGenExec;

#[async_trait]
impl NodeExecutor for MusicGenExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let prompt_tpl = node.config.get("prompt_template").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        if prompt.trim().is_empty() {
            bail!("音乐生成节点的提示词为空");
        }

        let (provider_id, config) = build_media_config(node)?;
        let result = providers::dispatch::generate_music(&provider_id, &prompt, &config).await?;

        let artifacts_dir = home::default_memory_dir().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir)?;
        let ext = if result.mime_type.contains("wav") { "wav" } else { "mp3" };
        let filename = format!("music_{}.{}", chrono::Local::now().format("%Y%m%d_%H%M%S"), ext);
        let path = artifacts_dir.join(&filename);
        std::fs::write(&path, &result.data)?;

        Ok(NodeResult::Success(serde_json::json!({
            "path": path.to_string_lossy(),
            "mime_type": result.mime_type,
            "duration_ms": result.duration_ms,
            "provider": provider_id,
        })))
    }
}

// ── Text to Speech ──────────────────────────────────────────────────

pub struct TtsExec;

#[async_trait]
impl NodeExecutor for TtsExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let text_tpl = node.config.get("text_template").and_then(|v| v.as_str()).unwrap_or("");
        let text = ctx.interpolate(text_tpl);
        if text.trim().is_empty() {
            bail!("TTS 节点的文本为空");
        }

        let (provider_id, config) = build_media_config(node)?;
        let result = providers::dispatch::text_to_speech(&provider_id, &text, &config).await?;

        let artifacts_dir = home::default_memory_dir().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir)?;
        let ext = if result.mime_type.contains("wav") { "wav" } else { "mp3" };
        let filename = format!("tts_{}.{}", chrono::Local::now().format("%Y%m%d_%H%M%S"), ext);
        let path = artifacts_dir.join(&filename);
        std::fs::write(&path, &result.data)?;

        Ok(NodeResult::Success(serde_json::json!({
            "path": path.to_string_lossy(),
            "mime_type": result.mime_type,
            "duration_ms": result.duration_ms,
            "provider": provider_id,
        })))
    }
}

// ── Subtitle Generation (ASR) ───────────────────────────────────────

pub struct SubtitleGenExec;

#[async_trait]
impl NodeExecutor for SubtitleGenExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let src = node.config.get("audio_source").and_then(|v| v.as_str()).unwrap_or("");
        let resolved = ctx.interpolate(src);
        bail!("ASR/字幕生成功能待接入 — 各厂商 ASR API 暂无统一路由。音频源: {}", &resolved[..resolved.len().min(100)])
    }
}
