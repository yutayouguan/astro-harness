//! Gemini Robotics-ER：原生 `generateContent`（点 / 框 / 轨迹 / 规划）。

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::veo_http::google_native_base;
use crate::types::request::ProviderConfig;

fn trim_slash(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

pub fn default_robotics_model() -> &'static str {
    "gemini-robotics-er-1.6-preview"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoboticsMode {
    Point,
    Detect,
    Trajectory,
    Plan,
}

impl RoboticsMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Detect => "detect",
            Self::Trajectory => "trajectory",
            Self::Plan => "plan",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "point" => Ok(Self::Point),
            "detect" => Ok(Self::Detect),
            "trajectory" => Ok(Self::Trajectory),
            "plan" => Ok(Self::Plan),
            other => anyhow::bail!("无效 mode: {other}（期望 point|detect|trajectory|plan）"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoboticsImage {
    pub mime_type: String,
    pub data_b64: String,
}

pub fn default_robotics_prompt(
    mode: RoboticsMode,
    queries: Option<&[String]>,
    robot_api: Option<&str>,
    user_prompt: Option<&str>,
) -> String {
    if let Some(p) = user_prompt.map(str::trim).filter(|s| !s.is_empty()) {
        // 调用方自定义 prompt 时仍可附带 queries / robot_api 上下文
        let mut out = p.to_string();
        if let Some(q) = queries.filter(|q| !q.is_empty()) {
            out.push_str("\nObjects: ");
            out.push_str(&q.join(", "));
        }
        if let Some(api) = robot_api.map(str::trim).filter(|s| !s.is_empty()) {
            out.push_str("\n\nRobot API:\n");
            out.push_str(api);
            out.push_str(
                "\nProvide the sequence of function calls as a JSON list of objects with \"function\" and \"args\" keys.",
            );
        }
        return out;
    }
    match mode {
        RoboticsMode::Point => {
            if let Some(q) = queries.filter(|q| !q.is_empty()) {
                format!(
                    "Get all points matching the following objects: {}.\n\
                     The label returned should be an identifying name for the object detected.\n\
                     The answer should follow the json format:\n\
                     [{{\"point\": [y, x], \"label\": <label>}}, ...].\n\
                     The points are in [y, x] format normalized to 0-1000.",
                    q.join(", ")
                )
            } else {
                "Point to no more than 10 items in the image. The label returned \
                 should be an identifying name for the object detected.\n\
                 The answer should follow the json format: [{\"point\": [y, x], \"label\": <label1>}, ...]. \
                 The points are in [y, x] format normalized to 0-1000."
                    .to_string()
            }
        }
        RoboticsMode::Detect => {
            "Return bounding boxes as a JSON array with labels. Never return masks \
             or code fencing. Limit to 25 objects. Include as many objects as you \
             can identify.\n\
             If an object is present multiple times, name them according to their \
             unique characteristic (colors, size, position, etc.).\n\
             The format should be as follows: [{\"box_2d\": [ymin, xmin, ymax, xmax], \
             \"label\": <label for the object>}] normalized to 0-1000. The values in \
             box_2d must only be integers."
                .to_string()
        }
        RoboticsMode::Trajectory => {
            "Place a point on the primary object to move, then up to 15 points for the \
             trajectory to the target location described by the scene or task.\n\
             The points should be labeled by order of the trajectory, from '0' \
             (start) to <n> (final point).\n\
             The answer should follow the json format:\n\
             [{\"point\": [y, x], \"label\": <label>}, ...].\n\
             The points are in [y, x] format normalized to 0-1000."
                .to_string()
        }
        RoboticsMode::Plan => {
            let mut s = "Explain how to complete the task visible in the image step by step. \
                 Point to each object that you refer to. Each point should be in the format:\n\
                 [{\"point\": [y, x], \"label\": <label>}], where the coordinates are \
                 normalized between 0-1000."
                .to_string();
            if let Some(api) = robot_api.map(str::trim).filter(|a| !a.is_empty()) {
                s.push_str("\n\nYou have the following robot functions available:\n");
                s.push_str(api);
                s.push_str(
                    "\n\nProvide reasoning, then the sequence of function calls as a JSON list of objects, \
                     where each object has a \"function\" key and an \"args\" key (a list of arguments).",
                );
            }
            s
        }
    }
}

pub fn build_robotics_generate_body(
    prompt: &str,
    images: &[RoboticsImage],
    thinking_budget: i32,
) -> Value {
    let mut parts = Vec::new();
    for img in images {
        parts.push(json!({
            "inlineData": {
                "mimeType": img.mime_type,
                "data": img.data_b64
            }
        }));
    }
    parts.push(json!({ "text": prompt }));
    // model 只出现在 URL path，body 不含 model（对齐官方 REST）
    json!({
        "contents": [{ "role": "user", "parts": parts }],
        "generationConfig": {
            "temperature": 1.0,
            "thinkingConfig": {
                "thinkingBudget": thinking_budget
            }
        }
    })
}

pub fn robotics_generate_content_url(config: &ProviderConfig, model: &str) -> String {
    let base = trim_slash(&google_native_base(config));
    if base.contains("/v1beta") {
        format!("{base}/models/{model}:generateContent")
    } else {
        format!("{base}/v1beta/models/{model}:generateContent")
    }
}

pub fn parse_generate_content_text(v: &Value) -> Result<String> {
    let mut parts = Vec::new();
    if let Some(cands) = v.get("candidates").and_then(|c| c.as_array()) {
        for cand in cands {
            if let Some(ps) = cand.pointer("/content/parts").and_then(|p| p.as_array()) {
                for p in ps {
                    if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                        parts.push(t.to_string());
                    }
                }
            }
        }
    }
    let joined = parts.join("");
    if joined.trim().is_empty() {
        anyhow::bail!("generateContent 响应无文本");
    }
    Ok(joined)
}

pub fn strip_json_fence(s: &str) -> &str {
    let t = s.trim();
    let t = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```JSON"))
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim()
}

pub async fn google_robotics_generate(
    client: &Client,
    model: &str,
    prompt: &str,
    images: &[RoboticsImage],
    thinking_budget: i32,
    config: &ProviderConfig,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    if images.is_empty() {
        anyhow::bail!("robotics 至少需要一张图片");
    }
    let model = if model.trim().is_empty() {
        default_robotics_model()
    } else {
        model.trim()
    };
    let url = robotics_generate_content_url(config, model);
    let body = build_robotics_generate_body(prompt, images, thinking_budget);
    let response = client
        .post(&url)
        .header("x-goog-api-key", &config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google Robotics generateContent 失败: {url}"))?;
    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Robotics generateContent JSON 失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Robotics generateContent 失败");
        anyhow::bail!("Google Robotics HTTP {status}: {msg}");
    }
    parse_generate_content_text(&v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_model_is_robotics_er_16() {
        assert_eq!(default_robotics_model(), "gemini-robotics-er-1.6-preview");
    }

    #[test]
    fn parse_mode() {
        assert_eq!(RoboticsMode::parse("").unwrap(), RoboticsMode::Point);
        assert_eq!(RoboticsMode::parse("detect").unwrap(), RoboticsMode::Detect);
        assert!(RoboticsMode::parse("segment").is_err());
    }

    #[test]
    fn build_body_inline_and_thinking_budget() {
        let images = [RoboticsImage {
            mime_type: "image/png".into(),
            data_b64: "YWJj".into(),
        }];
        let body = build_robotics_generate_body("Point to items", &images, 0);
        let parts = body["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["inlineData"]["mimeType"], "image/png");
        assert_eq!(parts[0]["inlineData"]["data"], "YWJj");
        assert_eq!(parts[1]["text"], "Point to items");
        assert_eq!(body["generationConfig"]["temperature"], 1.0);
        assert_eq!(
            body["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            0
        );
        assert!(body.get("model").is_none());
    }

    #[test]
    fn url_uses_v1beta_generate_content() {
        let cfg = ProviderConfig {
            api_key: "k".into(),
            base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai".into()),
            ..ProviderConfig::default()
        };
        let url = robotics_generate_content_url(&cfg, "gemini-robotics-er-1.6-preview");
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-robotics-er-1.6-preview:generateContent"
        );
    }

    #[test]
    fn parse_candidates_text() {
        let v = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"text": "[{\"point\":[1,2],\"label\":\"a\"}]"}
                    ]
                }
            }]
        });
        assert!(parse_generate_content_text(&v).unwrap().contains("point"));
    }

    #[test]
    fn strip_fence() {
        let s = "```json\n[{\"point\":[1,2],\"label\":\"a\"}]\n```";
        assert!(strip_json_fence(s).starts_with('['));
    }

    #[test]
    fn point_prompt_mentions_normalized_coords() {
        let p = default_robotics_prompt(RoboticsMode::Point, None, None, None);
        assert!(p.contains("0-1000") || p.contains("0–1000"));
        assert!(p.contains("point"));
    }

    #[test]
    fn plan_prompt_includes_robot_api_when_provided() {
        let api = "def move(x,y,high): ...";
        let p =
            default_robotics_prompt(RoboticsMode::Plan, None, Some(api), Some("pick blue block"));
        assert!(p.contains("move"));
        assert!(p.contains("function"));
        assert!(p.contains("pick blue block"));
    }
}
