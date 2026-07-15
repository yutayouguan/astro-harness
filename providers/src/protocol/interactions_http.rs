//! Gemini Interactions API 出图（Nano Banana）。

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use serde_json::{json, Value};

use crate::trait_::GeneratedImage;

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
