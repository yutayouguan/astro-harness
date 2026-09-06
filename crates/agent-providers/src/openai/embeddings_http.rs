//! OpenAI Embeddings HTTP（`POST /embeddings`）。

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::DEFAULT_API_BASE;
use crate::compat::openai_compatible_base;
use crate::types::request::ProviderConfig;

/// 默认 Embedding 模型。
pub fn default_embedding_model() -> &'static str {
    super::defaults::DEFAULT_EMBEDDING_MODEL
}

fn openai_base(config: &ProviderConfig) -> String {
    let raw = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_API_BASE);
    openai_compatible_base(raw)
}

fn build_embedding_request(
    client: &Client,
    texts: &[String],
    model: &str,
    config: &ProviderConfig,
) -> Result<reqwest::Request> {
    let base = openai_base(config);
    let url = format!("{base}/embeddings");
    let body = json!({
        "model": model,
        "input": texts,
    });
    client
        .post(url)
        .bearer_auth(config.api_key.trim())
        .header("content-type", "application/json")
        .json(&body)
        .build()
        .context("build OpenAI-compatible Embedding request")
}

/// 批量生成文本 embedding 向量。
///
/// 签名对齐 [`crate::google::interactions_http::google_batch_embed`]。
pub async fn openai_batch_embed(
    client: &Client,
    texts: &[String],
    model: &str,
    config: &ProviderConfig,
) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    if config.api_key.trim().is_empty() {
        anyhow::bail!("OpenAI API Key 为空");
    }

    let model = if model.trim().is_empty() {
        default_embedding_model()
    } else {
        model.trim()
    };

    let request = build_embedding_request(client, texts, model, config)?;
    let url = request.url().to_string();
    let resp = client
        .execute(request)
        .await
        .with_context(|| format!("连接 OpenAI Embedding API 失败: {url}"))?;

    let status = resp.status();
    let v: Value = resp
        .json()
        .await
        .context("解析 OpenAI Embedding 响应失败")?;

    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .or_else(|| v.get("error").and_then(|e| e.as_str()))
            .unwrap_or("OpenAI Embedding 请求失败");
        anyhow::bail!("OpenAI Embedding HTTP {status}: {msg}");
    }

    let data = v
        .get("data")
        .and_then(|d| d.as_array())
        .context("OpenAI Embedding 响应缺少 data 数组")?;

    let mut result: Vec<(usize, Vec<f32>)> = data
        .iter()
        .map(|item| {
            let index = item.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
            let embedding = item
                .get("embedding")
                .and_then(|e| e.as_array())
                .context("embedding 缺少 embedding 数组")?
                .iter()
                .map(|x| {
                    x.as_f64()
                        .map(|f| f as f32)
                        .context("embedding value 非数值")
                })
                .collect::<Result<Vec<f32>>>()?;
            Ok((index, embedding))
        })
        .collect::<Result<Vec<_>>>()?;

    result.sort_by_key(|(i, _)| *i);
    Ok(result.into_iter().map(|(_, v)| v).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model() {
        assert_eq!(default_embedding_model(), "text-embedding-3-small");
    }

    #[test]
    fn empty_input_returns_empty() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = Client::new();
        let config = ProviderConfig::default();
        let result = rt.block_on(openai_batch_embed(&client, &[], "", &config));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn azure_v1_embedding_request_uses_bearer_contract() {
        let client = Client::new();
        let config = ProviderConfig {
            api_key: "azure-secret".into(),
            base_url: Some(crate::impls::azure::azure_openai_v1_base(
                "https://example.openai.azure.com",
            )),
            model: "text-embedding-3-small".into(),
            ..ProviderConfig::default()
        };
        let request =
            build_embedding_request(&client, &["hello".to_string()], &config.model, &config)
                .expect("embedding request should build");
        assert_eq!(
            request.url().as_str(),
            "https://example.openai.azure.com/openai/v1/embeddings"
        );
        assert_eq!(
            request
                .headers()
                .get("authorization")
                .expect("Authorization header"),
            "Bearer azure-secret"
        );
        assert!(request.headers().get("api-key").is_none());
        let body: Value = serde_json::from_slice(
            request
                .body()
                .and_then(reqwest::Body::as_bytes)
                .expect("JSON request body"),
        )
        .expect("valid JSON body");
        assert_eq!(body["model"], "text-embedding-3-small");
        assert_eq!(body["input"], json!(["hello"]));
    }
}
