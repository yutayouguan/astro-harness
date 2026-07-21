//! Robotics：Google Gemini Robotics-ER 原生 generateContent。

use base64::Engine;
use providers::robotics_http::{
    default_robotics_model, default_robotics_prompt, google_robotics_generate, strip_json_fence,
    RoboticsImage, RoboticsMode,
};
use providers::trait_::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::{ImageGenCreds, ToolContext};
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RoboticsArgs {
    #[serde(default)]
    pub image_urls: Option<Vec<String>>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub queries: Option<Vec<String>>,
    #[serde(default)]
    pub robot_api: Option<String>,
    #[serde(default)]
    pub thinking_budget: Option<i32>,
    #[serde(default)]
    pub model: Option<String>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "robotics".to_string(),
        toolset: "robotics".to_string(),
        description: "Spatial robotics perception/planning via Gemini Robotics-ER. Modes: point, detect, trajectory, plan. Google-only."
            .to_string(),
        schema: schema_for_args::<RoboticsArgs>(),
        check_fn: None,
        icon: "bot",
            ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["robotics"],
    async_ctx: dispatch,
}

pub async fn dispatch(ctx: &ToolContext<'_>, args: &Value) -> anyhow::Result<String> {
    let parsed: RoboticsArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("robotics 参数无效: {e}"))?;
    let mode = RoboticsMode::parse(parsed.mode.as_deref().unwrap_or(""))?;
    let mut urls = parsed.image_urls.unwrap_or_default();
    if let Some(one) = parsed.image_url {
        let t = one.trim();
        if !t.is_empty() {
            urls.push(t.to_string());
        }
    }
    urls.retain(|u| !u.trim().is_empty());
    if urls.is_empty() {
        anyhow::bail!("robotics 需要 image_urls 或 image_url");
    }

    let Some(creds) = ctx.image_gen_targets.google() else {
        anyhow::bail!("robotics 需要配置 Google API Key（Providers 面板）");
    };

    let queries = parsed.queries.filter(|q| !q.is_empty());
    let robot_api = parsed
        .robot_api
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let user_prompt = parsed
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let prompt = default_robotics_prompt(mode, queries.as_deref(), robot_api, user_prompt);
    let thinking_budget = parsed.thinking_budget.unwrap_or(0);
    let model = parsed
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_robotics_model())
        .to_string();

    let images = resolve_images(ctx, &urls).await?;
    let text = call_google(creds, &model, &prompt, &images, thinking_budget).await?;
    let looks_json = looks_like_json_payload(&text);
    Ok(format_robotics_output(&text, &model, mode, looks_json))
}

fn looks_like_json_payload(text: &str) -> bool {
    let t = strip_json_fence(text);
    (t.starts_with('[') || t.starts_with('{')) && serde_json::from_str::<Value>(t).is_ok()
}

fn format_robotics_output(text: &str, model: &str, mode: RoboticsMode, ok_json: bool) -> String {
    let body = if ok_json {
        let t = strip_json_fence(text);
        serde_json::from_str::<Value>(t)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| text.to_string())
    } else if mode == RoboticsMode::Plan {
        // plan 常含推理 + JSON；保留原文，不加 parse=raw
        text.to_string()
    } else {
        format!("{text}\nparse=raw")
    };
    format!(
        "{body}\nprovider=google\nmodel={model}\nmode={}",
        mode.as_str()
    )
}

async fn call_google(
    creds: &ImageGenCreds,
    model: &str,
    prompt: &str,
    images: &[RoboticsImage],
    thinking_budget: i32,
) -> anyhow::Result<String> {
    let config = ProviderConfig {
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        model: model.to_string(),
        ..ProviderConfig::default()
    };
    let client = reqwest::Client::new();
    google_robotics_generate(&client, model, prompt, images, thinking_budget, &config).await
}

async fn resolve_images(
    ctx: &ToolContext<'_>,
    urls: &[String],
) -> anyhow::Result<Vec<RoboticsImage>> {
    let client = reqwest::Client::new();
    let mut out = Vec::new();
    for u in urls {
        let u = u.trim();
        if u.starts_with("http://") || u.starts_with("https://") {
            let resp = client
                .get(u)
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("下载图片失败 {u}: {e}"))?;
            if !resp.status().is_success() {
                anyhow::bail!("下载图片 HTTP {}: {u}", resp.status());
            }
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| anyhow::anyhow!("读取远程图片失败 {u}: {e}"))?;
            let mime = mime_from_url_or_path(u).to_string();
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            out.push(RoboticsImage {
                mime_type: mime,
                data_b64: b64,
            });
        } else if let Some(rest) = u.strip_prefix("data:") {
            let (meta, b64) = rest
                .split_once(',')
                .ok_or_else(|| anyhow::anyhow!("无效 data URL"))?;
            let mime = meta.split(';').next().unwrap_or("image/jpeg");
            out.push(RoboticsImage {
                mime_type: mime.to_string(),
                data_b64: b64.to_string(),
            });
        } else {
            let path = ctx.workspace_dir.join(u);
            if !path.exists() {
                anyhow::bail!("本地文件不存在: {}", path.display());
            }
            let bytes = std::fs::read(&path)
                .map_err(|e| anyhow::anyhow!("读取图片失败 {}: {e}", path.display()))?;
            let mime = mime_from_path(&path).to_string();
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            out.push(RoboticsImage {
                mime_type: mime,
                data_b64: b64,
            });
        }
    }
    Ok(out)
}

fn mime_from_url_or_path(s: &str) -> &'static str {
    let path_part = s.split(['?', '#']).next().unwrap_or(s);
    mime_from_extension(
        std::path::Path::new(path_part)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or(""),
    )
}

fn mime_from_path(path: &std::path::Path) -> &'static str {
    mime_from_extension(path.extension().and_then(|e| e.to_str()).unwrap_or(""))
}

fn mime_from_extension(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        "heic" => "image/heic",
        "heif" => "image/heif",
        _ => "image/jpeg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use providers::robotics_http::RoboticsMode;

    #[test]
    fn args_default_mode_point() {
        let args: RoboticsArgs = serde_json::from_value(serde_json::json!({
            "image_url": "a.png"
        }))
        .unwrap();
        assert_eq!(args.image_url.as_deref(), Some("a.png"));
        assert_eq!(
            RoboticsMode::parse(args.mode.as_deref().unwrap_or("")).unwrap(),
            RoboticsMode::Point
        );
    }

    #[test]
    fn format_output_marks_parse_raw_on_invalid_json() {
        let out = format_robotics_output("not-json", "m", RoboticsMode::Point, false);
        assert!(out.contains("parse=raw"));
        assert!(out.contains("provider=google"));
    }

    #[test]
    fn format_output_pretty_json_array() {
        let raw = r#"[{"point":[1,2],"label":"a"}]"#;
        let out = format_robotics_output(raw, "m", RoboticsMode::Point, true);
        assert!(out.contains("\"point\""));
        assert!(!out.contains("parse=raw"));
    }
}
