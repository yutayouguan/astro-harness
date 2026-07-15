//! Gemini Interactions API（`POST /v1beta/interactions`）。
//!
//! 当前承载 TTS；image / vision 可复用 URL 与 `x-goog-api-key` 骨架。
//! 与 OpenAI 兼容端点无关，请勿经 `…/v1beta/openai`。

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::media_http::{google_native_base, pcm_to_wav};
use crate::trait_::ProviderConfig;

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
            if let Some(speaker) = c.speaker.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
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
pub fn parse_interaction_tts_response(v: &Value) -> Result<InteractionTtsResult> {
    let interaction_id = v
        .get("id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    let b64 = v
        .pointer("/output_audio/data")
        .or_else(|| v.pointer("/outputAudio/data"))
        .and_then(|d| d.as_str())
        .ok_or_else(|| anyhow!("Google interactions TTS 响应无 output_audio.data"))?;

    let pcm = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("解码 Interactions TTS PCM 失败")?;
    if pcm.is_empty() {
        anyhow::bail!("Google interactions TTS 返回空音频");
    }

    let rate = mime_sample_rate(
        v.pointer("/output_audio/mime_type")
            .or_else(|| v.pointer("/output_audio/mimeType"))
            .or_else(|| v.pointer("/outputAudio/mime_type"))
            .and_then(|m| m.as_str())
            .unwrap_or(""),
    )
    .unwrap_or(DEFAULT_SAMPLE_RATE);

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
/// - `interaction.start` / 顶层 `id` 用于 interaction_id
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

        let dtype = delta
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("");
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

fn error_message(v: &Value) -> String {
    v.pointer("/error/message")
        .and_then(|m| m.as_str())
        .or_else(|| v.get("error").and_then(|e| e.as_str()))
        .unwrap_or("Google interactions 失败")
        .to_string()
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
    let response = client
        .post(url)
        .header("content-type", "application/json")
        .header("x-goog-api-key", config.api_key.trim())
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
        anyhow::bail!(
            "Google interactions HTTP {status}: {}",
            error_message(&v)
        );
    }
    parse_interaction_tts_response(&v)
}

async fn tts_stream_once(
    client: &Client,
    config: &ProviderConfig,
    url: &str,
    body: &Value,
) -> Result<InteractionTtsResult> {
    let response = client
        .post(url)
        .header("content-type", "application/json")
        .header("x-goog-api-key", config.api_key.trim())
        .header("Api-Revision", API_REVISION)
        .json(body)
        .send()
        .await
        .with_context(|| format!("连接 Google interactions TTS 流式失败: {url}"))?;
    let status = response.status();
    if !status.is_success() {
        let v: Value = response.json().await.unwrap_or(json!({}));
        anyhow::bail!(
            "Google interactions HTTP {status}: {}",
            error_message(&v)
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert_eq!(body["generation_config"]["speech_config"][0]["voice"], "Kore");
        assert!(body["generation_config"]["speech_config"][0].get("speaker").is_none());
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
        let sc = body["generation_config"]["speech_config"].as_array().unwrap();
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
            json!({ "event_type": "interaction.start", "id": "ix-s" }),
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
            base_url: Some(
                "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            ),
            model: String::new(),
            ..ProviderConfig::default()
        };
        assert_eq!(
            interactions_url(&cfg),
            "https://generativelanguage.googleapis.com/v1beta/interactions"
        );
    }
}
