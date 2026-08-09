use anyhow::{bail, Result};
use async_trait::async_trait;

use providers::ProviderConfig;

use crate::engine::executor::{NodeExecutor, NodeResult};
use crate::engine::variables::VariableContext;
use crate::model::WorkflowNode;

/// 从节点 config 中解析字符串数组字段（支持 JSON 数组或逗号分隔字符串）。
fn parse_string_array(
    config: &serde_json::Value,
    key: &str,
    ctx: &VariableContext,
) -> Vec<String> {
    let Some(val) = config.get(key) else { return vec![] };
    if let Some(arr) = val.as_array() {
        arr.iter()
            .filter_map(|v| v.as_str().map(|s| ctx.interpolate(s)))
            .filter(|s| !s.trim().is_empty())
            .collect()
    } else if let Some(s) = val.as_str() {
        let interpolated = ctx.interpolate(s);
        if let Ok(arr) = serde_json::from_str::<Vec<String>>(&interpolated) {
            arr.into_iter().filter(|s| !s.trim().is_empty()).collect()
        } else {
            interpolated
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
    } else {
        vec![]
    }
}

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

        // 从节点 config 读取所有视频生成参数
        let first_frame = node.config.get("first_frame_image")
            .and_then(|v| v.as_str())
            .map(|s| ctx.interpolate(s))
            .filter(|s| !s.trim().is_empty());
        let last_frame = node.config.get("last_frame_image")
            .and_then(|v| v.as_str())
            .map(|s| ctx.interpolate(s))
            .filter(|s| !s.trim().is_empty());

        let ref_images = parse_string_array(&node.config, "reference_images", ctx);
        let ref_videos = parse_string_array(&node.config, "reference_videos", ctx);
        let ref_audios = parse_string_array(&node.config, "reference_audios", ctx);

        let duration = node.config.get("duration_seconds")
            .and_then(|v| v.as_u64())
            .map(|d| d as u32);
        let resolution = node.config.get("resolution")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty());
        let ratio = node.config.get("aspect_ratio")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty());
        let prompt_optimizer = node.config.get("prompt_optimizer")
            .and_then(|v| v.as_bool());
        let enhance_prompt = node.config.get("enhance_prompt")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let options = providers::dispatch::VideoGenOptions {
            prompt,
            first_frame_image: first_frame,
            last_frame_image: last_frame,
            reference_images: ref_images,
            reference_videos: ref_videos,
            reference_audios: ref_audios,
            duration,
            resolution,
            ratio,
            prompt_optimizer,
            enhance_prompt,
        };

        let result = providers::dispatch::generate_video_with_options(
            &provider_id, &options, &config,
        ).await?;

        let artifacts_dir = home::default_memory_dir().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir)?;
        let filename = format!("video_{}.mp4", chrono::Local::now().format("%Y%m%d_%H%M%S"));
        let path = artifacts_dir.join(&filename);
        std::fs::write(&path, &result.data)?;

        Ok(NodeResult::Success(serde_json::json!({
            "path": path.to_string_lossy(),
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

// ── Voice Clone (MiniMax) ──────────────────────────────────────────

pub struct VoiceCloneExec;

#[async_trait]
impl NodeExecutor for VoiceCloneExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let ref_audio = node.config.get("reference_audio").and_then(|v| v.as_str()).unwrap_or("");
        let ref_audio = ctx.interpolate(ref_audio);
        let text_tpl = node.config.get("text_template").and_then(|v| v.as_str()).unwrap_or("");
        let text = ctx.interpolate(text_tpl);
        let speaker_id = node.config.get("speaker_id").and_then(|v| v.as_str()).unwrap_or("");
        let speaker_id = ctx.interpolate(speaker_id);

        if ref_audio.trim().is_empty() && speaker_id.trim().is_empty() {
            bail!("声音克隆节点的参考音频和说话人 ID 均为空，至少提供一个");
        }

        let (provider_id, config) = build_media_config(node)?;
        if provider_id != "minimax" {
            bail!("声音克隆目前仅支持 MiniMax 供应商，当前: {}", provider_id);
        }

        let client = reqwest::Client::new();
        let voice_id = if !speaker_id.trim().is_empty() {
            speaker_id.trim().to_string()
        } else {
            // 1) 上传参考音频
            let audio_data = std::fs::read(&ref_audio)
                .map_err(|e| anyhow::anyhow!("读取参考音频失败 {}: {}", ref_audio, e))?;
            let filename = std::path::Path::new(&ref_audio)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "voice_ref.wav".to_string());

            let file_info = providers::minimax::files_http::minimax_upload_file(
                &client, &config, audio_data, &filename,
                providers::minimax::files_http::FileUploadPurpose::VoiceClone,
            ).await?;

            // 2) 克隆音色
            let clone_voice_id = format!("clone_{}", file_info.file_id);
            let clone_req = providers::minimax::voice_clone_http::VoiceCloneRequest {
                file_id: file_info.file_id,
                voice_id: clone_voice_id.clone(),
                text: if text.trim().is_empty() { None } else { Some(text.clone()) },
                model: Some(config.model.clone()).filter(|s| !s.is_empty()),
                need_noise_reduction: true,
                need_volume_normalization: true,
                ..Default::default()
            };
            providers::minimax::voice_clone_http::minimax_voice_clone(
                &client, &config, &clone_req,
            ).await?;

            clone_voice_id
        };

        // 3) 用克隆的音色合成语音
        if text.trim().is_empty() {
            return Ok(NodeResult::Success(serde_json::json!({
                "voice_id": voice_id,
                "provider": provider_id,
                "note": "音色已克隆，未提供合成文本",
            })));
        }

        let tts_req = providers::minimax::tts_http::MiniMaxTtsRequest {
            model: if config.model.is_empty() { "speech-2.8-hd".to_string() } else { config.model.clone() },
            text: text.clone(),
            voice_setting: providers::minimax::tts_http::VoiceSetting {
                voice_id: voice_id.clone(),
                ..Default::default()
            },
            output_format: "hex".to_string(),
            ..Default::default()
        };
        let tts_result = providers::minimax::tts_http::minimax_tts(
            &client, &config, &tts_req,
        ).await?;

        let artifacts_dir = home::default_memory_dir().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir)?;
        let filename = format!("voice_clone_{}.mp3", chrono::Local::now().format("%Y%m%d_%H%M%S"));
        let path = artifacts_dir.join(&filename);
        std::fs::write(&path, &tts_result.audio_bytes)?;

        Ok(NodeResult::Success(serde_json::json!({
            "path": path.to_string_lossy(),
            "voice_id": voice_id,
            "duration_ms": tts_result.duration_ms,
            "provider": provider_id,
        })))
    }
}

// ── Speech to Text ─────────────────────────────────────────────────

pub struct SpeechToTextExec;

#[async_trait]
impl NodeExecutor for SpeechToTextExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let input = node.config.get("input_path").and_then(|v| v.as_str()).unwrap_or("");
        let input = ctx.interpolate(input);
        let language = node.config.get("language").and_then(|v| v.as_str()).unwrap_or("auto");
        let provider_id = node.config.get("provider_id").and_then(|v| v.as_str()).unwrap_or("");

        if input.trim().is_empty() {
            bail!("语音识别节点的输入路径为空");
        }

        Ok(NodeResult::Success(serde_json::json!({
            "type": "speech_to_text",
            "input_path": input,
            "language": language,
            "provider_id": provider_id,
            "note": "语音识别待接入 Whisper/各厂商 ASR API"
        })))
    }
}

// ── Image Edit ─────────────────────────────────────────────────────

pub struct ImageEditExec;

#[async_trait]
impl NodeExecutor for ImageEditExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let op = node.config.get("operation").and_then(|v| v.as_str()).unwrap_or("inpaint");
        let input = node.config.get("input_image").and_then(|v| v.as_str()).unwrap_or("");
        let input = ctx.interpolate(input);
        let prompt_tpl = node.config.get("prompt_template").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        let provider_id = node.config.get("provider_id").and_then(|v| v.as_str()).unwrap_or("");

        if input.trim().is_empty() {
            bail!("图片编辑节点的输入图片为空");
        }

        Ok(NodeResult::Success(serde_json::json!({
            "type": "image_edit",
            "operation": op,
            "input_image": input,
            "prompt": prompt,
            "provider_id": provider_id,
            "note": "图片编辑待接入 Image Edit API"
        })))
    }
}

// ── Translation ────────────────────────────────────────────────────

pub struct TranslationExec;

#[async_trait]
impl NodeExecutor for TranslationExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let text_tpl = node.config.get("text_template").and_then(|v| v.as_str()).unwrap_or("");
        let text = ctx.interpolate(text_tpl);
        let source = node.config.get("source_lang").and_then(|v| v.as_str()).unwrap_or("auto");
        let target = node.config.get("target_lang").and_then(|v| v.as_str()).unwrap_or("en");

        if text.trim().is_empty() {
            bail!("翻译节点的输入文本为空");
        }

        // 通过 LLM 实现翻译
        let system = format!(
            "你是一个专业翻译。将用户输入从{}翻译为{}。只输出译文，不要添加解释。",
            if source == "auto" { "源语言（自动检测）" } else { source },
            target,
        );
        let (pid, config) = build_media_config(node)?;
        let ai_nodes = crate::nodes::ai::one_shot_llm(&pid, &config, &system, &text).await?;

        Ok(NodeResult::Success(serde_json::json!({
            "translation": ai_nodes.trim(),
            "source_lang": source,
            "target_lang": target,
        })))
    }
}
