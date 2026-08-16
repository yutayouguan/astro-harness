//! OpenAI Chat Completions 兼容补全模型。
//!
//! `OpenAICompletionModel<Ext>` 通过 `Ext: OpenAICompatible` 的 hook 方法处理厂商差异，
//! 无需 `match provider_id { ... }` 分支。

use std::marker::PhantomData;
use std::sync::Arc;

use anyhow::{Context, Result};
use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use super::messages::to_openai_messages;
use super::sse::extract_openai_delta;
use crate::traits::{CompletionModel, FromClient, ProviderClient, ProviderExt};
use crate::types::{CompletionRequest, CompletionStream};

/// OpenAI 兼容厂商的 hook trait。
///
/// 实现此 trait + `ProviderExt` + `Capabilities` 即可接入一个 OpenAI 兼容厂商。
pub trait OpenAICompatible: ProviderExt {
    /// 是否支持 `stream_options.include_usage`。
    const STREAM_USAGE: bool = true;

    /// 是否支持原生 function calling。
    const SUPPORTS_TOOLS: bool = true;

    /// 请求体微调（线路格式差异修补）。
    ///
    /// 在 JSON body 构造完成后、发送前调用。
    /// 可用于：
    /// - DeepSeek: 注入 thinking 参数
    /// - MiniMax: 调整字段名
    fn finalize_body(&self, _body: &mut Value) {}
}

/// 泛型 OpenAI 兼容补全模型。
///
/// `Ext` 为厂商扩展类型，通过 `OpenAICompatible` trait 的 hook 处理厂商差异。
pub struct OpenAICompletionModel<Ext> {
    http: HttpClient,
    base_url: String,
    api_key: String,
    model: String,
    _ext: PhantomData<Ext>,
    ext_instance: Ext,
}

impl<Ext: Clone> Clone for OpenAICompletionModel<Ext> {
    fn clone(&self) -> Self {
        Self {
            http: self.http.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            model: self.model.clone(),
            _ext: PhantomData,
            ext_instance: self.ext_instance.clone(),
        }
    }
}

impl<Ext: ProviderExt> FromClient<Ext> for OpenAICompletionModel<Ext> {
    fn from_client(client: &ProviderClient<Ext>, model: &str) -> Self {
        Self {
            http: client.http.clone(),
            base_url: client.base_url.clone(),
            api_key: client.api_key.clone(),
            model: model.to_string(),
            _ext: PhantomData,
            ext_instance: client.ext.clone(),
        }
    }
}

#[async_trait::async_trait]
impl<Ext> CompletionModel for OpenAICompletionModel<Ext>
where
    Ext: OpenAICompatible + Clone + Send + Sync + 'static,
{
    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{base}/chat/completions");

        let mut body = json!({
            "model": if request.model.is_empty() { &self.model } else { &request.model },
            "messages": to_openai_messages(&request.messages),
            "stream": true,
        });

        if let Some(temp) = request.temperature {
            body["temperature"] = json!(temp);
        }
        if let Some(max) = request.max_tokens {
            body["max_tokens"] = json!(max);
        }

        // thinking 模式下 max_tokens 同时覆盖推理和正文。DeepSeek R1 等模型的
        // reasoning 轻松消耗 10k-30k tokens，16384 远远不够，导致正文为空。
        // 保底 65536 覆盖 DeepSeek R1 (max 64k) / MiniMax 等主流 thinking 模型。
        if request.thinking.as_ref().is_some_and(|tc| tc.enabled) {
            let current = body.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            if current < 65536 {
                body["max_tokens"] = json!(65536);
            }
        }

        if Ext::STREAM_USAGE {
            body["stream_options"] = json!({"include_usage": true});
        }

        if let Some(tc) = &request.thinking {
            body["thinking_config"] = serde_json::json!({
                "enabled": tc.enabled,
                "effort": tc.effort,
            });
        }

        if Ext::SUPPORTS_TOOLS && !request.tools.is_empty() {
            let tools: Vec<Value> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })
                })
                .collect();
            body["tools"] = Value::Array(tools);
            body["tool_choice"] = json!("auto");
        }

        // 厂商 hook：线路格式微调
        self.ext_instance.finalize_body(&mut body);

        // 清理未被 finalize_body 消费的 thinking_config（避免发送给不支持的 API）
        if let Some(obj) = body.as_object_mut() {
            obj.remove("thinking_config");
        }

        // additional_params 合并
        if let Some(extra) = request.additional_params.as_object() {
            if let Some(obj) = body.as_object_mut() {
                for (k, v) in extra {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }

        let auth = self.ext_instance.auth_headers(&self.api_key);
        let has_auth = !auth.is_empty();
        let mut req_builder = self
            .http
            .post(&url)
            .headers(auth)
            .header("content-type", "application/json")
            .json(&body);

        if !self.api_key.is_empty() && !has_auth {
            req_builder = req_builder.bearer_auth(&self.api_key);
        }

        let response = req_builder
            .send()
            .await
            .with_context(|| format!("连接 {} 失败: {url}", Ext::NAME))?;

        let stream =
            crate::shared::sse::sse_stream(response, Arc::new(extract_openai_delta)).await?;
        Ok(super::think_tag::wrap_think_tag_extraction(stream))
    }
}
