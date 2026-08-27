//! Google Gemini Interactions API — 原生 CompletionModel 实现。

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use crate::traits::{
    Capabilities, Capable, CompletionModel, EmbeddingModel, FromClient, ImageGenModel, ModelBase,
    MusicGenModel, ProviderClient, ProviderExt, TTSModel, VideoGenModel,
};
use crate::types::media::{
    Embedding, GeneratedAudio, GeneratedImage, GeneratedVideo, ImageGenConfig, MusicGenConfig,
    TTSConfig, VideoGenConfig,
};
use crate::types::{CompletionRequest, CompletionStream};

const API_REVISION: &str = "2026-05-20";

// ─── Provider Extension ─────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub struct Google;

impl ProviderExt for Google {
    const NAME: &'static str = "google";
    const BASE_URL: &'static str = "https://generativelanguage.googleapis.com";

    fn auth_headers(&self, key: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Ok(v) = HeaderValue::from_str(key) {
            h.insert("x-goog-api-key", v);
        }
        if let Ok(v) = HeaderValue::from_str(API_REVISION) {
            h.insert("Api-Revision", v);
        }
        h
    }
}

impl Capabilities for Google {
    type Chat = Capable<InteractionsCompletionModel>;
    type Embedding = Capable<GeminiEmbeddingModel>;
    type ImageGen = Capable<GeminiImageModel>;
    type VideoGen = Capable<VeoVideoModel>;
    type TTS = Capable<GeminiTTSModel>;
    type MusicGen = Capable<LyriaMusicModel>;
}

// ─── Completion Model ────────────────────────────────────

pub struct InteractionsCompletionModel {
    http: HttpClient,
    base_url: String,
    api_key: String,
    model: String,
}

impl Clone for InteractionsCompletionModel {
    fn clone(&self) -> Self {
        Self {
            http: self.http.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            model: self.model.clone(),
        }
    }
}

impl FromClient<Google> for InteractionsCompletionModel {
    fn from_client(client: &ProviderClient<Google>, model: &str) -> Self {
        Self {
            http: client.http.clone(),
            base_url: client.base_url.clone(),
            api_key: client.api_key.clone(),
            model: model.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl CompletionModel for InteractionsCompletionModel {
    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream> {
        let base = self.base_url.trim_end_matches('/');
        let url = if base.contains("/v1beta") {
            format!("{base}/interactions")
        } else {
            format!("{base}/v1beta/interactions")
        };

        let model = if request.model.is_empty() {
            &self.model
        } else {
            &request.model
        };
        let previous = request
            .previous_interaction_id
            .as_deref()
            .filter(|prev| !prev.is_empty());
        let converted = to_interactions_input(&request.messages, previous.is_some());

        let mut body = json!({
            "model": model,
            "input": converted.steps,
            "stream": true,
        });

        if converted.continues_previous {
            if let Some(prev) = previous {
                body["previous_interaction_id"] = json!(prev);
            }
        }
        let system = converted.system;
        if let Some(sys) = system {
            body["system_instruction"] = json!(sys);
        }

        // Tools
        if !request.tools.is_empty() {
            let tools: Vec<Value> = request.tools.iter().map(|t| {
                json!({"type": "function", "name": t.name, "description": t.description, "parameters": t.parameters})
            }).collect();
            body["tools"] = Value::Array(tools);
        }

        // generation_config
        let mut gen = serde_json::Map::new();
        if let Some(temp) = request.temperature {
            gen.insert("temperature".into(), json!(temp));
        }
        if let Some(max) = request.max_tokens {
            if max > 0 {
                gen.insert("max_output_tokens".into(), json!(max));
            }
        }
        gen.insert(
            "thinking_level".into(),
            json!(thinking_level(request.thinking.as_ref())),
        );
        if !gen.is_empty() {
            body["generation_config"] = Value::Object(gen);
        }

        // additional_params features
        if let Some(v) = request.additional_params.get("google_search") {
            if v.as_bool().unwrap_or(false) {
                let tools = body.get_mut("tools").and_then(|t| t.as_array_mut());
                if let Some(arr) = tools {
                    arr.push(json!({"type": "google_search"}));
                } else {
                    body["tools"] = json!([{"type": "google_search"}]);
                }
            }
        }
        if let Some(rf) = request.additional_params.get("response_format") {
            body["response_format"] = rf.clone();
        }
        if let Some(ss) = request.additional_params.get("safety_settings") {
            body["safety_settings"] = ss.clone();
        }

        // additional_params merge — route sampling params into generation_config;
        // skip params unsupported by Google Interactions API.
        if let Some(extra) = request.additional_params.as_object() {
            const GEN_CFG_KEYS: &[&str] = &["top_p", "top_k", "temperature"];
            const SKIP_KEYS: &[&str] = &[
                "frequency_penalty",
                "presence_penalty",
                "repetition_penalty",
                "google_search",
                "response_format",
                "safety_settings",
            ];
            // Phase 1: route generation_config params
            for (k, v) in extra {
                if GEN_CFG_KEYS.contains(&k.as_str()) {
                    if let Some(gc) = body
                        .get_mut("generation_config")
                        .and_then(|g| g.as_object_mut())
                    {
                        gc.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }
            }
            // Phase 2: merge remaining top-level params
            if let Some(obj) = body.as_object_mut() {
                for (k, v) in extra {
                    if GEN_CFG_KEYS.contains(&k.as_str()) || SKIP_KEYS.contains(&k.as_str()) {
                        continue;
                    }
                    if !obj.contains_key(k) {
                        obj.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        let send = |body: Value| {
            let request = self
                .http
                .post(&url)
                .headers(Google.auth_headers(&self.api_key))
                .header("content-type", "application/json")
                .json(&body);
            let url = url.clone();
            async move {
                request
                    .send()
                    .await
                    .with_context(|| format!("连接 Google Interactions 失败: {url}"))
            }
        };

        let mut response = send(body.clone()).await?;
        // 服务端 interaction 状态过期或与客户端历史不一致时，续写会以 404 拒绝；
        // 此时放弃 previous_interaction_id 并整段重放。
        if converted.continues_previous && response.status() == reqwest::StatusCode::NOT_FOUND {
            if let Some(obj) = body.as_object_mut() {
                obj.remove("previous_interaction_id");
                obj.insert(
                    "input".into(),
                    json!(to_interactions_input(&request.messages, false).steps),
                );
            }
            response = send(body).await?;
        }

        crate::shared::sse::sse_stream(
            response,
            crate::shared::sse::wrap_single_extract(extract_interactions_delta),
        )
        .await
        .with_context(|| format!("{url} (model={model})"))
    }
}

/// Interactions 的思考档位。
///
/// 服务端只接受 `low` / `medium` / `high`，`minimal` 会被整轮拒收
/// （400 invalid_request），因此关闭思考与 minimal effort 都落到最低档 `low`。
fn thinking_level(thinking: Option<&crate::types::request::ThinkingConfig>) -> &'static str {
    match thinking {
        Some(tc) if !tc.enabled => "low",
        Some(tc) => match tc.effort.trim() {
            "max" | "xhigh" | "high" => "high",
            "minimal" | "min" | "low" => "low",
            _ => "medium",
        },
        None => "medium",
    }
}

// ─── Message Conversion ──────────────────────────────────

/// `to_interactions_input` 的产物。
struct InteractionsInput {
    system: Option<String>,
    steps: Vec<Value>,
    /// 是否以 `previous_interaction_id` 续写服务端已有的 interaction。
    continues_previous: bool,
}

/// 把内部消息序列转成 Interactions API 的 `input` 步骤。
///
/// 工具回合（末尾只剩 tool 结果）在服务端已经存有 `function_call` 与
/// thought signature，此时只补发新增的 `function_result`；重放整段历史会与
/// 服务端状态冲突。其余情况（新的用户消息、压缩后的历史）以客户端历史为准，
/// 整段重放并放弃续写。
///
/// 服务端只接受自己签发的 `function_call`，客户端重放的会被判为非法参数。
/// 因此整段重放不伪造 function call：只保留助手正文，并把既有工具结果作为
/// 明确标注来源的用户输入上下文。
fn to_interactions_input(
    messages: &[crate::types::Message],
    has_previous: bool,
) -> InteractionsInput {
    use crate::types::message::*;

    let tool_loop_start = messages
        .iter()
        .rposition(|m| matches!(m, Message::Assistant { .. }))
        .map(|idx| idx + 1)
        .filter(|start| {
            *start < messages.len() && messages[*start..].iter().all(|m| m.role() == Role::Tool)
        });
    let continues_previous = has_previous && tool_loop_start.is_some();
    let replayed = if continues_previous {
        &messages[tool_loop_start.unwrap_or(0)..]
    } else {
        messages
    };

    // Message::Tool 不带工具名，但 function_result 步骤必须带，从助手回合回填。
    let mut call_names = std::collections::HashMap::new();
    for m in messages {
        if let Message::Assistant { content } = m {
            for c in content {
                if let AssistantContent::ToolCall(tc) = c {
                    call_names.insert(tc.id.as_str(), tc.name.as_str());
                }
            }
        }
    }
    // system_instruction 是 interaction 级参数，续写时同样要重发。
    let system = messages.iter().find_map(|m| match m {
        Message::System { content } => Some(content.clone()),
        _ => None,
    });

    let steps = to_interactions_steps(replayed, &call_names, continues_previous);
    InteractionsInput {
        system,
        steps,
        continues_previous,
    }
}

fn to_interactions_steps(
    messages: &[crate::types::Message],
    call_names: &std::collections::HashMap<&str, &str>,
    continues_previous: bool,
) -> Vec<Value> {
    use crate::types::message::*;
    let mut steps = Vec::new();

    for m in messages {
        match m {
            // system_instruction 由调用方单独下发，不进 input。
            Message::System { .. } => {}
            Message::User { content } => {
                let parts: Vec<Value> = content
                    .iter()
                    .map(|c| match c {
                        UserContent::Text { text } => json!({"type": "text", "text": text}),
                        UserContent::Image { url } => media_part("image", url, "image/jpeg"),
                        UserContent::Audio { url, mime_type } => {
                            media_part("audio", url, mime_type)
                        }
                        UserContent::Video { url, mime_type } => {
                            media_part("video", url, mime_type)
                        }
                        UserContent::Document { .. } => {
                            json!({"type": "text", "text": "[document]"})
                        }
                        UserContent::ToolResult { .. } => json!({"type": "text", "text": ""}),
                    })
                    .collect();
                steps.push(json!({"type": "user_input", "content": parts}));
            }
            Message::Tool {
                tool_call_id,
                content,
                ..
            } => {
                let name = call_names
                    .get(tool_call_id.as_str())
                    .copied()
                    .unwrap_or("tool");
                if continues_previous {
                    // 缺少 name 的 function_result 会被服务端整体拒收。
                    steps.push(json!({
                        "type": "function_result",
                        "call_id": tool_call_id,
                        "name": name,
                        "result": content,
                    }));
                } else {
                    steps.push(json!({
                        "type": "user_input",
                        "content": [{
                            "type": "text",
                            "text": format!("Tool result ({name}):\n{content}"),
                        }],
                    }));
                }
            }
            Message::Assistant { content } => {
                // thinking step first
                for c in content {
                    if let AssistantContent::Thinking { text, signature } = c {
                        let mut step = json!({"type": "thought"});
                        if let Some(sig) = signature {
                            step["signature"] = json!(sig);
                        }
                        if !text.is_empty() {
                            step["content"] = json!([{"type": "text", "text": text}]);
                        }
                        steps.push(step);
                    }
                }
                // 只重放模型正文；历史 ToolCall 保持结构化语义，不伪装成文本。
                let text = content
                    .iter()
                    .filter_map(|c| match c {
                        AssistantContent::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if !text.is_empty() {
                    steps.push(json!({
                        "type": "model_output",
                        "content": [{"type": "text", "text": text}]
                    }));
                }
            }
        }
    }
    steps
}

fn media_part(kind: &str, url: &str, mime_hint: &str) -> Value {
    if url.starts_with("data:") {
        if let Some(rest) = url.strip_prefix("data:") {
            if let Some((meta, b64)) = rest.split_once(";base64,") {
                return json!({"type": kind, "inline_data": {"mime_type": meta, "data": b64}});
            }
        }
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return json!({"type": kind, "file_data": {"mime_type": mime_hint, "file_uri": url}});
    }
    json!({"type": "text", "text": format!("[{kind}: {url}]")})
}

// ─── SSE Parsing ─────────────────────────────────────────

fn extract_interactions_delta(data: &str) -> Option<crate::types::StreamChunk> {
    use crate::types::stream::{StreamChunk, Usage};
    let v: Value = serde_json::from_str(data).ok()?;
    let event_type = v.get("event_type").and_then(|e| e.as_str()).unwrap_or("");

    // interaction.created → extract interaction_id early
    if event_type == "interaction.created" {
        let id = v
            .pointer("/interaction/id")
            .and_then(|s| s.as_str())
            .map(str::to_string)?;
        return Some(StreamChunk::InteractionId(id));
    }

    // interaction completed → extract usage + finish
    if event_type.contains("completed") || event_type.contains("failed") {
        let status = v
            .pointer("/interaction/status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown");
        let interaction_id = v
            .pointer("/interaction/id")
            .and_then(|s| s.as_str())
            .map(str::to_string);

        let usage = v.pointer("/interaction/usage").and_then(|u| {
            let input = u
                .get("total_input_tokens")
                .or_else(|| u.get("prompt_tokens"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0) as u32;
            let output = u
                .get("total_output_tokens")
                .or_else(|| u.get("completion_tokens"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0) as u32;
            let reasoning = u
                .get("total_thought_tokens")
                .or_else(|| u.get("reasoning_tokens"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0) as u32;
            let cache_read = u
                .get("total_cached_tokens")
                .or_else(|| u.get("cached_tokens"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0) as u32;
            if input == 0 && output == 0 {
                return None;
            }
            Some(Usage {
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cache_read,
                cache_write_tokens: 0,
                reasoning_tokens: reasoning,
                request_count: 1,
            })
        });

        if let Some(id) = interaction_id {
            return Some(StreamChunk::InteractionId(id));
        }
        if let Some(u) = usage {
            return Some(StreamChunk::Usage(u));
        }
        let reason = if status == "requires_action" {
            "tool_calls"
        } else {
            status
        };
        return Some(StreamChunk::Done {
            finish_reason: reason.to_string(),
        });
    }

    // step events — route on event_type, not step/type
    let delta = v.get("delta");

    match event_type {
        "step.delta" => {
            let dt = delta
                .and_then(|d| d.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            match dt {
                "text" | "text_delta" => {
                    let text = delta
                        .and_then(|d| d.get("text"))
                        .and_then(|t| t.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)?;
                    Some(StreamChunk::Text(text))
                }
                "thought" | "thought_summary" => {
                    let text = delta
                        .and_then(|d| d.get("text").or(d.get("summary")))
                        .and_then(|t| t.as_str())
                        .or_else(|| {
                            delta
                                .and_then(|d| d.pointer("/content/text"))
                                .and_then(|t| t.as_str())
                        })
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)?;
                    Some(StreamChunk::Thinking(text))
                }
                "thought_signature" => {
                    let sig = delta
                        .and_then(|d| d.get("signature"))
                        .and_then(|s| s.as_str())
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)?;
                    Some(StreamChunk::ThoughtSignature(sig))
                }
                "arguments" | "arguments_delta" => {
                    let args = delta
                        .and_then(|d| d.get("arguments"))
                        .map(interactions_arguments_delta)?;
                    let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    Some(StreamChunk::ToolCallDelta {
                        index,
                        arguments: args,
                    })
                }
                "function_call" => {
                    let step = v.get("step").unwrap_or(&v);
                    let id = step
                        .get("id")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = step
                        .get("name")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    Some(StreamChunk::ToolCallStart { index, id, name })
                }
                _ => None,
            }
        }
        "step.start" => {
            let step = v.get("step")?;
            let step_kind = step.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match step_kind {
                "function_call" => {
                    let id = step
                        .get("id")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = step
                        .get("name")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    Some(StreamChunk::ToolCallStart { index, id, name })
                }
                _ => None,
            }
        }
        _ if event_type.contains("error") => {
            let msg = v
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("Google API error");
            Some(StreamChunk::Error(msg.to_string()))
        }
        _ => None,
    }
}

/// Interactions API may stream function arguments either as an object or as an
/// already-encoded JSON string. Calling `Value::to_string()` on the latter adds
/// another pair of quotes, so the accumulator eventually parses it as
/// `Value::String` instead of the object expected by tool argument structs.
fn interactions_arguments_delta(arguments: &Value) -> String {
    arguments
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| arguments.to_string())
}

// ─── 连通性探测 ─────────────────────────────────────────

/// Interactions 连通性探测（非流式最小请求）。
pub async fn probe_interactions(
    client: &HttpClient,
    model: &str,
    config: &crate::types::request::ProviderConfig,
) -> Result<String, String> {
    if config.api_key.trim().is_empty() {
        return Err("Google API Key 为空".into());
    }
    let url = crate::google::interactions_http::interactions_url(config);
    let body = serde_json::json!({
        "model": model,
        "input": "ping",
        "generation_config": { "max_output_tokens": 1 }
    });
    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .header("x-goog-api-key", config.api_key.trim())
        .header("Api-Revision", API_REVISION)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("连接 Google Interactions 失败: {e}"))?;
    let status = resp.status();
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let msg = json
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        return Err(format!("失败 ({status}): {msg}"));
    }
    Ok("调用成功".to_string())
}

// ─── Embedding Model ────────────────────────────────────

#[derive(Clone)]
pub struct GeminiEmbeddingModel(ModelBase);

impl FromClient<Google> for GeminiEmbeddingModel {
    fn from_client(client: &ProviderClient<Google>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl EmbeddingModel for GeminiEmbeddingModel {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Embedding>> {
        let cfg = self.0.to_provider_config();
        let vectors = crate::google::interactions_http::google_batch_embed(
            self.0.http(),
            texts,
            self.0.model(),
            &cfg,
        )
        .await?;
        Ok(vectors
            .into_iter()
            .map(|v| Embedding { values: v })
            .collect())
    }
}

// ─── Image Generation Model ────────────────────────────

#[derive(Clone)]
pub struct GeminiImageModel(ModelBase);

impl FromClient<Google> for GeminiImageModel {
    fn from_client(client: &ProviderClient<Google>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl ImageGenModel for GeminiImageModel {
    async fn generate(
        &self,
        prompt: &str,
        config: &ImageGenConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        let cfg = self.0.to_provider_config();
        let req = crate::google::interactions_http::InteractionImageRequest {
            prompt: prompt.to_string(),
            aspect_ratio: config.aspect_ratio.clone(),
            ..Default::default()
        };
        let result =
            crate::google::interactions_http::google_interactions_image(self.0.http(), &cfg, &req)
                .await?;
        Ok(vec![result.image])
    }
}

// ─── Video Generation Model ────────────────────────────

#[derive(Clone)]
pub struct VeoVideoModel(ModelBase);

impl FromClient<Google> for VeoVideoModel {
    fn from_client(client: &ProviderClient<Google>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl VideoGenModel for VeoVideoModel {
    async fn generate(
        &self,
        prompt: &str,
        config: &VideoGenConfig,
    ) -> anyhow::Result<GeneratedVideo> {
        let cfg = self.0.to_provider_config();
        let mut extras = crate::google::veo_http::VideoGenExtras::default();
        if let Some(ar) = config
            .additional_params
            .get("aspect_ratio")
            .and_then(|v| v.as_str())
        {
            extras.aspect_ratio = Some(ar.to_string());
        }
        if config.duration_seconds > 0 {
            extras.duration_seconds = Some(config.duration_seconds);
        }
        if !config.resolution.is_empty() {
            extras.resolution = Some(config.resolution.clone());
        }
        let result = crate::google::veo_http::google_native_generate_video(
            self.0.http(),
            prompt,
            &cfg,
            &extras,
            None,
        )
        .await?;
        Ok(GeneratedVideo {
            data: result.data,
            mime_type: result.mime_type,
            width: 0,
            height: 0,
        })
    }
}

// ─── TTS Model ──────────────────────────────────────────

#[derive(Clone)]
pub struct GeminiTTSModel(ModelBase);

impl FromClient<Google> for GeminiTTSModel {
    fn from_client(client: &ProviderClient<Google>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl TTSModel for GeminiTTSModel {
    async fn synthesize(
        &self,
        text: &str,
        tts_config: &TTSConfig,
    ) -> anyhow::Result<GeneratedAudio> {
        let cfg = self.0.to_provider_config();
        let voice = if tts_config.voice_id.is_empty() {
            "Kore".to_string()
        } else {
            tts_config.voice_id.clone()
        };
        let req = crate::google::interactions_http::InteractionTtsRequest {
            model: self.0.model().to_string(),
            input: text.to_string(),
            speech_config: vec![crate::google::interactions_http::InteractionSpeechConfig {
                speaker: None,
                voice,
            }],
            stream: false,
        };
        let result =
            crate::google::interactions_http::google_interactions_tts(self.0.http(), &cfg, &req)
                .await?;
        Ok(GeneratedAudio {
            data: result.wav_bytes,
            mime_type: "audio/wav".to_string(),
            duration_ms: 0,
        })
    }
}

// ─── Music Generation Model ────────────────────────────

#[derive(Clone)]
pub struct LyriaMusicModel(ModelBase);

impl FromClient<Google> for LyriaMusicModel {
    fn from_client(client: &ProviderClient<Google>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl MusicGenModel for LyriaMusicModel {
    async fn generate(
        &self,
        prompt: &str,
        _config: &MusicGenConfig,
    ) -> anyhow::Result<GeneratedAudio> {
        let cfg = self.0.to_provider_config();
        let model_id = crate::google::interactions_http::resolve_lyria_model_id(self.0.model())
            .unwrap_or_else(|_| self.0.model().to_string());
        let req = crate::google::interactions_http::InteractionMusicRequest {
            model: model_id,
            prompt: prompt.to_string(),
            images: vec![],
            format: crate::google::interactions_http::MusicAudioFormat::Mp3,
        };
        let result =
            crate::google::interactions_http::google_interactions_music(self.0.http(), &cfg, &req)
                .await?;
        Ok(GeneratedAudio {
            data: result.audio_bytes,
            mime_type: result.mime_type,
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::client::ChatClient;

    #[test]
    fn google_has_chat() {
        let client = ProviderClient::new("test-key", Google);
        let _model = client.completion_model("gemini-3.5-flash");
    }

    #[test]
    fn thinking_level_never_emits_unsupported_minimal() {
        use crate::types::request::ThinkingConfig;
        let off = ThinkingConfig {
            enabled: false,
            effort: "high".into(),
            ..ThinkingConfig::default()
        };
        let minimal = ThinkingConfig {
            enabled: true,
            effort: "minimal".into(),
            ..ThinkingConfig::default()
        };
        assert_eq!(thinking_level(Some(&off)), "low");
        assert_eq!(thinking_level(Some(&minimal)), "low");
        assert_eq!(thinking_level(None), "medium");
        let high = ThinkingConfig {
            enabled: true,
            effort: "xhigh".into(),
            ..ThinkingConfig::default()
        };
        assert_eq!(thinking_level(Some(&high)), "high");
    }

    #[test]
    fn system_instruction_extracted() {
        let msgs = vec![
            crate::types::Message::system("Be helpful"),
            crate::types::Message::user_text("Hi"),
        ];
        let converted = to_interactions_input(&msgs, false);
        assert_eq!(converted.system.as_deref(), Some("Be helpful"));
        assert_eq!(converted.steps.len(), 1);
        assert_eq!(converted.steps[0]["type"], "user_input");
        assert!(!converted.continues_previous);
    }

    fn tool_loop_messages() -> Vec<crate::types::Message> {
        use crate::types::message::{AssistantContent, ToolCall};
        vec![
            crate::types::Message::system("Be helpful"),
            crate::types::Message::user_text("查看当前目录"),
            crate::types::Message::assistant(vec![AssistantContent::ToolCall(ToolCall {
                id: "call-1".into(),
                name: "terminal".into(),
                arguments: json!({"command": "pwd"}),
                signature: None,
            })]),
            crate::types::Message::tool_result("call-1", "/tmp", false),
        ]
    }

    #[test]
    fn tool_loop_continues_previous_interaction_with_delta_only() {
        let converted = to_interactions_input(&tool_loop_messages(), true);
        assert!(converted.continues_previous);
        // 服务端已存有 user_input / function_call，续写只补发工具结果。
        assert_eq!(converted.steps.len(), 1);
        assert_eq!(converted.steps[0]["type"], "function_result");
        assert_eq!(converted.steps[0]["call_id"], "call-1");
        // 缺少 name 会被服务端整体拒收。
        assert_eq!(converted.steps[0]["name"], "terminal");
        assert_eq!(converted.system.as_deref(), Some("Be helpful"));
    }

    #[test]
    fn replayed_history_keeps_tool_result_context_without_faking_a_call() {
        let converted = to_interactions_input(&tool_loop_messages(), false);
        assert!(!converted.continues_previous);
        let kinds: Vec<&str> = converted
            .steps
            .iter()
            .map(|s| s["type"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(kinds, ["user_input", "user_input"]);
        let result = converted.steps[1]["content"][0]["text"].as_str().unwrap();
        assert!(result.starts_with("Tool result (terminal):"), "{result}");
        assert!(result.contains("/tmp"), "{result}");
    }

    #[test]
    fn replay_does_not_serialize_structured_tool_calls_as_text() {
        use crate::types::message::{AssistantContent, ToolCall};
        let msgs = vec![
            crate::types::Message::user_text("查看当前目录"),
            crate::types::Message::assistant(vec![
                AssistantContent::Text {
                    text: "Checking the directory.".into(),
                },
                AssistantContent::ToolCall(ToolCall {
                    id: "call-1".into(),
                    name: "terminal".into(),
                    arguments: json!({"command": "pwd"}),
                    signature: None,
                }),
            ]),
        ];
        let converted = to_interactions_input(&msgs, false);
        let text = converted.steps[1]["content"][0]["text"].as_str().unwrap();
        assert_eq!(text, "Checking the directory.");
        assert!(!text.contains("terminal"));
        assert!(!text.contains("pwd"));
    }

    #[test]
    fn new_user_turn_replays_history_instead_of_continuing() {
        let mut msgs = tool_loop_messages();
        msgs.push(crate::types::Message::assistant_text("在 /tmp"));
        msgs.push(crate::types::Message::user_text("再看一次"));
        let converted = to_interactions_input(&msgs, true);
        // 客户端历史可能已被压缩，新用户回合以客户端为准整段重放。
        assert!(!converted.continues_previous);
        assert_eq!(
            converted.steps.last().map(|s| s["type"].as_str()),
            Some(Some("user_input"))
        );
    }

    #[test]
    fn extract_text_delta() {
        let data =
            r#"{"index":1,"delta":{"text":"Hello","type":"text"},"event_type":"step.delta"}"#;
        let chunk = extract_interactions_delta(data);
        assert!(matches!(chunk, Some(crate::types::StreamChunk::Text(ref t)) if t == "Hello"));
    }

    #[test]
    fn extract_string_encoded_arguments_without_double_encoding() {
        let data = r#"{"index":0,"delta":{"type":"arguments","arguments":"{\"command\":\"pwd && ls -la\"}"},"event_type":"step.delta"}"#;
        let chunk = extract_interactions_delta(data);
        assert!(matches!(
            chunk,
            Some(crate::types::StreamChunk::ToolCallDelta { arguments, .. })
                if arguments == r#"{"command":"pwd && ls -la"}"#
        ));
    }

    #[test]
    fn extract_object_arguments_as_json() {
        let data = r#"{"index":0,"delta":{"type":"arguments","arguments":{"path":".","operation":"list"}},"event_type":"step.delta"}"#;
        let chunk = extract_interactions_delta(data);
        assert!(matches!(
            chunk,
            Some(crate::types::StreamChunk::ToolCallDelta { arguments, .. })
                if arguments == r#"{"operation":"list","path":"."}"#
        ));
    }

    #[test]
    fn extract_thought_signature() {
        let data = r#"{"index":0,"delta":{"signature":"abc123","type":"thought_signature"},"event_type":"step.delta"}"#;
        let chunk = extract_interactions_delta(data);
        assert!(
            matches!(chunk, Some(crate::types::StreamChunk::ThoughtSignature(ref s)) if s == "abc123")
        );
    }

    #[test]
    fn extract_completed_usage() {
        let data = r#"{"interaction":{"id":"v1_test","status":"completed","usage":{"total_input_tokens":10,"total_output_tokens":5}},"event_type":"interaction.completed"}"#;
        let chunk = extract_interactions_delta(data);
        assert!(
            matches!(chunk, Some(crate::types::StreamChunk::InteractionId(ref id)) if id == "v1_test")
        );
    }
}
