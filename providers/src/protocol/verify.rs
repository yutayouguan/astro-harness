//! 连通性探测：从 Tauri `probe_one_model` 下沉的协议分支。

use reqwest::Client;
use serde_json::json;

use crate::http_stream::{azure_base, openai_compatible_base, AZURE_API_VERSION};
use crate::trait_::{ProviderConfig, VerifyResult};

/// 去掉 endpoint 末尾斜杠。
fn trim_slash(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

/// 优先使用配置中的 `base_url`，否则使用给定 fallback。
fn resolve_endpoint(config: &ProviderConfig, fallback: &str) -> String {
    config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

/// 按 provider_id 路由协议并探测（网络/协议错误收敛为 `ok: false`）。
pub async fn probe(
    client: &Client,
    provider_id: &str,
    model: &str,
    config: &ProviderConfig,
) -> VerifyResult {
    let started = std::time::Instant::now();
    let model = model.to_string();
    let fail = |message: String| VerifyResult {
        ok: false,
        latency_ms: started.elapsed().as_millis() as u64,
        model: model.clone(),
        message,
    };

    let message = match provider_id {
        "ollama" => {
            let endpoint = resolve_endpoint(config, "http://localhost:11434");
            let url = format!("{}/api/chat", trim_slash(&endpoint));
            let body = json!({
                "model": model,
                "stream": false,
                "messages": [{"role": "user", "content": "ping"}],
                "options": { "num_predict": 1 }
            });
            let resp = match client.post(&url).json(&body).send().await {
                Ok(r) => r,
                Err(e) => return fail(format!("连接 Ollama 失败: {e}")),
            };
            let status = resp.status();
            let json: serde_json::Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => return fail(e.to_string()),
            };
            if !status.is_success() {
                let msg = json["error"].as_str().unwrap_or("未知错误");
                return fail(format!("失败 ({status}): {msg}"));
            }
            "调用成功".to_string()
        }
        "claude" | "anthropic" => {
            let endpoint = resolve_endpoint(config, "https://api.anthropic.com");
            let url = format!("{}/v1/messages", trim_slash(&endpoint));
            let body = json!({
                "model": model,
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "ping"}]
            });
            let resp = match client
                .post(&url)
                .header("x-api-key", &config.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => return fail(format!("连接 Anthropic 失败: {e}")),
            };
            let status = resp.status();
            let json: serde_json::Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => return fail(e.to_string()),
            };
            if !status.is_success() {
                let msg = json["error"]["message"].as_str().unwrap_or("未知错误");
                return fail(format!("失败 ({status}): {msg}"));
            }
            "调用成功".to_string()
        }
        "google" => {
            let endpoint = resolve_endpoint(config, "https://generativelanguage.googleapis.com");
            let base = trim_slash(&endpoint);
            let api_key = &config.api_key;
            let url = if base.contains("/v1beta") {
                format!("{base}/models/{model}:generateContent?key={api_key}")
            } else {
                format!("{base}/v1beta/models/{model}:generateContent?key={api_key}")
            };
            let body = json!({
                "contents": [{"parts": [{"text": "ping"}]}],
                "generationConfig": { "maxOutputTokens": 1 }
            });
            let resp = match client.post(&url).json(&body).send().await {
                Ok(r) => r,
                Err(e) => return fail(format!("连接 Google 失败: {e}")),
            };
            let status = resp.status();
            let json: serde_json::Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => return fail(e.to_string()),
            };
            if !status.is_success() {
                let msg = json["error"]["message"].as_str().unwrap_or("未知错误");
                return fail(format!("失败 ({status}): {msg}"));
            }
            "调用成功".to_string()
        }
        "azure" => {
            let endpoint = resolve_endpoint(config, "");
            let base = azure_base(&endpoint);
            let url = format!(
                "{base}/openai/deployments/{model}/chat/completions?api-version={AZURE_API_VERSION}"
            );
            let body = json!({
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "ping"}]
            });
            let resp = match client
                .post(&url)
                .header("api-key", &config.api_key)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => return fail(format!("连接 Azure OpenAI 失败: {e}")),
            };
            let status = resp.status();
            let json: serde_json::Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => return fail(e.to_string()),
            };
            if !status.is_success() {
                let msg = json["error"]["message"].as_str().unwrap_or("未知错误");
                return fail(format!("失败 ({status}): {msg}"));
            }
            "调用成功".to_string()
        }
        // openai / deepseek / zhipu / openrouter / bailian / nvidia / moonshot /
        // volcengine / minimax / mimo / custom → OpenAI 兼容
        _ => {
            let endpoint = resolve_endpoint(config, "https://api.openai.com/v1");
            let url = format!("{}/chat/completions", openai_compatible_base(&endpoint));
            let body = json!({
                "model": model,
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "ping"}]
            });
            let mut req = client.post(&url).json(&body);
            if !config.api_key.is_empty() {
                req = req.bearer_auth(&config.api_key);
            }
            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => return fail(format!("连接模型服务失败: {e}")),
            };
            let status = resp.status();
            let json: serde_json::Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => return fail(e.to_string()),
            };
            if !status.is_success() {
                let msg = json["error"]["message"].as_str().unwrap_or("未知错误");
                return fail(format!("失败 ({status}): {msg}"));
            }
            "调用成功".to_string()
        }
    };

    VerifyResult {
        ok: true,
        latency_ms: started.elapsed().as_millis() as u64,
        model,
        message,
    }
}
