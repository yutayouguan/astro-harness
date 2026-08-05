//! OpenAI — 基础 OpenAI 兼容 + Bearer auth 辅助 + 连通性探测。

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client;
use serde_json::{json, Value};

use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::traits::{
    Capable, Capabilities, EmbeddingModel, FromClient, ImageGenModel, ModelBase, Nothing,
    ProviderClient, ProviderExt, TTSModel,
};
use crate::types::media::{Embedding, GeneratedAudio, GeneratedImage, ImageGenConfig, TTSConfig};

/// Bearer token 认证 header（OpenAI 及大多数兼容厂商共用）。
pub fn bearer_headers(api_key: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if !api_key.is_empty() {
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
            headers.insert(AUTHORIZATION, val);
        }
    }
    headers
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAI;

impl ProviderExt for OpenAI {
    const NAME: &'static str = "openai";
    const BASE_URL: &'static str = "https://api.openai.com/v1";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        bearer_headers(key)
    }
}

impl OpenAICompatible for OpenAI {
    const STREAM_USAGE: bool = true;

    fn finalize_body(&self, body: &mut Value) {
        if let Some(tc) = body.get("thinking_config").cloned() {
            body.as_object_mut().unwrap().remove("thinking_config");
            let enabled = tc.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
            if enabled {
                let effort = tc
                    .get("effort")
                    .and_then(|v| v.as_str())
                    .unwrap_or("high");
                let mapped = match effort {
                    "max" | "xhigh" => "high",
                    "" => "high",
                    other => other,
                };
                body["reasoning_effort"] = serde_json::json!(mapped);
            }
        }
    }
}

impl Capabilities for OpenAI {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Capable<OpenAIEmbeddingModel>;
    type ImageGen = Capable<OpenAIImageModel>;
    type VideoGen = Nothing;
    type TTS = Capable<OpenAITTSModel>;
    type MusicGen = Nothing;
}

// ─── Embedding Model ────────────────────────────────────

#[derive(Clone)]
pub struct OpenAIEmbeddingModel(ModelBase);

impl FromClient<OpenAI> for OpenAIEmbeddingModel {
    fn from_client(client: &ProviderClient<OpenAI>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl EmbeddingModel for OpenAIEmbeddingModel {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Embedding>> {
        let cfg = self.0.to_provider_config();
        let vectors =
            crate::openai::embeddings_http::openai_batch_embed(self.0.http(), texts, self.0.model(), &cfg)
                .await?;
        Ok(vectors.into_iter().map(|v| Embedding { values: v }).collect())
    }
}

// ─── Image Generation Model ────────────────────────────

#[derive(Clone)]
pub struct OpenAIImageModel(ModelBase);

impl FromClient<OpenAI> for OpenAIImageModel {
    fn from_client(client: &ProviderClient<OpenAI>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl ImageGenModel for OpenAIImageModel {
    async fn generate(
        &self,
        prompt: &str,
        _config: &ImageGenConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        let cfg = self.0.to_provider_config();
        crate::openai::image_http::openai_generate_image(self.0.http(), prompt, &cfg).await
    }
}

// ─── TTS Model ──────────────────────────────────────────

#[derive(Clone)]
pub struct OpenAITTSModel(ModelBase);

impl FromClient<OpenAI> for OpenAITTSModel {
    fn from_client(client: &ProviderClient<OpenAI>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl TTSModel for OpenAITTSModel {
    async fn synthesize(
        &self,
        text: &str,
        tts_config: &TTSConfig,
    ) -> anyhow::Result<GeneratedAudio> {
        let cfg = self.0.to_provider_config();
        let voice = if tts_config.voice_id.is_empty() {
            "alloy".to_string()
        } else {
            tts_config.voice_id.clone()
        };
        let req = crate::openai::tts_http::OpenAiTtsRequest {
            model: self.0.model().to_string(),
            input: text.to_string(),
            voice,
            speed: if tts_config.speed > 0.0 { tts_config.speed } else { 1.0 },
            ..Default::default()
        };
        let result = crate::openai::tts_http::openai_tts(self.0.http(), &cfg, &req).await?;
        Ok(GeneratedAudio {
            data: result.audio_bytes,
            mime_type: result.mime_type,
            duration_ms: 0,
        })
    }
}

// ─── 连通性探测 ─────────────────────────────────────────

/// 从各厂商错误响应中提取人类可读消息。
pub(crate) fn extract_error_message(v: &Value) -> &str {
    v.pointer("/error/message")
        .and_then(|m| m.as_str())
        .or_else(|| v.pointer("/base_resp/status_msg").and_then(|m| m.as_str()))
        .or_else(|| v.get("error").and_then(|e| e.as_str()))
        .or_else(|| v.get("message").and_then(|m| m.as_str()))
        .filter(|s| !s.is_empty())
        .unwrap_or("未知错误")
}

/// OpenAI 兼容 chat/completions 最小探测。
pub async fn probe_openai_compat(
    client: &Client,
    endpoint: &str,
    model: &str,
    api_key: &str,
) -> Result<String, String> {
    let url = format!(
        "{}/chat/completions",
        crate::compat::openai_compatible_base(endpoint)
    );
    let body = json!({
        "model": model,
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "ping"}]
    });
    let mut req = client.post(&url).json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("连接模型服务失败: {e}"))?;
    let status = resp.status();
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("失败 ({status}): {}", extract_error_message(&json)));
    }
    Ok("调用成功".to_string())
}

/// Responses API 最小探测（`POST {base}/responses`）。
pub async fn probe_openai_responses(
    client: &Client,
    endpoint: &str,
    model: &str,
    api_key: &str,
) -> Result<String, String> {
    let url = format!(
        "{}/responses",
        crate::compat::openai_compatible_base(endpoint)
    );
    let body = json!({
        "model": model,
        "input": "ping",
        "max_output_tokens": 1,
    });
    let mut req = client.post(&url).json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("连接 Responses API 失败: {e}"))?;
    let status = resp.status();
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("失败 ({status}): {}", extract_error_message(&json)));
    }
    Ok("调用成功".to_string())
}
