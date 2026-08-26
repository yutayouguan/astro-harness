//! Gemini Interactions API（`POST /v1beta/interactions`）、音频理解。
//!
//! 承载 TTS、出图（Nano Banana）、视觉（describe/detect/segment）与视频理解。
//! 与 OpenAI 兼容端点无关，请勿经 `…/v1beta/openai`。

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::defaults::DEFAULT_MODEL;
use super::veo_http::{google_native_base, pcm_to_wav};
use crate::types::media::GeneratedImage;
use crate::types::ProviderConfig;

const API_REVISION: &str = "2026-05-20";
const DEFAULT_SAMPLE_RATE: u32 = 24_000;
const MAX_TTS_RETRIES: u32 = 2;

fn trim_slash(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

/// Interactions 端点 URL。
pub fn interactions_url(config: &ProviderConfig) -> String {
    let base = trim_slash(&google_native_base(config));
    if base.contains("/v1beta") {
        format!("{base}/interactions")
    } else {
        format!("{base}/v1beta/interactions")
    }
}

/// 统一 Interactions POST：`content-type` + `x-goog-api-key` + `Api-Revision`。
fn interactions_post(client: &Client, url: &str, api_key: &str) -> reqwest::RequestBuilder {
    client
        .post(url)
        .header("content-type", "application/json")
        .header("x-goog-api-key", api_key)
        .header("Api-Revision", API_REVISION)
}

fn error_message(v: &Value) -> String {
    crate::http_stream::json_error_message(v, "Google interactions 失败").to_string()
}

// ── TTS ──────────────────────────────────────────────────────────────────────

/// 单条 speech_config 项（单说话人或多说话人之一）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InteractionSpeechConfig {
    /// 多说话人时与转写中的角色名一致；单说话人可省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    pub voice: String,
}

/// TTS Interactions 请求。
#[derive(Debug, Clone)]
pub struct InteractionTtsRequest {
    pub model: String,
    pub input: String,
    pub speech_config: Vec<InteractionSpeechConfig>,
    pub stream: bool,
}

/// TTS Interactions 结果。
#[derive(Debug, Clone)]
pub struct InteractionTtsResult {
    pub wav_bytes: Vec<u8>,
    pub interaction_id: String,
}

/// 拼装 TTS Interactions 请求 body。
pub fn build_interaction_tts_body(req: &InteractionTtsRequest) -> Value {
    let speech_config: Vec<Value> = req
        .speech_config
        .iter()
        .map(|c| {
            if let Some(speaker) = c
                .speaker
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                json!({ "speaker": speaker, "voice": c.voice })
            } else {
                json!({ "voice": c.voice })
            }
        })
        .collect();

    let mut body = json!({
        "model": req.model,
        "input": req.input,
        "response_format": { "type": "audio" },
        "generation_config": {
            "speech_config": speech_config
        }
    });
    if req.stream {
        body["stream"] = json!(true);
    }
    body
}

/// 解析非流式 Interactions TTS 响应。
///
/// 优先读 `output_audio`（便利属性），回退到 `steps` 数组中的 audio 块。
pub fn parse_interaction_tts_response(v: &Value) -> Result<InteractionTtsResult> {
    let interaction_id = v
        .get("id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    let mut audio_b64: Option<&str> = v
        .pointer("/output_audio/data")
        .or_else(|| v.pointer("/outputAudio/data"))
        .and_then(|d| d.as_str());

    let mut rate_hint: Option<&str> = v
        .pointer("/output_audio/mime_type")
        .or_else(|| v.pointer("/output_audio/mimeType"))
        .or_else(|| v.pointer("/outputAudio/mime_type"))
        .and_then(|m| m.as_str());

    if audio_b64.is_none() {
        if let Some(steps) = v.get("steps").and_then(|s| s.as_array()) {
            for step in steps {
                if step_type(step) != "model_output" {
                    continue;
                }
                let Some(content) = step.get("content").and_then(|c| c.as_array()) else {
                    continue;
                };
                for block in content {
                    if block_type(block) == "audio" {
                        if let Some(d) = block.get("data").and_then(|x| x.as_str()) {
                            audio_b64 = Some(d);
                        }
                        if let Some(m) = block
                            .get("mime_type")
                            .or_else(|| block.get("mimeType"))
                            .and_then(|x| x.as_str())
                        {
                            rate_hint = Some(m);
                        }
                        break;
                    }
                }
                if audio_b64.is_some() {
                    break;
                }
            }
        }
    }

    let b64 = audio_b64.ok_or_else(|| anyhow!("Google interactions TTS 响应无音频数据"))?;

    let pcm = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("解码 Interactions TTS PCM 失败")?;
    if pcm.is_empty() {
        anyhow::bail!("Google interactions TTS 返回空音频");
    }

    let rate = mime_sample_rate(rate_hint.unwrap_or("")).unwrap_or(DEFAULT_SAMPLE_RATE);

    Ok(InteractionTtsResult {
        wav_bytes: pcm_to_wav(&pcm, rate, 1, 16),
        interaction_id,
    })
}

/// 从流式事件 JSON 数组聚合 PCM，并尽量提取 `interaction_id`。
///
/// 支持：
/// - 完整事件对象：`{ "event_type": "step.delta", "delta": { "type": "audio", "data": "…" } }`
/// - 已剥离的 `delta` 对象：`{ "type": "audio", "data": "…" }`
/// - `interaction.created` / `/interaction/id` / 顶层 `id` 用于 interaction_id
pub fn parse_tts_stream_events(events: &[Value]) -> Result<InteractionTtsResult> {
    let mut pcm = Vec::new();
    let mut interaction_id = String::new();

    for ev in events {
        if interaction_id.is_empty() {
            if let Some(id) = ev.get("id").and_then(|x| x.as_str()) {
                interaction_id = id.to_string();
            } else if let Some(id) = ev
                .pointer("/interaction/id")
                .and_then(|x| x.as_str())
                .or_else(|| ev.get("interaction_id").and_then(|x| x.as_str()))
            {
                interaction_id = id.to_string();
            }
        }

        let event_type = ev
            .get("event_type")
            .or_else(|| ev.get("eventType"))
            .and_then(|t| t.as_str())
            .unwrap_or("");

        let delta = if event_type == "step.delta" || event_type.is_empty() {
            ev.get("delta").unwrap_or(ev)
        } else {
            continue;
        };

        let dtype = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if dtype != "audio" {
            continue;
        }
        let Some(b64) = delta.get("data").and_then(|d| d.as_str()) else {
            continue;
        };
        let chunk = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .context("解码流式 TTS PCM chunk 失败")?;
        pcm.extend_from_slice(&chunk);
    }

    if pcm.is_empty() {
        anyhow::bail!("Google interactions TTS 流式响应无音频 chunk");
    }

    Ok(InteractionTtsResult {
        wav_bytes: pcm_to_wav(&pcm, DEFAULT_SAMPLE_RATE, 1, 16),
        interaction_id,
    })
}

fn mime_sample_rate(mime: &str) -> Option<u32> {
    for part in mime.split(';') {
        let p = part.trim();
        if let Some(rest) = p.strip_prefix("rate=") {
            if let Ok(n) = rest.parse::<u32>() {
                return Some(n);
            }
        }
    }
    None
}

/// 调用 Gemini Interactions TTS；非流式遇 HTTP 500 最多重试 2 次。
pub async fn google_interactions_tts(
    client: &Client,
    config: &ProviderConfig,
    req: &InteractionTtsRequest,
) -> Result<InteractionTtsResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    if req.input.trim().is_empty() {
        anyhow::bail!("TTS input 为空");
    }
    if req.speech_config.is_empty() {
        anyhow::bail!("TTS speech_config 为空");
    }
    if req.speech_config.len() > 2 {
        anyhow::bail!("TTS 最多支持 2 个说话人");
    }

    let url = interactions_url(config);
    let body = build_interaction_tts_body(req);

    if req.stream {
        return tts_stream_once(client, config, &url, &body).await;
    }

    let mut last_err = None;
    for attempt in 0..=MAX_TTS_RETRIES {
        match tts_unary_once(client, config, &url, &body).await {
            Ok(r) => return Ok(r),
            Err(e) => {
                let msg = e.to_string();
                let is_500 = msg.contains("HTTP 500");
                last_err = Some(e);
                if is_500 && attempt < MAX_TTS_RETRIES {
                    continue;
                }
                break;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("Google interactions TTS 失败")))
}

async fn tts_unary_once(
    client: &Client,
    config: &ProviderConfig,
    url: &str,
    body: &Value,
) -> Result<InteractionTtsResult> {
    let response = interactions_post(client, url, config.api_key.trim())
        .json(body)
        .send()
        .await
        .with_context(|| format!("连接 Google interactions TTS 失败: {url}"))?;
    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Google interactions TTS 响应失败")?;
    if !status.is_success() {
        anyhow::bail!("Google interactions HTTP {status}: {}", error_message(&v));
    }
    parse_interaction_tts_response(&v)
}

async fn tts_stream_once(
    client: &Client,
    config: &ProviderConfig,
    url: &str,
    body: &Value,
) -> Result<InteractionTtsResult> {
    let response = interactions_post(client, url, config.api_key.trim())
        .json(body)
        .send()
        .await
        .with_context(|| format!("连接 Google interactions TTS 流式失败: {url}"))?;
    let status = response.status();
    if !status.is_success() {
        let v: Value = response.json().await.unwrap_or(json!({}));
        anyhow::bail!("Google interactions HTTP {status}: {}", error_message(&v));
    }

    let mut buf = String::new();
    let mut events = Vec::new();
    let mut byte_stream = response.bytes_stream();

    while let Some(item) = byte_stream.next().await {
        let bytes = item.context("读取 Interactions TTS 流失败")?;
        buf.push_str(&String::from_utf8_lossy(&bytes));
        drain_sse_events(&mut buf, &mut events)?;
    }
    if !buf.trim().is_empty() {
        // 尾部无换行的最后一行
        let line = buf.trim().to_string();
        if let Some(ev) = parse_sse_data_line(&line)? {
            events.push(ev);
        }
    }

    parse_tts_stream_events(&events)
}

fn drain_sse_events(buf: &mut String, events: &mut Vec<Value>) -> Result<()> {
    while let Some(nl) = buf.find('\n') {
        let line = buf[..nl].trim_end_matches('\r').to_string();
        *buf = buf[nl + 1..].to_string();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(':') {
            continue;
        }
        if let Some(ev) = parse_sse_data_line(trimmed)? {
            events.push(ev);
        }
    }
    Ok(())
}

fn parse_sse_data_line(line: &str) -> Result<Option<Value>> {
    let data = if let Some(rest) = line.strip_prefix("data:") {
        rest.trim()
    } else if line.starts_with('{') {
        // 部分端点直接推送 NDJSON
        line
    } else {
        return Ok(None);
    };
    if data.is_empty() || data == "[DONE]" {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(data)
        .with_context(|| format!("解析 Interactions TTS SSE JSON 失败: {data}"))?;
    Ok(Some(v))
}

/// 将用户文本与可选风格拼成防误朗读的 Interactions `input`。
pub fn build_tts_input(text: &str, style: Option<&str>) -> String {
    let mut out = String::from(
        "Synthesize speech for the transcript below. Follow the director notes; do not read the notes aloud.\n\n",
    );
    if let Some(style) = style.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("### DIRECTOR'S NOTES\n");
        out.push_str(style);
        out.push_str("\n\n");
    }
    out.push_str("#### TRANSCRIPT\n");
    out.push_str(text.trim());
    out
}

// ── Image ────────────────────────────────────────────────────────────────────

pub struct InteractionImagePart {
    pub data: Vec<u8>,
    pub mime_type: String,
}

pub enum InteractionVideoInput {
    Uri { uri: String, mime_type: String },
    Bytes { data: Vec<u8>, mime_type: String },
}

#[derive(Default)]
pub struct InteractionImageRequest {
    pub prompt: String,
    pub aspect_ratio: Option<String>,
    pub image_size: Option<String>,
    pub mime_type: Option<String>,
    pub reference_images: Vec<InteractionImagePart>,
    pub previous_interaction_id: Option<String>,
    pub google_search: bool,
    pub image_search: bool,
    pub thinking_level: Option<String>,
    pub video: Option<InteractionVideoInput>,
}

pub struct InteractionImageResult {
    pub image: GeneratedImage,
    pub interaction_id: String,
    pub output_text: Option<String>,
    pub search_suggestions: Option<String>,
}

pub fn build_interaction_image_body(model: &str, req: &InteractionImageRequest) -> Value {
    let mut input = vec![json!({ "type": "text", "text": req.prompt })];
    for img in &req.reference_images {
        input.push(json!({
            "type": "image",
            "data": base64::engine::general_purpose::STANDARD.encode(&img.data),
            "mime_type": img.mime_type,
        }));
    }
    if let Some(v) = &req.video {
        match v {
            InteractionVideoInput::Uri { uri, mime_type } => {
                input.push(json!({ "type": "video", "uri": uri, "mime_type": mime_type }));
            }
            InteractionVideoInput::Bytes { data, mime_type } => {
                input.push(json!({
                    "type": "video",
                    "data": base64::engine::general_purpose::STANDARD.encode(data),
                    "mime_type": mime_type,
                }));
            }
        }
    }

    let mime = req
        .mime_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("image/png");
    let mut response_format = json!({ "type": "image", "mime_type": mime });
    if let Some(ar) = req.aspect_ratio.as_deref().filter(|s| !s.is_empty()) {
        response_format["aspect_ratio"] = json!(ar);
    }
    if let Some(sz) = req.image_size.as_deref().filter(|s| !s.is_empty()) {
        response_format["image_size"] = json!(sz);
    }

    let mut body = json!({
        "model": model,
        "input": input,
        "response_format": response_format,
    });
    if let Some(id) = req
        .previous_interaction_id
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        body["previous_interaction_id"] = json!(id);
    }
    if req.google_search {
        let mut tool = json!({ "type": "google_search" });
        if req.image_search {
            tool["search_types"] = json!(["web_search", "image_search"]);
        }
        body["tools"] = json!([tool]);
    }
    if let Some(level) = req.thinking_level.as_deref().filter(|s| !s.is_empty()) {
        body["generation_config"] = json!({ "thinking_level": level });
    }
    body
}

fn step_type(step: &Value) -> &str {
    step.get("type").and_then(|t| t.as_str()).unwrap_or("")
}

fn block_type(block: &Value) -> &str {
    block.get("type").and_then(|t| t.as_str()).unwrap_or("")
}

pub fn parse_interaction_image_response(v: &Value) -> Result<InteractionImageResult> {
    let interaction_id = v
        .get("id")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Interactions 响应缺少 id"))?
        .to_string();

    let steps = v
        .get("steps")
        .and_then(|s| s.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);

    let mut texts: Vec<String> = Vec::new();
    let mut last_image: Option<GeneratedImage> = None;
    let mut search_suggestions: Option<String> = None;

    for step in steps {
        let ty = step_type(step);
        if ty == "google_search_result" {
            if let Some(s) = step
                .get("search_suggestions")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
            {
                search_suggestions = Some(s.to_string());
            }
            continue;
        }
        if ty != "model_output" {
            continue; // 跳过 thought 等
        }
        let content = step
            .get("content")
            .and_then(|c| c.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        for block in content {
            match block_type(block) {
                "text" => {
                    if let Some(t) = block.get("text").and_then(|x| x.as_str()) {
                        if !t.is_empty() {
                            texts.push(t.to_string());
                        }
                    }
                }
                "image" => {
                    let b64 = block
                        .get("data")
                        .and_then(|d| d.as_str())
                        .ok_or_else(|| anyhow!("image block 缺少 data"))?;
                    let mime = block
                        .get("mime_type")
                        .or_else(|| block.get("mimeType"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("image/png")
                        .to_string();
                    let data = base64::engine::general_purpose::STANDARD
                        .decode(b64)
                        .context("解码 Interactions 图片 base64 失败")?;
                    last_image = Some(GeneratedImage {
                        data,
                        mime_type: mime,
                    });
                }
                _ => {}
            }
        }
    }

    let image = last_image.ok_or_else(|| anyhow!("未返回图片数据（可能被安全策略拦截）"))?;
    Ok(InteractionImageResult {
        image,
        interaction_id,
        output_text: if texts.is_empty() {
            None
        } else {
            Some(texts.join("\n"))
        },
        search_suggestions,
    })
}

pub async fn google_interactions_image(
    client: &Client,
    config: &ProviderConfig,
    req: &InteractionImageRequest,
) -> Result<InteractionImageResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let model = if config.model.trim().is_empty() {
        "gemini-3.1-flash-image"
    } else {
        config.model.trim()
    };
    let url = interactions_url(config);
    let body = build_interaction_image_body(model, req);

    let response = interactions_post(client, &url, config.api_key.trim())
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google Interactions 出图失败: {url}"))?;

    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Google Interactions 出图响应失败")?;
    if !status.is_success() {
        anyhow::bail!("Google interactions HTTP {status}: {}", error_message(&v));
    }
    parse_interaction_image_response(&v)
}

/// 视觉模式（定义在共享层，此处再导出以保持 `interactions_http::VisionMode` 路径稳定）。
pub use crate::shared::vision::VisionMode;

#[derive(Debug, Clone)]
pub enum VisionImagePart {
    Inline { mime_type: String, data_b64: String },
    Uri { mime_type: String, uri: String },
}

pub fn default_vision_prompt(mode: VisionMode) -> &'static str {
    match mode {
        VisionMode::Describe => "请描述这张图片",
        VisionMode::Detect => {
            "Detect all prominent items in the image. The box_2d should be [ymin, xmin, ymax, xmax] normalized to 0-1000. Return JSON with boxes array of {box_2d, label}."
        }
        VisionMode::Segment => {
            "Give segmentation masks for the prominent items. Each entry: box_2d [ymin,xmin,ymax,xmax] 0-1000, mask as [x,y] polygon 0-1000, and label."
        }
    }
}

pub fn vision_boxes_json_schema(include_mask: bool) -> Value {
    let mut item_props = json!({
        "box_2d": { "type": "array", "items": { "type": "integer" } },
        "label": { "type": "string" }
    });
    let mut required = vec!["box_2d", "label"];
    if include_mask {
        item_props["mask"] = json!({
            "type": "array",
            "items": { "type": "array", "items": { "type": "integer" } }
        });
        required.push("mask");
    }
    json!({
        "type": "object",
        "properties": {
            "boxes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": item_props,
                    "required": required
                }
            }
        },
        "required": ["boxes"]
    })
}

pub fn build_interaction_vision_body(
    model: &str,
    prompt: &str,
    images: &[VisionImagePart],
    mode: VisionMode,
) -> Value {
    let mut input = vec![json!({"type": "text", "text": prompt})];
    for img in images {
        match img {
            VisionImagePart::Inline {
                mime_type,
                data_b64,
            } => {
                input.push(json!({
                    "type": "image",
                    "data": data_b64,
                    "mime_type": mime_type
                }));
            }
            VisionImagePart::Uri { mime_type, uri } => {
                input.push(json!({
                    "type": "image",
                    "uri": uri,
                    "mime_type": mime_type
                }));
            }
        }
    }
    let mut body = json!({ "model": model, "input": input });
    match mode {
        VisionMode::Describe => {}
        VisionMode::Detect => {
            body["response_format"] = json!({
                "type": "text",
                "mime_type": "application/json",
                "schema": vision_boxes_json_schema(false)
            });
        }
        VisionMode::Segment => {
            body["response_format"] = json!({
                "type": "text",
                "mime_type": "application/json",
                "schema": vision_boxes_json_schema(true)
            });
            // `minimal` 只被部分 Gemini 版本接受（3.7 起会 400），`low` 是通用最低档。
            body["generation_config"] = json!({ "thinking_level": "low" });
        }
    }
    body
}

pub fn parse_interaction_vision_text(v: &Value) -> Result<String> {
    if let Some(s) = v.get("output_text").and_then(|t| t.as_str()) {
        let t = s.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    let mut parts = Vec::new();
    if let Some(steps) = v.get("steps").and_then(|s| s.as_array()) {
        for step in steps {
            if step.get("type").and_then(|t| t.as_str()) != Some("model_output") {
                continue;
            }
            if let Some(content) = step.get("content").and_then(|c| c.as_array()) {
                for item in content {
                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                            parts.push(t.to_string());
                        }
                    }
                }
            }
        }
    }
    let joined = parts.join("");
    if joined.trim().is_empty() {
        anyhow::bail!("Interactions 视觉响应无文本");
    }
    Ok(joined)
}

pub async fn google_interactions_vision(
    client: &Client,
    prompt: &str,
    images: &[VisionImagePart],
    mode: VisionMode,
    config: &ProviderConfig,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    if images.is_empty() {
        anyhow::bail!("vision 至少需要一张图片");
    }
    let model = if config.model.trim().is_empty() {
        DEFAULT_MODEL
    } else {
        config.model.trim()
    };
    let url = interactions_url(config);
    let body = build_interaction_vision_body(model, prompt, images, mode);
    let response = interactions_post(client, &url, config.api_key.trim())
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google Interactions 视觉失败: {url}"))?;
    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Interactions 视觉 JSON 失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Interactions 视觉请求失败");
        anyhow::bail!("Google Interactions HTTP {status}: {msg}");
    }
    parse_interaction_vision_text(&v)
}

// ── Video understanding ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoUnderstandMode {
    Qa,
    Summarize,
    Timeline,
}

impl VideoUnderstandMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Qa => "qa",
            Self::Summarize => "summarize",
            Self::Timeline => "timeline",
        }
    }
}

impl std::str::FromStr for VideoUnderstandMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "qa" | "" => Ok(Self::Qa),
            "summarize" => Ok(Self::Summarize),
            "timeline" => Ok(Self::Timeline),
            other => anyhow::bail!("未知 video_understand mode: {other}"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum VideoInputPart {
    Inline {
        mime_type: String,
        data_b64: String,
    },
    Uri {
        mime_type: Option<String>,
        uri: String,
    },
}

pub fn default_video_understand_prompt(mode: VideoUnderstandMode) -> &'static str {
    match mode {
        VideoUnderstandMode::Qa => {
            "请概括该视频，并用要点回答关于其内容的问题。引用时刻请用 MM:SS。"
        }
        VideoUnderstandMode::Summarize => {
            "请用 3–5 句话总结该视频，并分别说明关键的视觉与音频要点。引用时刻请用 MM:SS。"
        }
        VideoUnderstandMode::Timeline => {
            "提取该视频的关键事件时间线。每个事件包含 timestamp(MM:SS)、description、modality(visual|audio|both)。"
        }
    }
}

pub fn video_timeline_json_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "events": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "timestamp": { "type": "string" },
                        "description": { "type": "string" },
                        "modality": { "type": "string", "enum": ["visual", "audio", "both"] }
                    },
                    "required": ["timestamp", "description", "modality"]
                }
            }
        },
        "required": ["events"]
    })
}

pub fn build_interaction_video_body(
    model: &str,
    prompt: &str,
    video: &VideoInputPart,
    mode: VideoUnderstandMode,
) -> Value {
    let video_part = match video {
        VideoInputPart::Inline {
            mime_type,
            data_b64,
        } => json!({
            "type": "video",
            "data": data_b64,
            "mime_type": mime_type,
        }),
        VideoInputPart::Uri { mime_type, uri } => {
            let mut p = json!({ "type": "video", "uri": uri });
            if let Some(m) = mime_type.as_ref().filter(|s| !s.is_empty()) {
                p["mime_type"] = json!(m);
            }
            p
        }
    };
    let mut body = json!({
        "model": model,
        "input": [video_part, { "type": "text", "text": prompt }],
    });
    if mode == VideoUnderstandMode::Timeline {
        // 与 TTS(`type: audio`)/出图(`type: image`) 的扁平 response_format 风格保持一致，
        // 并匹配 Gemini 官方结构化输出格式：{ type: "text", mime_type: "application/json", schema }
        body["response_format"] = json!({
            "type": "text",
            "mime_type": "application/json",
            "schema": video_timeline_json_schema()
        });
    }
    body
}

pub fn parse_interaction_video_text(v: &Value) -> Result<String> {
    if let Some(t) = v.get("output_text").and_then(|x| x.as_str()) {
        let t = t.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    let mut parts = Vec::new();
    if let Some(steps) = v.get("steps").and_then(|s| s.as_array()) {
        for step in steps {
            if step_type(step) != "model_output" {
                continue; // 跳过 thought 等，与 image/vision 解析一致
            }
            let content = step
                .get("content")
                .or_else(|| step.pointer("/model_output/content"));
            if let Some(arr) = content.and_then(|c| c.as_array()) {
                for item in arr {
                    if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                        parts.push(t.to_string());
                    }
                }
            } else if let Some(t) = step.pointer("/content/0/text").and_then(|t| t.as_str()) {
                parts.push(t.to_string());
            }
        }
    }
    let joined = parts.join("\n").trim().to_string();
    if joined.is_empty() {
        anyhow::bail!("Interactions 视频理解响应无文本");
    }
    Ok(joined)
}

pub fn try_parse_timeline_events(text: &str) -> Result<Value> {
    let trimmed = text.trim();
    // 允许 markdown fence
    let json_str = if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest.trim_start_matches("json").trim_start_matches('\n');
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        trimmed
    };
    let v: Value = serde_json::from_str(json_str).context("timeline JSON 解析失败")?;
    if !v.get("events").map(|e| e.is_array()).unwrap_or(false) {
        anyhow::bail!("timeline JSON 缺少 events 数组");
    }
    Ok(v)
}

pub async fn google_interactions_video(
    client: &Client,
    config: &ProviderConfig,
    model: &str,
    prompt: &str,
    video: &VideoInputPart,
    mode: VideoUnderstandMode,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let url = interactions_url(config);
    let body = build_interaction_video_body(model, prompt, video, mode);
    let response = interactions_post(client, &url, config.api_key.trim())
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google interactions video 失败: {url}"))?;
    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Google interactions video 响应失败")?;
    if !status.is_success() {
        anyhow::bail!("Google interactions HTTP {status}: {}", error_message(&v));
    }
    parse_interaction_video_text(&v)
}

// ── Audio understand ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioUnderstandMode {
    Describe,
    Transcribe,
}

impl AudioUnderstandMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Describe => "describe",
            Self::Transcribe => "transcribe",
        }
    }

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "describe" => Ok(Self::Describe),
            "transcribe" => Ok(Self::Transcribe),
            other => {
                anyhow::bail!("audio_understand mode 无效: {other}（支持 describe|transcribe）")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMediaKind {
    Audio,
    Video,
}

impl AudioMediaKind {
    fn as_type_str(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }
}

#[derive(Debug, Clone)]
pub enum AudioMediaPart {
    Inline {
        media_type: AudioMediaKind,
        mime_type: String,
        data_b64: String,
    },
    Uri {
        media_type: AudioMediaKind,
        mime_type: String,
        uri: String,
    },
}

pub fn default_audio_understand_prompt(mode: AudioUnderstandMode) -> &'static str {
    match mode {
        AudioUnderstandMode::Describe => "请描述这段音频",
        AudioUnderstandMode::Transcribe => {
            "Process the audio and generate a detailed transcription. \
Identify distinct speakers (Speaker 1, Speaker 2, …). \
Provide timestamps MM:SS. Detect language; if not English provide English translation in translation. \
emotion must be one of happy, sad, angry, neutral. Include a brief summary."
        }
    }
}

pub fn audio_transcribe_json_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string" },
            "segments": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "speaker": { "type": "string" },
                        "timestamp": { "type": "string" },
                        "content": { "type": "string" },
                        "language": { "type": "string" },
                        "translation": { "type": "string" },
                        "emotion": {
                            "type": "string",
                            "enum": ["happy", "sad", "angry", "neutral"]
                        }
                    },
                    "required": ["speaker", "timestamp", "content", "emotion"]
                }
            }
        },
        "required": ["summary", "segments"]
    })
}

pub fn build_interaction_audio_body(
    model: &str,
    prompt: &str,
    media: &AudioMediaPart,
    mode: AudioUnderstandMode,
) -> Value {
    let media_json = match media {
        AudioMediaPart::Inline {
            media_type,
            mime_type,
            data_b64,
        } => json!({
            "type": media_type.as_type_str(),
            "data": data_b64,
            "mime_type": mime_type,
        }),
        AudioMediaPart::Uri {
            media_type,
            mime_type,
            uri,
        } => json!({
            "type": media_type.as_type_str(),
            "uri": uri,
            "mime_type": mime_type,
        }),
    };
    let mut body = json!({
        "model": model,
        "input": [
            { "type": "text", "text": prompt },
            media_json
        ]
    });
    if mode == AudioUnderstandMode::Transcribe {
        body["response_format"] = json!({
            "type": "text",
            "mime_type": "application/json",
            "schema": audio_transcribe_json_schema()
        });
    }
    body
}

pub fn parse_interaction_audio_text(v: &Value) -> Result<String> {
    if let Some(t) = v.get("output_text").and_then(|x| x.as_str()) {
        let t = t.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    let mut out = String::new();
    if let Some(steps) = v.get("steps").and_then(|s| s.as_array()) {
        for step in steps {
            if step.get("type").and_then(|t| t.as_str()) != Some("model_output") {
                continue;
            }
            if let Some(content) = step.get("content").and_then(|c| c.as_array()) {
                for part in content {
                    if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(t) = part.get("text").and_then(|x| x.as_str()) {
                            out.push_str(t);
                        }
                    }
                }
            }
        }
    }
    let out = out.trim().to_string();
    if out.is_empty() {
        anyhow::bail!("Google interactions 音频响应无文本");
    }
    Ok(out)
}

pub async fn google_interactions_audio(
    client: &Client,
    prompt: &str,
    media: &AudioMediaPart,
    mode: AudioUnderstandMode,
    config: &ProviderConfig,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let model = if config.model.trim().is_empty() {
        DEFAULT_MODEL.to_string()
    } else {
        config.model.trim().to_string()
    };
    let url = interactions_url(config);
    let body = build_interaction_audio_body(&model, prompt, media, mode);
    let response = interactions_post(client, &url, config.api_key.trim())
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google interactions 音频理解失败: {url}"))?;
    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Google interactions 音频理解响应失败")?;
    if !status.is_success() {
        anyhow::bail!("Google interactions HTTP {status}: {}", error_message(&v));
    }
    parse_interaction_audio_text(&v)
}

// ── Music (Lyria 3) ──────────────────────────────────────────────────────────

pub fn default_music_model_clip() -> &'static str {
    "lyria-3-clip-preview"
}

pub fn default_music_model_pro() -> &'static str {
    "lyria-3-pro-preview"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicAudioFormat {
    Mp3,
    Wav,
}

#[derive(Debug, Clone)]
pub struct MusicImagePart {
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone)]
pub struct InteractionMusicRequest {
    pub model: String,
    pub prompt: String,
    pub images: Vec<MusicImagePart>,
    pub format: MusicAudioFormat,
}

#[derive(Debug, Clone)]
pub struct InteractionMusicResult {
    pub audio_bytes: Vec<u8>,
    pub mime_type: String,
    pub lyrics_text: Option<String>,
    pub interaction_id: String,
}

pub fn resolve_lyria_model_id(alias: &str) -> Result<String> {
    let a = alias.trim();
    Ok(match a {
        "" | "clip" => default_music_model_clip().to_string(),
        "pro" => default_music_model_pro().to_string(),
        "lyria-3-clip-preview" | "lyria-3-pro-preview" => a.to_string(),
        _ => anyhow::bail!("无效 music model: {a}（clip | pro | lyria-3-*-preview）"),
    })
}

pub fn music_extension(mime: &str, format: MusicAudioFormat) -> &'static str {
    let m = mime.to_ascii_lowercase();
    if m.contains("wav") {
        return "wav";
    }
    if m.contains("mpeg") || m.contains("mp3") {
        return "mp3";
    }
    match format {
        MusicAudioFormat::Wav => "wav",
        MusicAudioFormat::Mp3 => "mp3",
    }
}

pub fn build_interaction_music_body(req: &InteractionMusicRequest) -> Value {
    let input = if req.images.is_empty() {
        json!(req.prompt)
    } else {
        let mut parts = vec![json!({ "type": "text", "text": req.prompt })];
        for img in &req.images {
            parts.push(json!({
                "type": "image",
                "mime_type": img.mime_type,
                "data": img.data_base64,
            }));
        }
        json!(parts)
    };

    let mut response_format = json!({ "type": "audio" });
    if req.format == MusicAudioFormat::Wav {
        response_format["mime_type"] = json!("audio/wav");
    }

    json!({
        "model": req.model,
        "input": input,
        "response_format": response_format,
    })
}

pub fn parse_interaction_music_response(v: &Value) -> Result<InteractionMusicResult> {
    let interaction_id = v
        .get("id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    let mut mime_type = String::new();
    let mut lyrics_parts: Vec<String> = Vec::new();
    let mut audio_b64: Option<&str> = None;

    if let Some(oa) = v.get("output_audio").or_else(|| v.get("outputAudio")) {
        if let Some(d) = oa.get("data").and_then(|x| x.as_str()) {
            audio_b64 = Some(d);
        }
        if let Some(m) = oa
            .get("mime_type")
            .or_else(|| oa.get("mimeType"))
            .and_then(|x| x.as_str())
        {
            mime_type = m.to_string();
        }
    }
    if let Some(t) = v
        .get("output_text")
        .or_else(|| v.get("outputText"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        lyrics_parts.push(t.to_string());
    }

    if audio_b64.is_none() {
        if let Some(steps) = v.get("steps").and_then(|s| s.as_array()) {
            for step in steps {
                let st = step.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if st != "model_output" && !st.is_empty() {
                    continue;
                }
                let Some(content) = step.get("content").and_then(|c| c.as_array()) else {
                    continue;
                };
                for block in content {
                    let bt = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match bt {
                        "audio" => {
                            if let Some(d) = block.get("data").and_then(|x| x.as_str()) {
                                audio_b64 = Some(d);
                            }
                            if let Some(m) = block
                                .get("mime_type")
                                .or_else(|| block.get("mimeType"))
                                .and_then(|x| x.as_str())
                            {
                                mime_type = m.to_string();
                            }
                        }
                        "text" => {
                            if let Some(t) = block.get("text").and_then(|x| x.as_str()) {
                                let t = t.trim();
                                if !t.is_empty() {
                                    lyrics_parts.push(t.to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let b64 = audio_b64.ok_or_else(|| anyhow!("Google interactions music 响应无音频数据"))?;
    let audio_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("解码 Interactions music 音频失败")?;
    if audio_bytes.is_empty() {
        anyhow::bail!("Google interactions music 返回空音频");
    }

    let lyrics_text = if lyrics_parts.is_empty() {
        None
    } else {
        Some(lyrics_parts.join("\n"))
    };

    // filtered_prompt：若有则拼进错误旁注不在此处；成功路径可忽略
    let _ = v.get("filtered_prompt");

    Ok(InteractionMusicResult {
        audio_bytes,
        mime_type,
        lyrics_text,
        interaction_id,
    })
}

pub async fn google_interactions_music(
    client: &Client,
    config: &ProviderConfig,
    req: &InteractionMusicRequest,
) -> Result<InteractionMusicResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    if req.prompt.trim().is_empty() {
        anyhow::bail!("music prompt 为空");
    }
    if req.images.len() > 10 {
        anyhow::bail!("music 参考图最多 10 张");
    }

    let url = interactions_url(config);
    let body = build_interaction_music_body(req);
    let response = interactions_post(client, &url, config.api_key.trim())
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google interactions music 失败: {url}"))?;
    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Google interactions music 响应失败")?;
    if !status.is_success() {
        let mut msg = error_message(&v);
        if let Some(fp) = v
            .get("filtered_prompt")
            .and_then(|x| x.as_str())
            .or_else(|| v.pointer("/filtered_prompt/text").and_then(|x| x.as_str()))
        {
            msg = format!("{msg}; filtered_prompt={fp}");
        }
        anyhow::bail!("Google interactions music HTTP {status}: {msg}");
    }
    parse_interaction_music_response(&v)
}

// ---------------------------------------------------------------------------
// Embedding API
// ---------------------------------------------------------------------------

/// 批量生成文本 embedding 向量。
///
/// 调用 `POST /v1beta/models/{model}:batchEmbedContents`。
pub async fn google_batch_embed(
    client: &Client,
    texts: &[String],
    model: &str,
    config: &ProviderConfig,
) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let base = google_native_base(config);
    let model_path = if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{model}")
    };
    let url = format!("{base}/v1beta/{model_path}:batchEmbedContents");

    let requests: Vec<Value> = texts
        .iter()
        .map(|t| {
            json!({
                "model": model_path,
                "content": { "parts": [{ "text": t }] }
            })
        })
        .collect();
    let body = json!({ "requests": requests });

    let resp = client
        .post(&url)
        .header("x-goog-api-key", &config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .context("连接 Google Embedding 失败")?;
    let status = resp.status();
    let v: Value = resp
        .json()
        .await
        .context("解析 Google Embedding 响应失败")?;
    if !status.is_success() {
        anyhow::bail!("Google Embedding HTTP {status}: {}", error_message(&v));
    }

    let embeddings = v
        .get("embeddings")
        .and_then(|e| e.as_array())
        .context("响应缺少 embeddings 数组")?;
    embeddings
        .iter()
        .map(|emb| {
            let vals = emb
                .get("values")
                .and_then(|v| v.as_array())
                .context("embedding 缺少 values")?;
            vals.iter()
                .map(|x| {
                    x.as_f64()
                        .map(|f| f as f32)
                        .context("embedding value 非数值")
                })
                .collect::<Result<Vec<f32>>>()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Context Caching API
// ---------------------------------------------------------------------------

/// 创建缓存内容，返回 `cachedContents/{name}` 资源名。
///
/// 调用 `POST /v1beta/cachedContents`。
pub async fn google_create_cached_content(
    client: &Client,
    model: &str,
    system_instruction: Option<&str>,
    contents: &[Value],
    ttl_secs: u64,
    config: &ProviderConfig,
) -> Result<String> {
    let base = google_native_base(config);
    let url = format!("{base}/v1beta/cachedContents");

    let model_path = if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{model}")
    };
    let mut body = json!({
        "model": model_path,
        "contents": contents,
        "ttl": format!("{ttl_secs}s"),
    });
    if let Some(sys) = system_instruction {
        body["system_instruction"] = json!({
            "parts": [{ "text": sys }]
        });
    }

    let resp = client
        .post(&url)
        .header("x-goog-api-key", &config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .context("连接 Google CachedContents 失败")?;
    let status = resp.status();
    let v: Value = resp
        .json()
        .await
        .context("解析 Google CachedContents 响应失败")?;
    if !status.is_success() {
        anyhow::bail!("Google CachedContents HTTP {status}: {}", error_message(&v));
    }

    v.get("name")
        .and_then(|n| n.as_str())
        .map(str::to_string)
        .context("CachedContents 响应缺少 name 字段")
}

/// 删除缓存内容。
///
/// 调用 `DELETE /v1beta/{name}`，其中 `name` 形如 `cachedContents/xxx`。
pub async fn google_delete_cached_content(
    client: &Client,
    name: &str,
    config: &ProviderConfig,
) -> Result<()> {
    let base = google_native_base(config);
    let url = format!("{base}/v1beta/{name}");
    let resp = client
        .delete(&url)
        .header("x-goog-api-key", &config.api_key)
        .send()
        .await
        .context("连接 Google CachedContents DELETE 失败")?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Google CachedContents DELETE 失败: {body}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── TTS tests ──

    #[test]
    fn build_body_single_speaker() {
        let req = InteractionTtsRequest {
            model: "gemini-3.1-flash-tts-preview".into(),
            input: "Hello".into(),
            speech_config: vec![InteractionSpeechConfig {
                speaker: None,
                voice: "Kore".into(),
            }],
            stream: false,
        };
        let body = build_interaction_tts_body(&req);
        assert_eq!(body["model"], "gemini-3.1-flash-tts-preview");
        assert_eq!(body["response_format"]["type"], "audio");
        assert_eq!(
            body["generation_config"]["speech_config"][0]["voice"],
            "Kore"
        );
        assert!(body["generation_config"]["speech_config"][0]
            .get("speaker")
            .is_none());
        assert!(body.get("stream").is_none());
    }

    #[test]
    fn build_body_multi_speaker_and_stream() {
        let req = InteractionTtsRequest {
            model: "gemini-3.1-flash-tts-preview".into(),
            input: "Joe: hi\nJane: hey".into(),
            speech_config: vec![
                InteractionSpeechConfig {
                    speaker: Some("Joe".into()),
                    voice: "Kore".into(),
                },
                InteractionSpeechConfig {
                    speaker: Some("Jane".into()),
                    voice: "Puck".into(),
                },
            ],
            stream: true,
        };
        let body = build_interaction_tts_body(&req);
        assert_eq!(body["stream"], true);
        let sc = body["generation_config"]["speech_config"]
            .as_array()
            .unwrap();
        assert_eq!(sc.len(), 2);
        assert_eq!(sc[0]["speaker"], "Joe");
        assert_eq!(sc[1]["voice"], "Puck");
    }

    #[test]
    fn parse_unary_output_audio() {
        // 极短 PCM：4 字节
        let pcm = [0u8, 1, 2, 3];
        let b64 = base64::engine::general_purpose::STANDARD.encode(pcm);
        let v = json!({
            "id": "ix-tts-1",
            "output_audio": {
                "data": b64,
                "mime_type": "audio/L16;codec=pcm;rate=24000"
            }
        });
        let r = parse_interaction_tts_response(&v).unwrap();
        assert_eq!(r.interaction_id, "ix-tts-1");
        assert_eq!(&r.wav_bytes[0..4], b"RIFF");
        assert_eq!(r.wav_bytes.len(), 44 + 4);
    }

    #[test]
    fn parse_stream_chunks() {
        let c1 = base64::engine::general_purpose::STANDARD.encode([1u8, 2]);
        let c2 = base64::engine::general_purpose::STANDARD.encode([3u8, 4]);
        let events = vec![
            json!({ "event_type": "interaction.created", "interaction": { "id": "ix-s" }, "id": "ix-s" }),
            json!({
                "event_type": "step.delta",
                "delta": { "type": "audio", "data": c1 }
            }),
            json!({
                "event_type": "step.delta",
                "delta": { "type": "audio", "data": c2 }
            }),
            json!({ "event_type": "step.delta", "delta": { "type": "text", "data": "x" } }),
        ];
        let r = parse_tts_stream_events(&events).unwrap();
        assert_eq!(r.interaction_id, "ix-s");
        assert_eq!(r.wav_bytes.len(), 44 + 4);
        assert_eq!(&r.wav_bytes[44..], &[1, 2, 3, 4]);
    }

    #[test]
    fn build_tts_input_with_style() {
        let s = build_tts_input("Hello world", Some("Speak cheerfully"));
        assert!(s.contains("DIRECTOR'S NOTES"));
        assert!(s.contains("Speak cheerfully"));
        assert!(s.contains("#### TRANSCRIPT\nHello world"));
        assert!(s.contains("do not read the notes aloud"));
    }

    #[test]
    fn build_tts_input_without_style() {
        let s = build_tts_input("Hi", None);
        assert!(!s.contains("DIRECTOR'S NOTES"));
        assert!(s.contains("#### TRANSCRIPT\nHi"));
    }

    #[test]
    fn interactions_url_strips_openai_suffix() {
        let cfg = ProviderConfig {
            api_key: "k".into(),
            base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai".into()),
            model: String::new(),
            ..ProviderConfig::default()
        };
        assert_eq!(
            interactions_url(&cfg),
            "https://generativelanguage.googleapis.com/v1beta/interactions"
        );
    }

    // ── Image tests ──

    #[test]
    fn build_body_includes_aspect_size_and_search() {
        let req = InteractionImageRequest {
            prompt: "a cat".into(),
            aspect_ratio: Some("16:9".into()),
            image_size: Some("2K".into()),
            mime_type: None,
            reference_images: vec![],
            previous_interaction_id: None,
            google_search: true,
            image_search: true,
            thinking_level: Some("high".into()),
            video: None,
        };
        let body = build_interaction_image_body("gemini-3.1-flash-image", &req);
        assert_eq!(body["model"], "gemini-3.1-flash-image");
        assert_eq!(body["response_format"]["type"], "image");
        assert_eq!(body["response_format"]["aspect_ratio"], "16:9");
        assert_eq!(body["response_format"]["image_size"], "2K");
        assert_eq!(body["generation_config"]["thinking_level"], "high");
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools[0]["type"], "google_search");
        let st = tools[0]["search_types"].as_array().unwrap();
        assert!(st.iter().any(|x| x == "web_search"));
        assert!(st.iter().any(|x| x == "image_search"));
        let input = body["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "text");
        assert_eq!(input[0]["text"], "a cat");
    }

    #[test]
    fn parse_takes_last_model_output_image_ignores_thought() {
        let v = json!({
            "id": "ix-123",
            "steps": [
                {
                    "type": "thought",
                    "summary": [{ "type": "image", "data": "AAAA", "mime_type": "image/png" }]
                },
                {
                    "type": "model_output",
                    "content": [
                        { "type": "text", "text": "hello" },
                        { "type": "image", "data": "Zmlyc3Q=", "mime_type": "image/png" },
                        { "type": "image", "data": "c2Vjb25k", "mime_type": "image/jpeg" }
                    ]
                },
                {
                    "type": "google_search_result",
                    "search_suggestions": "<div>suggest</div>"
                }
            ]
        });
        let r = parse_interaction_image_response(&v).unwrap();
        assert_eq!(r.interaction_id, "ix-123");
        assert_eq!(r.image.mime_type, "image/jpeg");
        assert_eq!(r.image.data, b"second");
        assert_eq!(r.output_text.as_deref(), Some("hello"));
        assert_eq!(r.search_suggestions.as_deref(), Some("<div>suggest</div>"));
    }

    #[test]
    fn parse_accepts_mime_type_camel_case() {
        let v = json!({
            "id": "ix-camel",
            "steps": [{
                "type": "model_output",
                "content": [
                    { "type": "image", "data": "cG5n", "mimeType": "image/png" }
                ]
            }]
        });
        let r = parse_interaction_image_response(&v).unwrap();
        assert_eq!(r.image.mime_type, "image/png");
        assert_eq!(r.image.data, b"png");
    }

    #[test]
    fn parse_errors_when_no_image() {
        let v = json!({ "id": "ix", "steps": [{ "type": "model_output", "content": [{ "type": "text", "text": "x" }] }] });
        assert!(parse_interaction_image_response(&v).is_err());
    }

    // ── Music tests ──

    #[test]
    fn build_music_body_text_only() {
        let req = InteractionMusicRequest {
            model: "lyria-3-clip-preview".into(),
            prompt: "minimal techno".into(),
            images: vec![],
            format: MusicAudioFormat::Mp3,
        };
        let body = build_interaction_music_body(&req);
        assert_eq!(body["model"], "lyria-3-clip-preview");
        assert_eq!(body["input"], "minimal techno");
        assert_eq!(body["response_format"]["type"], "audio");
        assert!(body.get("generation_config").is_none());
    }

    #[test]
    fn build_music_body_with_images_and_wav() {
        let req = InteractionMusicRequest {
            model: "lyria-3-pro-preview".into(),
            prompt: "ambient from image".into(),
            images: vec![MusicImagePart {
                mime_type: "image/jpeg".into(),
                data_base64: "abc".into(),
            }],
            format: MusicAudioFormat::Wav,
        };
        let body = build_interaction_music_body(&req);
        let input = body["input"].as_array().expect("input array");
        assert_eq!(input[0]["type"], "text");
        assert_eq!(input[0]["text"], "ambient from image");
        assert_eq!(input[1]["type"], "image");
        assert_eq!(input[1]["mime_type"], "image/jpeg");
        assert_eq!(input[1]["data"], "abc");
        // Pro+Wav：在 response_format 上附 mime_type 提示（官方文档字段若变更，只改此处与本断言）
        assert_eq!(body["response_format"]["type"], "audio");
        assert_eq!(body["response_format"]["mime_type"], "audio/wav");
    }

    #[test]
    fn parse_music_output_audio_and_text() {
        let v = serde_json::json!({
            "id": "ix-music-1",
            "output_audio": { "data": "Zm9v", "mime_type": "audio/mpeg" },
            "output_text": "[Verse]\nhello"
        });
        let r = parse_interaction_music_response(&v).unwrap();
        assert_eq!(r.interaction_id, "ix-music-1");
        assert_eq!(r.audio_bytes, b"foo");
        assert!(r.mime_type.contains("mpeg"));
        assert_eq!(r.lyrics_text.as_deref(), Some("[Verse]\nhello"));
    }

    #[test]
    fn parse_music_from_steps_fallback() {
        let v = serde_json::json!({
            "id": "ix-2",
            "steps": [{
                "type": "model_output",
                "content": [
                    { "type": "text", "text": "line1" },
                    { "type": "audio", "data": "YmFy", "mime_type": "audio/wav" }
                ]
            }]
        });
        let r = parse_interaction_music_response(&v).unwrap();
        assert_eq!(r.audio_bytes, b"bar");
        assert!(r.mime_type.contains("wav"));
        assert_eq!(r.lyrics_text.as_deref(), Some("line1"));
    }

    #[test]
    fn parse_music_missing_audio_errors() {
        let v = serde_json::json!({ "id": "ix", "output_text": "only text" });
        assert!(parse_interaction_music_response(&v).is_err());
    }

    #[test]
    fn resolve_lyria_model_and_extension() {
        assert_eq!(
            resolve_lyria_model_id("clip").unwrap(),
            "lyria-3-clip-preview"
        );
        assert_eq!(
            resolve_lyria_model_id("pro").unwrap(),
            "lyria-3-pro-preview"
        );
        assert_eq!(
            resolve_lyria_model_id("lyria-3-pro-preview").unwrap(),
            "lyria-3-pro-preview"
        );
        assert!(resolve_lyria_model_id("nope").is_err());
        assert_eq!(music_extension("audio/mpeg", MusicAudioFormat::Mp3), "mp3");
        assert_eq!(music_extension("audio/wav", MusicAudioFormat::Wav), "wav");
        assert_eq!(music_extension("", MusicAudioFormat::Wav), "wav");
        assert_eq!(music_extension("", MusicAudioFormat::Mp3), "mp3");
    }
}

#[cfg(test)]
mod vision_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_describe_body_inline_and_uri() {
        let images = vec![
            VisionImagePart::Inline {
                mime_type: "image/png".into(),
                data_b64: "YWJj".into(),
            },
            VisionImagePart::Uri {
                mime_type: "image/jpeg".into(),
                uri: "https://example.com/a.jpg".into(),
            },
        ];
        let body = build_interaction_vision_body(
            "gemini-3.5-flash",
            "compare",
            &images,
            VisionMode::Describe,
        );
        assert_eq!(body["model"], "gemini-3.5-flash");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "text");
        assert_eq!(input[0]["text"], "compare");
        assert_eq!(input[1]["type"], "image");
        assert_eq!(input[1]["data"], "YWJj");
        assert_eq!(input[1]["mime_type"], "image/png");
        assert_eq!(input[2]["uri"], "https://example.com/a.jpg");
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn build_detect_body_has_schema_without_mask() {
        let body = build_interaction_vision_body(
            "gemini-3.5-flash",
            "detect",
            &[VisionImagePart::Uri {
                mime_type: "image/png".into(),
                uri: "https://x/y.png".into(),
            }],
            VisionMode::Detect,
        );
        let schema = &body["response_format"]["schema"];
        assert_eq!(body["response_format"]["mime_type"], "application/json");
        let props = &schema["properties"]["boxes"]["items"]["properties"];
        assert!(props.get("box_2d").is_some());
        assert!(props.get("label").is_some());
        assert!(props.get("mask").is_none());
        assert!(body.get("generation_config").is_none());
    }

    #[test]
    fn build_segment_body_has_mask_and_lowest_thinking() {
        let body = build_interaction_vision_body(
            "gemini-3.5-flash",
            "seg",
            &[VisionImagePart::Uri {
                mime_type: "image/png".into(),
                uri: "https://x/y.png".into(),
            }],
            VisionMode::Segment,
        );
        let props =
            &body["response_format"]["schema"]["properties"]["boxes"]["items"]["properties"];
        assert!(props.get("mask").is_some());
        assert_eq!(body["generation_config"]["thinking_level"], "low");
    }

    #[test]
    fn parse_prefers_output_text() {
        let v = json!({ "output_text": "caption", "steps": [] });
        assert_eq!(parse_interaction_vision_text(&v).unwrap(), "caption");
    }

    #[test]
    fn parse_falls_back_to_steps_model_output() {
        let v = json!({
            "steps": [
                {
                    "type": "model_output",
                    "content": [
                        { "type": "text", "text": "hello " },
                        { "type": "text", "text": "world" }
                    ]
                }
            ]
        });
        assert_eq!(parse_interaction_vision_text(&v).unwrap(), "hello world");
    }

    #[test]
    fn parse_errors_when_empty() {
        let v = json!({ "steps": [] });
        assert!(parse_interaction_vision_text(&v).is_err());
    }
}

#[cfg(test)]
mod video_understand_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_qa_body_video_before_text() {
        let body = build_interaction_video_body(
            "gemini-3.5-flash",
            "summarize please",
            &VideoInputPart::Uri {
                mime_type: Some("video/mp4".into()),
                uri: "https://www.youtube.com/watch?v=abc".into(),
            },
            VideoUnderstandMode::Qa,
        );
        assert_eq!(body["model"], "gemini-3.5-flash");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["type"], "video");
        assert_eq!(input[0]["uri"], "https://www.youtube.com/watch?v=abc");
        assert_eq!(input[1]["type"], "text");
        assert_eq!(input[1]["text"], "summarize please");
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn build_inline_and_timeline_schema() {
        let body = build_interaction_video_body(
            "gemini-3.5-flash",
            "timeline",
            &VideoInputPart::Inline {
                mime_type: "video/mp4".into(),
                data_b64: "YWJj".into(),
            },
            VideoUnderstandMode::Timeline,
        );
        assert_eq!(body["input"][0]["data"], "YWJj");
        assert_eq!(body["input"][0]["mime_type"], "video/mp4");
        // 与 TTS/出图一致的扁平 response_format 风格，匹配官方结构化输出格式
        assert_eq!(body["response_format"]["type"], "text");
        assert_eq!(body["response_format"]["mime_type"], "application/json");
        let schema = body.pointer("/response_format/schema").expect("schema");
        assert!(schema.pointer("/properties/events").is_some());
    }

    #[test]
    fn parse_prefers_output_text() {
        let v = json!({ "output_text": "ok", "steps": [] });
        assert_eq!(parse_interaction_video_text(&v).unwrap(), "ok");
    }

    #[test]
    fn parse_falls_back_to_steps() {
        let v = json!({
            "steps": [{
                "type": "model_output",
                "content": [{ "type": "text", "text": "from-steps" }]
            }]
        });
        assert_eq!(parse_interaction_video_text(&v).unwrap(), "from-steps");
    }

    #[test]
    fn parse_ignores_thought_steps() {
        let v = json!({
            "steps": [
                {
                    "type": "thought",
                    "content": [{ "type": "text", "text": "secret-thought" }]
                },
                {
                    "type": "model_output",
                    "content": [{ "type": "text", "text": "visible" }]
                }
            ]
        });
        assert_eq!(parse_interaction_video_text(&v).unwrap(), "visible");
    }

    #[test]
    fn try_parse_timeline_events_ok() {
        let text = r#"{"events":[{"timestamp":"00:05","description":"intro","modality":"both"}]}"#;
        let v = try_parse_timeline_events(text).unwrap();
        assert_eq!(v["events"][0]["timestamp"], "00:05");
    }

    #[test]
    fn try_parse_timeline_events_rejects_missing() {
        assert!(try_parse_timeline_events(r#"{"foo":1}"#).is_err());
    }

    #[test]
    fn mode_parse() {
        assert_eq!(
            "timeline".parse::<VideoUnderstandMode>().unwrap(),
            VideoUnderstandMode::Timeline
        );
        assert!("nope".parse::<VideoUnderstandMode>().is_err());
    }
}

#[cfg(test)]
mod audio_understand_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_describe_inline_audio() {
        let media = AudioMediaPart::Inline {
            media_type: AudioMediaKind::Audio,
            mime_type: "audio/mp3".into(),
            data_b64: "YWJj".into(),
        };
        let body = build_interaction_audio_body(
            "gemini-3.5-flash",
            "describe please",
            &media,
            AudioUnderstandMode::Describe,
        );
        assert_eq!(body["model"], "gemini-3.5-flash");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "text");
        assert_eq!(input[0]["text"], "describe please");
        assert_eq!(input[1]["type"], "audio");
        assert_eq!(input[1]["data"], "YWJj");
        assert_eq!(input[1]["mime_type"], "audio/mp3");
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn build_youtube_as_video_uri() {
        let media = AudioMediaPart::Uri {
            media_type: AudioMediaKind::Video,
            mime_type: "video/mp4".into(),
            uri: "https://www.youtube.com/watch?v=ku-N-eS1lgM".into(),
        };
        let body = build_interaction_audio_body(
            "gemini-3.5-flash",
            "transcribe",
            &media,
            AudioUnderstandMode::Transcribe,
        );
        let input = body["input"].as_array().unwrap();
        assert_eq!(input[1]["type"], "video");
        assert_eq!(
            input[1]["uri"],
            "https://www.youtube.com/watch?v=ku-N-eS1lgM"
        );
        assert_eq!(input[1]["mime_type"], "video/mp4");
        assert_eq!(body["response_format"]["type"], "text");
        assert_eq!(body["response_format"]["mime_type"], "application/json");
        let props = &body["response_format"]["schema"]["properties"];
        assert!(props.get("summary").is_some());
        assert!(props["segments"]["items"]["properties"]
            .get("emotion")
            .is_some());
        assert!(props["segments"]["items"]["properties"]
            .get("speaker")
            .is_some());
    }

    #[test]
    fn parse_prefers_output_text() {
        let v = json!({ "output_text": "ok audio", "steps": [] });
        assert_eq!(parse_interaction_audio_text(&v).unwrap(), "ok audio");
    }

    #[test]
    fn parse_falls_back_to_steps() {
        let v = json!({
            "steps": [{
                "type": "model_output",
                "content": [
                    { "type": "text", "text": "hello " },
                    { "type": "text", "text": "audio" }
                ]
            }]
        });
        assert_eq!(parse_interaction_audio_text(&v).unwrap(), "hello audio");
    }

    #[test]
    fn parse_ignores_thought_steps() {
        let v = json!({
            "steps": [
                {
                    "type": "thought",
                    "content": [{ "type": "text", "text": "internal reasoning" }]
                },
                {
                    "type": "model_output",
                    "content": [{ "type": "text", "text": "final answer" }]
                }
            ]
        });
        assert_eq!(parse_interaction_audio_text(&v).unwrap(), "final answer");
    }

    #[test]
    fn mode_parse() {
        assert_eq!(
            AudioUnderstandMode::parse("").unwrap(),
            AudioUnderstandMode::Describe
        );
        assert_eq!(
            AudioUnderstandMode::parse("transcribe").unwrap(),
            AudioUnderstandMode::Transcribe
        );
        assert!(AudioUnderstandMode::parse("detect").is_err());
    }
}
