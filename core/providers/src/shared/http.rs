//! 跨厂商共享 HTTP 工具函数。

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::types::request::ProviderConfig;

/// 将 `additional_params` 浅合并进请求体（对象字段覆盖同名键；非对象则忽略）
pub fn merge_additional_params(body: &mut Value, params: &Value) {
    let (Some(obj), Some(extra)) = (body.as_object_mut(), params.as_object()) else {
        return;
    };
    for (k, v) in extra {
        obj.insert(k.clone(), v.clone());
    }
}

/// 去掉 endpoint 末尾斜杠。
pub fn trim_slash(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

/// 优先使用配置中的 `base_url`，否则回退到供应商默认值。
pub fn resolve_base(config: &ProviderConfig, provider: &str) -> String {
    config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| crate::profile::default_base_for(provider))
        .to_string()
}

pub fn parse_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(";base64,")?;
    let media_type = meta.trim();
    if media_type.is_empty() || data.is_empty() {
        return None;
    }
    Some((media_type.to_string(), data.to_string()))
}

/// 从 JSON 响应体提取 `/error/message` 或顶层 `error` 字符串。
pub fn json_error_option(v: &Value) -> Option<&str> {
    v.pointer("/error/message")
        .and_then(|m| m.as_str())
        .or_else(|| v.get("error").and_then(|e| e.as_str()))
}

/// [`json_error_option`] 的带默认值版本。
pub fn json_error_message<'a>(v: &'a Value, default: &'a str) -> &'a str {
    json_error_option(v).unwrap_or(default)
}

/// 检查 HTTP 响应状态；非 2xx 时消耗响应体并返回结构化错误。
pub async fn check_response_status(
    response: reqwest::Response,
) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
                .or_else(|| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        })
        .unwrap_or(body);
    Err(anyhow!("上游 HTTP {status}: {msg}"))
}

