//! OpenAI Chat Completions 兼容补全模型。
//!
//! `OpenAICompletionModel<Ext>` 通过 `Ext: OpenAICompatible` 的 hook 方法处理厂商差异，
//! 无需 `match provider_id { ... }` 分支。

use std::marker::PhantomData;
use std::sync::Arc;

use anyhow::{Context, Result};
use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use super::messages::to_openai_messages_with_developer_role;
use super::sse::extract_openai_delta;
use crate::traits::{CompletionModel, FromClient, ProviderClient, ProviderExt};
use crate::types::{CompletionRequest, CompletionStream};

/// Thinking 请求格式 — 厂商如何将统一 `thinking_config` 映射到线路字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingFormat {
    /// 不处理 thinking（忽略 thinking_config）。
    None,
    /// OpenAI Chat Completions: `reasoning_effort` + `max_tokens→max_completion_tokens`。
    ReasoningEffort,
    /// DeepSeek: `thinking.type=enabled/disabled` + `reasoning_effort`。
    DeepSeek,
    /// MiniMax: `reasoning_split=true` + `thinking.type=adaptive/disabled`。
    MiniMaxAdaptive,
}

/// OpenAI 兼容厂商的 hook trait。
///
/// 实现此 trait + `ProviderExt` + `Capabilities` 即可接入一个 OpenAI 兼容厂商。
/// 设置 `THINKING_FORMAT` + `EFFORT_MAP` 即可自动处理 thinking，无需覆盖 `finalize_body`。
pub trait OpenAICompatible: ProviderExt {
    /// 是否支持 `stream_options.include_usage`。
    const STREAM_USAGE: bool = true;

    /// 是否支持原生 function calling。
    const SUPPORTS_TOOLS: bool = true;

    /// Chat Completions 是否原生接受 `developer` role；旧兼容端点降级为 `system`。
    const SUPPORTS_DEVELOPER_ROLE: bool = false;

    /// 是否支持 Responses API（`/responses` 端点）。
    const SUPPORTS_RESPONSES: bool = false;

    /// Responses API: 是否设置 `store: false`（OpenAI 平台专有）。
    const RESPONSES_STORE_FALSE: bool = false;

    /// Responses API: 是否启用 `parallel_tool_calls`。
    const RESPONSES_PARALLEL_TOOLS: bool = false;

    /// Responses API: reasoning 对象是否包含 `summary: "auto"`。
    const RESPONSES_REASONING_SUMMARY: bool = false;

    /// Thinking 请求格式。
    const THINKING_FORMAT: ThinkingFormat = ThinkingFormat::None;

    /// 推理 effort 映射表（`(输入, 输出)` 对）。
    /// 未匹配时直通原始值；空值默认 `"high"`。
    const EFFORT_MAP: &'static [(&'static str, &'static str)] = &[];

    /// Chat Completions 请求体微调（线路格式差异修补）。
    ///
    /// 默认实现根据 `THINKING_FORMAT` + `EFFORT_MAP` 自动处理 thinking 参数。
    /// 仅在需要非 thinking 相关的特殊处理时才需覆盖（如 Azure 删除 model）。
    fn finalize_body(&self, body: &mut Value) {
        apply_thinking_compat(Self::THINKING_FORMAT, Self::EFFORT_MAP, body);
    }

    /// Responses API 请求体微调。
    ///
    /// 在 Responses JSON body 构造完成后、发送前调用。
    /// 默认空实现；厂商可覆盖以处理 thinking/reasoning 等差异。
    fn finalize_responses_body(&self, _body: &mut Value) {}

    /// Responses API 的 base URL。
    ///
    /// 默认直接返回传入的 base_url。Azure 覆盖此方法追加 `/openai/v1`。
    fn responses_base_url<'a>(&self, base: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(base)
    }
}

/// 根据 thinking 格式和 effort 映射表处理 `thinking_config`。
pub fn apply_thinking_compat(
    format: ThinkingFormat,
    effort_map: &[(&str, &str)],
    body: &mut Value,
) {
    let tc = match body.get("thinking_config").cloned() {
        Some(tc) => tc,
        None => return,
    };
    if let Some(obj) = body.as_object_mut() {
        obj.remove("thinking_config");
    }
    let enabled = tc.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
    let raw_effort = tc.get("effort").and_then(|v| v.as_str()).unwrap_or("high");
    let mapped_effort = effort_map
        .iter()
        .find(|(k, _)| *k == raw_effort)
        .map(|(_, v)| *v)
        .unwrap_or(if raw_effort.is_empty() {
            "high"
        } else {
            raw_effort
        });

    match format {
        ThinkingFormat::None => {}
        ThinkingFormat::ReasoningEffort => {
            if enabled {
                body["reasoning_effort"] = json!(mapped_effort);
                if let Some(obj) = body.as_object_mut() {
                    if let Some(max) = obj.remove("max_tokens") {
                        obj.insert("max_completion_tokens".to_string(), max);
                    }
                }
            }
        }
        ThinkingFormat::DeepSeek => {
            body["thinking"] = json!({
                "type": if enabled { "enabled" } else { "disabled" }
            });
            if enabled {
                body["reasoning_effort"] = json!(mapped_effort);
            }
        }
        ThinkingFormat::MiniMaxAdaptive => {
            body["reasoning_split"] = Value::Bool(true);
            body["thinking"] = json!({
                "type": if enabled { "adaptive" } else { "disabled" }
            });
        }
    }
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

        let messages = request.input_with_instructions();
        let mut body = json!({
            "model": if request.model.is_empty() { &self.model } else { &request.model },
            "messages": to_openai_messages_with_developer_role(
                &messages,
                Ext::SUPPORTS_DEVELOPER_ROLE,
            ),
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
                .flat_map(|tool| tool.function_definitions())
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                            "strict": tool.strict,
                        }
                    })
                })
                .collect();
            if !tools.is_empty() {
                body["tools"] = Value::Array(tools);
                body["tool_choice"] = json!("auto");
            }
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
        crate::shared::tool_policy::apply_openai_chat(
            &mut body,
            request.tool_choice.as_ref(),
            request.parallel_tool_calls,
        );

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_none_ignores_config() {
        let mut body = json!({"model": "m", "thinking_config": {"enabled": true, "effort": "max"}});
        apply_thinking_compat(ThinkingFormat::None, &[], &mut body);
        assert!(body.get("thinking_config").is_none());
        assert!(body.get("thinking").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn thinking_deepseek_enabled() {
        let mut body = json!({"thinking_config": {"enabled": true, "effort": "xhigh"}});
        let map = &[("max", "max"), ("xhigh", "max")];
        apply_thinking_compat(ThinkingFormat::DeepSeek, map, &mut body);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "max");
        assert!(body.get("thinking_config").is_none());
    }

    #[test]
    fn thinking_deepseek_disabled() {
        let mut body = json!({"thinking_config": {"enabled": false}});
        apply_thinking_compat(ThinkingFormat::DeepSeek, &[], &mut body);
        assert_eq!(body["thinking"]["type"], "disabled");
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn thinking_reasoning_effort_replaces_max_tokens() {
        let mut body =
            json!({"max_tokens": 4096, "thinking_config": {"enabled": true, "effort": "high"}});
        let map = &[("max", "high"), ("xhigh", "high")];
        apply_thinking_compat(ThinkingFormat::ReasoningEffort, map, &mut body);
        assert_eq!(body["reasoning_effort"], "high");
        assert!(body.get("max_tokens").is_none());
        assert_eq!(body["max_completion_tokens"], 4096);
    }

    #[test]
    fn thinking_minimax_adaptive() {
        let mut body = json!({"thinking_config": {"enabled": true}});
        apply_thinking_compat(ThinkingFormat::MiniMaxAdaptive, &[], &mut body);
        assert_eq!(body["reasoning_split"], true);
        assert_eq!(body["thinking"]["type"], "adaptive");
    }

    #[test]
    fn effort_map_passthrough_unknown() {
        let mut body = json!({"thinking_config": {"enabled": true, "effort": "medium"}});
        apply_thinking_compat(ThinkingFormat::DeepSeek, &[("max", "max")], &mut body);
        assert_eq!(body["reasoning_effort"], "medium");
    }

    #[test]
    fn effort_map_empty_defaults_to_high() {
        let mut body = json!({"thinking_config": {"enabled": true, "effort": ""}});
        apply_thinking_compat(ThinkingFormat::DeepSeek, &[], &mut body);
        assert_eq!(body["reasoning_effort"], "high");
    }
}
