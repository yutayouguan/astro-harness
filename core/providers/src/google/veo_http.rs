//! Google 原生媒体：Veo 视频、API 根路径、PCM→WAV。
//!
//! 出图/TTS/视觉/音频理解请用 [`super::interactions_http`]。

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::time::sleep;

use super::defaults::DEFAULT_API_HOST;
use crate::types::request::ProviderConfig;

/// 去掉 endpoint 末尾斜杠。
fn trim_slash(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

/// Google 原生 API 根（去掉 `/v1beta/openai`）。
pub fn google_native_base(config: &ProviderConfig) -> String {
    let raw = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_API_HOST);
    let base = trim_slash(raw);
    if base.ends_with("/openai") {
        return base
            .trim_end_matches("/openai")
            .trim_end_matches("/v1beta")
            .to_string();
    }
    base
}
/// 默认视频模型。
pub fn default_video_model() -> &'static str {
    "veo-3.1-generate-preview"
}

/// 默认 Gemini TTS 模型。
pub fn default_tts_model() -> &'static str {
    "gemini-3.1-flash-tts-preview"
}

/// 视频生成结果。
#[derive(Debug, Clone)]
pub struct GeneratedVideo {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub operation_id: String,
    pub video_uri: Option<String>,
}

/// multipart 图片字段（首帧 / 尾帧 / 参考图）。
#[derive(Debug, Clone)]
pub struct VideoImagePart {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub mime: String,
}

/// Google 视频创建的可选参数（文本 + 图片）。
#[derive(Debug, Clone, Default)]
pub struct VideoGenExtras {
    pub aspect_ratio: Option<String>,
    pub duration_seconds: Option<u32>,
    pub resolution: Option<String>,
    pub negative_prompt: Option<String>,
    pub style: Option<String>,
    pub extend_video_id: Option<String>,
    pub extend_video_uri: Option<String>,
    /// 续拍视频字节（本地文件或已下载 URI）；mime 通常 `video/mp4`
    pub extend_video: Option<VideoImagePart>,
    /// `allow_adult` / `allow_all` / `dont_allow`
    pub person_generation: Option<String>,
    pub seed: Option<i64>,
    pub image: Option<VideoImagePart>,
    pub last_frame: Option<VideoImagePart>,
    pub reference_images: Vec<VideoImagePart>,
}

/// Google 原生 API 根，始终以 `…/v1beta` 结尾。
pub fn google_v1beta_root(config: &ProviderConfig) -> String {
    let native = trim_slash(&google_native_base(config));
    if native.ends_with("/v1beta") {
        native
    } else if native.contains("/v1beta/") {
        let idx = native.find("/v1beta").unwrap();
        format!("{}{}", &native[..idx], "/v1beta")
    } else {
        format!("{native}/v1beta")
    }
}

pub fn google_veo_predict_url(config: &ProviderConfig, model: &str) -> String {
    format!(
        "{}/models/{}:predictLongRunning",
        google_v1beta_root(config),
        model.trim()
    )
}

pub fn google_operation_url(config: &ProviderConfig, operation_name: &str) -> String {
    let name = operation_name.trim().trim_start_matches('/');
    format!("{}/{}", google_v1beta_root(config), name)
}

fn inline_data_value(part: &VideoImagePart) -> Value {
    json!({
        "inlineData": {
            "mimeType": part.mime,
            "data": base64::engine::general_purpose::STANDARD.encode(&part.bytes),
        }
    })
}

pub fn build_veo_predict_body(prompt: &str, extras: &VideoGenExtras) -> Value {
    let mut instance = json!({ "prompt": prompt });
    if let Some(img) = &extras.image {
        instance["image"] = inline_data_value(img);
    }
    if let Some(img) = &extras.last_frame {
        instance["lastFrame"] = inline_data_value(img);
    }
    if !extras.reference_images.is_empty() {
        instance["referenceImages"] = Value::Array(
            extras
                .reference_images
                .iter()
                .map(|img| {
                    json!({
                        "image": inline_data_value(img),
                        "referenceType": "asset",
                    })
                })
                .collect(),
        );
    }
    if let Some(vid) = &extras.extend_video {
        instance["video"] = inline_data_value(vid);
    }

    let mut parameters = serde_json::Map::new();
    if let Some(ar) = extras
        .aspect_ratio
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parameters.insert("aspectRatio".into(), json!(ar));
    }
    if let Some(sec) = extras.duration_seconds.filter(|s| *s > 0) {
        parameters.insert("durationSeconds".into(), json!(sec));
    }
    if let Some(res) = extras
        .resolution
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let normalized = if res.eq_ignore_ascii_case("4k") {
            "4k".to_string()
        } else {
            res.to_ascii_lowercase()
        };
        parameters.insert("resolution".into(), json!(normalized));
    }
    if let Some(pg) = extras
        .person_generation
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parameters.insert("personGeneration".into(), json!(pg));
    }
    if let Some(seed) = extras.seed {
        parameters.insert("seed".into(), json!(seed));
    }

    let mut body = json!({ "instances": [instance] });
    if !parameters.is_empty() {
        body["parameters"] = Value::Object(parameters);
    }
    body
}

pub fn extract_veo_operation_name(create_body: &Value) -> Result<String> {
    create_body
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Veo 响应缺少 operation name"))
}

pub fn extract_veo_video_uri(status_body: &Value) -> Result<String> {
    if status_body.get("done") != Some(&json!(true))
        && status_body.pointer("/done").and_then(|d| d.as_bool()) != Some(true)
    {
        anyhow::bail!("Veo operation 尚未完成");
    }
    if let Some(msg) = crate::http_stream::json_error_option(status_body) {
        anyhow::bail!("Veo 生成失败: {msg}");
    }
    status_body
        .pointer("/response/generateVideoResponse/generatedSamples/0/video/uri")
        .and_then(|u| u.as_str())
        .or_else(|| {
            status_body
                .pointer("/response/generate_video_response/generated_samples/0/video/uri")
                .and_then(|u| u.as_str())
        })
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Veo 完成但缺少 video.uri"))
}

async fn google_api_get_bytes(client: &Client, url: &str, api_key: &str) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .header("x-goog-api-key", api_key)
        .send()
        .await
        .with_context(|| format!("GET 失败: {url}"))?;
    let status = response.status();
    if status.is_success() {
        return response
            .bytes()
            .await
            .context("读取响应字节失败")
            .map(|b| b.to_vec());
    }
    let v: Value = response.json().await.unwrap_or(json!({}));
    anyhow::bail!(
        "GET {status}: {}",
        crate::http_stream::json_error_message(&v, "GET 请求失败")
    );
}

/// Google 原生 Veo：`predictLongRunning` 创建、轮询 operation、下载视频字节。
///
/// 若 `extend_video` 为空且 `extend_video_uri` 有值，会先 GET 该 URI（带 API key）
/// 再写入请求体 `video.inlineData`。
pub async fn google_native_generate_video(
    client: &Client,
    prompt: &str,
    config: &ProviderConfig,
    extras: &VideoGenExtras,
    mut on_progress: Option<&mut (dyn FnMut(&str) + Send)>,
) -> Result<GeneratedVideo> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let api_key = config.api_key.trim();
    let model = if config.model.trim().is_empty() {
        default_video_model()
    } else {
        config.model.trim()
    };

    let mut body_extras = extras.clone();
    if body_extras.extend_video.is_none() {
        if let Some(uri) = extras
            .extend_video_uri
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let bytes = google_api_get_bytes(client, uri, api_key)
                .await
                .with_context(|| format!("下载 extend_video_uri 失败: {uri}"))?;
            body_extras.extend_video = Some(VideoImagePart {
                mime: "video/mp4".into(),
                filename: "extend.mp4".into(),
                bytes,
            });
        }
    }

    let predict_url = google_veo_predict_url(config, model);
    let body = build_veo_predict_body(prompt, &body_extras);

    let emit = |cb: &mut Option<&mut (dyn FnMut(&str) + Send)>, msg: &str| {
        if let Some(f) = cb.as_mut() {
            f(msg);
        }
    };

    let create_resp = client
        .post(&predict_url)
        .header("x-goog-api-key", api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("Veo predictLongRunning 失败: {predict_url}"))?;
    let create_status = create_resp.status();
    let create_body: Value = create_resp.json().await.context("解析 Veo 创建响应失败")?;
    if !create_status.is_success() {
        anyhow::bail!(
            "Veo predict HTTP {create_status}: {}",
            crate::http_stream::json_error_message(&create_body, "Veo predictLongRunning 失败")
        );
    }

    let op_name = extract_veo_operation_name(&create_body)?;
    emit(
        &mut on_progress,
        &format!("status=queued operation_name={op_name}"),
    );

    let poll_url = google_operation_url(config, &op_name);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10 * 60);
    let video_uri = loop {
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("Veo 生成超时（operation name={op_name}）");
        }
        let poll = client
            .get(&poll_url)
            .header("x-goog-api-key", api_key)
            .send()
            .await
            .with_context(|| format!("轮询 Veo operation 失败: {poll_url}"))?;
        let status = poll.status();
        let poll_body: Value = poll.json().await.context("解析 Veo 轮询响应失败")?;
        if !status.is_success() {
            anyhow::bail!(
                "Veo operation poll HTTP {status}: {}",
                crate::http_stream::json_error_message(&poll_body, "轮询 Veo operation 失败")
            );
        }
        let done = poll_body.get("done") == Some(&json!(true))
            || poll_body.pointer("/done").and_then(|d| d.as_bool()) == Some(true);
        if done {
            break extract_veo_video_uri(&poll_body)?;
        }
        emit(
            &mut on_progress,
            &format!("status=processing operation_name={op_name}"),
        );
        sleep(Duration::from_secs(10)).await;
    };

    emit(&mut on_progress, "status=downloading");
    let bytes = google_api_get_bytes(client, &video_uri, api_key)
        .await
        .with_context(|| format!("下载 Veo 视频失败: {video_uri}"))?;
    emit(&mut on_progress, "status=completed");

    Ok(GeneratedVideo {
        data: bytes,
        mime_type: "video/mp4".into(),
        operation_id: op_name,
        video_uri: Some(video_uri),
    })
}
/// 将 PCM s16le 封装为 WAV。
pub fn pcm_to_wav(pcm: &[u8], sample_rate: u32, channels: u16, bits_per_sample: u16) -> Vec<u8> {
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_len = pcm.len() as u32;
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_wav_header_size() {
        let wav = pcm_to_wav(&[0u8; 4], 24_000, 1, 16);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 48);
    }

    #[test]
    fn google_v1beta_root_strips_openai_suffix() {
        let cfg = ProviderConfig {
            api_key: "k".into(),
            base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai".into()),
            model: String::new(),
            ..ProviderConfig::default()
        };
        assert_eq!(
            google_v1beta_root(&cfg),
            "https://generativelanguage.googleapis.com/v1beta"
        );
        assert_eq!(
            google_veo_predict_url(&cfg, "veo-3.1-generate-preview"),
            "https://generativelanguage.googleapis.com/v1beta/models/veo-3.1-generate-preview:predictLongRunning"
        );
        assert_eq!(
            google_operation_url(&cfg, "operations/abc123"),
            "https://generativelanguage.googleapis.com/v1beta/operations/abc123"
        );
    }

    #[test]
    fn build_veo_predict_body_text_and_params() {
        let extras = VideoGenExtras {
            aspect_ratio: Some("9:16".into()),
            resolution: Some("4K".into()),
            duration_seconds: Some(8),
            person_generation: Some("allow_adult".into()),
            seed: Some(7),
            ..Default::default()
        };
        let body = build_veo_predict_body("a lion", &extras);
        assert_eq!(body["instances"][0]["prompt"], "a lion");
        assert_eq!(body["parameters"]["aspectRatio"], "9:16");
        assert_eq!(body["parameters"]["resolution"], "4k");
        assert_eq!(body["parameters"]["durationSeconds"], 8);
        assert_eq!(body["parameters"]["personGeneration"], "allow_adult");
        assert_eq!(body["parameters"]["seed"], 7);
        assert!(body["instances"][0].get("image").is_none());
    }

    #[test]
    fn build_veo_predict_body_frames_refs_and_extend() {
        let img = VideoImagePart {
            bytes: b"PNG".to_vec(),
            filename: "a.png".into(),
            mime: "image/png".into(),
        };
        let vid = VideoImagePart {
            bytes: b"MP4".to_vec(),
            filename: "v.mp4".into(),
            mime: "video/mp4".into(),
        };
        let extras = VideoGenExtras {
            image: Some(img.clone()),
            last_frame: Some(img.clone()),
            reference_images: vec![img.clone(), img],
            extend_video: Some(vid),
            ..Default::default()
        };
        let body = build_veo_predict_body("interp", &extras);
        let inst = &body["instances"][0];
        assert!(!inst["image"]["inlineData"]["data"]
            .as_str()
            .unwrap()
            .is_empty());
        assert!(inst["lastFrame"]["inlineData"]["data"].is_string());
        assert_eq!(inst["referenceImages"].as_array().unwrap().len(), 2);
        assert_eq!(inst["referenceImages"][0]["referenceType"], "asset");
        assert!(inst["video"]["inlineData"]["data"].is_string());
        assert_eq!(inst["video"]["inlineData"]["mimeType"], "video/mp4");
    }

    #[test]
    fn extract_veo_video_uri_success_and_error() {
        let ok = serde_json::json!({
            "done": true,
            "response": {
                "generateVideoResponse": {
                    "generatedSamples": [{
                        "video": { "uri": "https://example.com/v.mp4" }
                    }]
                }
            }
        });
        assert_eq!(
            extract_veo_video_uri(&ok).unwrap(),
            "https://example.com/v.mp4"
        );
        let err = serde_json::json!({
            "done": true,
            "error": { "message": "blocked" }
        });
        assert!(extract_veo_video_uri(&err)
            .unwrap_err()
            .to_string()
            .contains("blocked"));
    }
}
