//! Gemini Interactions API 出图（Nano Banana）与视觉（describe/detect/segment）。

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};

use crate::media_http::google_native_base;
use crate::trait_::{GeneratedImage, ProviderConfig};

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

    let mut response_format = json!({ "type": "image" });
    if let Some(ar) = req.aspect_ratio.as_deref().filter(|s| !s.is_empty()) {
        response_format["aspect_ratio"] = json!(ar);
    }
    if let Some(sz) = req.image_size.as_deref().filter(|s| !s.is_empty()) {
        response_format["image_size"] = json!(sz);
    }
    if let Some(mt) = req.mime_type.as_deref().filter(|s| !s.is_empty()) {
        response_format["mime_type"] = json!(mt);
    }

    let mut body = json!({
        "model": model,
        "input": input,
        "response_format": response_format,
    });
    if let Some(id) = req.previous_interaction_id.as_deref().filter(|s| !s.is_empty()) {
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

    let image = last_image.ok_or_else(|| {
        anyhow!("未返回图片数据（可能被安全策略拦截）")
    })?;
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

    let response = client
        .post(&url)
        .header("x-goog-api-key", &config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google Interactions API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Google Interactions 响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Google Interactions 出图失败");
        anyhow::bail!("Google interactions HTTP {status}: {msg}");
    }
    parse_interaction_image_response(&v)
}

fn trim_slash(s: &str) -> String {
    s.trim_end_matches('/').to_string()
}

fn interactions_url(config: &ProviderConfig) -> String {
    let base = trim_slash(&google_native_base(config));
    if base.contains("/v1beta") {
        format!("{base}/interactions")
    } else {
        format!("{base}/v1beta/interactions")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionMode {
    Describe,
    Detect,
    Segment,
}

impl VisionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Describe => "describe",
            Self::Detect => "detect",
            Self::Segment => "segment",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "describe" => Ok(Self::Describe),
            "detect" => Ok(Self::Detect),
            "segment" => Ok(Self::Segment),
            other => anyhow::bail!("无效 mode: {other}（期望 describe|detect|segment）"),
        }
    }
}

impl std::str::FromStr for VisionMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[derive(Debug, Clone)]
pub enum VisionImagePart {
    Inline {
        mime_type: String,
        data_b64: String,
    },
    Uri {
        mime_type: String,
        uri: String,
    },
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
            VisionImagePart::Inline { mime_type, data_b64 } => {
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
            body["generation_config"] = json!({ "thinking_level": "minimal" });
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
        crate::media_http::default_vision_model("google")
    } else {
        config.model.trim()
    };
    let url = interactions_url(config);
    let body = build_interaction_vision_body(model, prompt, images, mode);
    let response = client
        .post(&url)
        .header("x-goog-api-key", &config.api_key)
        .header("content-type", "application/json")
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn parse_errors_when_no_image() {
        let v = json!({ "id": "ix", "steps": [{ "type": "model_output", "content": [{ "type": "text", "text": "x" }] }] });
        assert!(parse_interaction_image_response(&v).is_err());
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
    fn build_segment_body_has_mask_and_minimal_thinking() {
        let body = build_interaction_vision_body(
            "gemini-3.5-flash",
            "seg",
            &[VisionImagePart::Uri {
                mime_type: "image/png".into(),
                uri: "https://x/y.png".into(),
            }],
            VisionMode::Segment,
        );
        let props = &body["response_format"]["schema"]["properties"]["boxes"]["items"]["properties"];
        assert!(props.get("mask").is_some());
        assert_eq!(body["generation_config"]["thinking_level"], "minimal");
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
