//! 结构化抽取（Rig Extractor 风格）：强制模型通过原生 `submit` 工具提交强类型结果。

use std::collections::BTreeMap;
use std::marker::PhantomData;

use futures::StreamExt;
use schemars::schema_for;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use thiserror::Error;

use crate::types::message::Message;
use crate::types::request::ProviderConfig;
use crate::types::stream::StreamChunk;

/// 结构化抽取过程中的错误类型。
#[derive(Debug, Error)]
pub enum ExtractionError {
    /// 模型输出中未找到可解析的数据。
    #[error("No data extracted")]
    NoData,

    /// JSON 反序列化为目标类型失败。
    #[error("Failed to deserialize the extracted data: {0}")]
    DeserializationError(#[from] serde_json::Error),

    /// 调用模型或解析流程中的业务错误。
    #[error("PromptError: {0}")]
    PromptError(String),
}

/// 从 JSON 围栏或裸 JSON 文本中解析结构化载荷。
///
/// 这是供现有直接调用方使用的纯 JSON 工具；[`Extractor`] 本身只接受原生
/// `submit` 工具调用，不从模型正文提取工具语义。
pub fn parse_submit_payload<T: DeserializeOwned>(raw: &str) -> Result<T, ExtractionError> {
    if let Some(obj) = extract_json_object(raw) {
        return Ok(serde_json::from_value(obj)?);
    }
    Err(ExtractionError::NoData)
}

#[derive(Debug, Default)]
struct NativeSubmitAccumulator {
    names: BTreeMap<u32, String>,
    arguments: BTreeMap<u32, String>,
}

impl NativeSubmitAccumulator {
    fn start(&mut self, index: u32, name: String) {
        self.names.insert(index, name);
    }

    fn push_arguments(&mut self, index: u32, delta: &str) {
        let arguments = self.arguments.entry(index).or_default();
        if looks_like_complete_json(delta)
            && (arguments.is_empty() || looks_like_complete_json(arguments))
        {
            *arguments = delta.to_string();
        } else {
            arguments.push_str(delta);
        }
    }

    fn finish<T: DeserializeOwned>(self) -> Result<T, ExtractionError> {
        let Some((index, _)) = self
            .names
            .iter()
            .find(|(_, name)| name.as_str() == "submit")
        else {
            return Err(ExtractionError::NoData);
        };
        let arguments = self.arguments.get(index).map(String::as_str).unwrap_or("");
        Ok(serde_json::from_str(arguments.trim())?)
    }
}

fn looks_like_complete_json(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with('{') && trimmed.ends_with('}')
}

/// 解析完整的 JSON 对象（支持 markdown 围栏与裸 JSON）。
fn extract_json_object(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if let Some(stripped) = strip_markdown_fence(trimmed) {
        if let Ok(v) = serde_json::from_str::<Value>(stripped.trim()) {
            if v.is_object() {
                return Some(v);
            }
        }
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if v.is_object() {
            return Some(v);
        }
    }
    None
}

/// 去掉 markdown ```json 围栏，返回内部文本切片。
fn strip_markdown_fence(s: &str) -> Option<&str> {
    let s = s.trim();
    if !s.starts_with("```") {
        return None;
    }
    let rest = s.strip_prefix("```")?;
    let rest = rest
        .strip_prefix("json")
        .or_else(|| rest.strip_prefix("JSON"))
        .unwrap_or(rest);
    let rest = rest.trim_start_matches('\n');
    let end = rest.rfind("```")?;
    Some(&rest[..end])
}

/// 为目标类型 `T` 生成 JSON Schema 并序列化为 `Value`。
fn schema_value_for<T: JsonSchema>() -> Value {
    let schema = schema_for!(T);
    serde_json::to_value(schema).unwrap_or_else(|_| json!({ "type": "object" }))
}

/// 结构化抽取器的构建器（链式配置 preamble / context）。
pub struct ExtractorBuilder<T> {
    /// 供应商标识符。
    provider: String,
    /// 模型名称。
    model: String,
    /// 运行时配置（可在 `build` 时写入 model）。
    config: ProviderConfig,
    /// 抽取规则说明（写入 system prompt）。
    preamble: Option<String>,
    /// 附加上下文（写入 system prompt）。
    context: Option<String>,
    /// 目标类型占位，不占用运行时内存。
    _t: PhantomData<T>,
}

impl<T> ExtractorBuilder<T>
where
    T: DeserializeOwned + Serialize + JsonSchema + Send + Sync,
{
    /// 创建构建器。
    pub fn new(
        provider: impl Into<String>,
        model: impl Into<String>,
        config: ProviderConfig,
    ) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            config,
            preamble: None,
            context: None,
            _t: PhantomData,
        }
    }

    /// 设置抽取规则（preamble），追加到 system prompt。
    pub fn preamble(mut self, text: impl Into<String>) -> Self {
        self.preamble = Some(text.into());
        self
    }

    /// 设置附加上下文，追加到 system prompt。
    pub fn context(mut self, text: impl Into<String>) -> Self {
        self.context = Some(text.into());
        self
    }

    /// 完成构建，返回可执行的 [`Extractor`]。
    pub fn build(self) -> Extractor<T> {
        let mut config = self.config;
        if config.model.trim().is_empty() {
            config.model = self.model.clone();
        } else if !self.model.trim().is_empty() {
            config.model = self.model;
        }
        Extractor {
            provider: self.provider,
            config,
            preamble: self.preamble,
            context: self.context,
            _t: PhantomData,
        }
    }
}

/// 从 provider id、model、config 快速构造 [`ExtractorBuilder`]（替代旧 `ProviderClient.extractor()`）。
pub fn extractor<T>(
    provider: impl Into<String>,
    model: impl Into<String>,
    config: ProviderConfig,
) -> ExtractorBuilder<T>
where
    T: DeserializeOwned + Serialize + JsonSchema + Send + Sync,
{
    ExtractorBuilder::new(provider, model, config)
}

/// 从 provider id 和环境变量快速构造 [`ExtractorBuilder`]。
pub fn extractor_from_env<T>(
    provider_id: &str,
    model: impl Into<String>,
) -> anyhow::Result<ExtractorBuilder<T>>
where
    T: DeserializeOwned + Serialize + JsonSchema + Send + Sync,
{
    let auth = crate::profile::AuthKind::for_provider(provider_id);
    let api_key = if auth == crate::profile::AuthKind::None {
        String::new()
    } else {
        crate::profile::read_env_api_key(provider_id)
            .ok_or_else(|| anyhow::anyhow!("未找到 {} 的 API Key 环境变量", provider_id))?
    };
    let base_url =
        Some(crate::profile::default_base_for(provider_id).to_string()).filter(|s| !s.is_empty());
    let config = ProviderConfig {
        api_key,
        base_url,
        model: String::new(),
        temperature: 0.2,
        max_tokens: 4096,
        thinking_enabled: false,
        reasoning_effort: "high".into(),
        additional_params: serde_json::Value::Null,
        previous_interaction_id: None,
        api_mode: String::new(),
    };
    Ok(ExtractorBuilder::new(provider_id, model, config))
}

/// 已配置的结构化抽取器，对指定文本调用模型并反序列化为 `T`。
pub struct Extractor<T> {
    /// 供应商标识符。
    provider: String,
    /// 模型调用配置。
    config: ProviderConfig,
    /// 抽取规则说明。
    preamble: Option<String>,
    /// 附加上下文。
    context: Option<String>,
    /// 目标类型占位。
    _t: PhantomData<T>,
}

impl<T> Extractor<T>
where
    T: DeserializeOwned + Serialize + JsonSchema + Send + Sync,
{
    /// 等价于 [`ExtractorBuilder::new`]，提供与 Rig 一致的入口命名。
    pub fn builder(
        provider: impl Into<String>,
        model: impl Into<String>,
        config: ProviderConfig,
    ) -> ExtractorBuilder<T> {
        ExtractorBuilder::new(provider, model, config)
    }

    /// 构造 system + user 消息，引导模型通过原生 `submit` 工具返回结果。
    fn build_messages(&self, text: &str) -> Vec<Message> {
        let schema = schema_value_for::<T>();
        let schema_pretty =
            serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".to_string());

        let mut system = String::new();
        system.push_str(
            "你是结构化数据抽取器。必须且只能调用一次原生 submit 工具，不要输出解释性散文。\n",
        );
        system.push_str("arguments 必须符合以下 JSON Schema：\n");
        system.push_str(&schema_pretty);
        if let Some(preamble) = &self.preamble {
            system.push_str("\n\n# 抽取规则\n");
            system.push_str(preamble);
        }
        if let Some(context) = &self.context {
            system.push_str("\n\n# 上下文\n");
            system.push_str(context);
        }

        let user = format!("请从以下文本抽取结构化数据：\n\n{text}");
        vec![Message::system(system), Message::user_text(user)]
    }

    fn submit_tool(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "submit",
                "description": "提交符合指定 JSON Schema 的结构化抽取结果",
                "parameters": schema_value_for::<T>(),
            }
        })
    }

    /// 调用模型并反序列化为 `T`。
    pub async fn extract(&self, text: &str) -> Result<T, ExtractionError> {
        let messages = self.build_messages(text);
        let mut stream = crate::dispatch::chat_stream(
            &self.provider,
            messages,
            vec![self.submit_tool()],
            &self.config,
        )
        .await
        .map_err(|e| ExtractionError::PromptError(e.to_string()))?;

        let mut submit = NativeSubmitAccumulator::default();
        while let Some(item) = stream.next().await {
            let chunk = item.map_err(|e| ExtractionError::PromptError(e.to_string()))?;
            match chunk {
                StreamChunk::ToolCallStart { index, name, .. } => submit.start(index, name),
                StreamChunk::ToolCallDelta { index, arguments } => {
                    submit.push_arguments(index, &arguments);
                }
                StreamChunk::Error(msg) => {
                    return Err(ExtractionError::PromptError(format!("error:{msg}")));
                }
                _ => {}
            }
        }

        submit.finish()
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
    struct Tiny {
        ok: bool,
    }

    #[test]
    fn native_submit_accumulates_arguments() {
        let mut submit = NativeSubmitAccumulator::default();
        submit.push_arguments(0, "{\"ok\":");
        submit.start(0, "submit".into());
        submit.push_arguments(0, "true}");
        assert_eq!(submit.finish::<Tiny>().unwrap(), Tiny { ok: true });
    }

    #[test]
    fn native_submit_rejects_non_submit_calls() {
        let mut submit = NativeSubmitAccumulator::default();
        submit.start(0, "other".into());
        submit.push_arguments(0, "{\"ok\":true}");
        assert!(matches!(
            submit.finish::<Tiny>(),
            Err(ExtractionError::NoData)
        ));
    }

    #[test]
    fn extractor_declares_native_submit_tool_without_text_protocol() {
        let extractor =
            Extractor::<Tiny>::builder("openai", "test-model", ProviderConfig::default()).build();
        let messages = extractor.build_messages("source");
        assert!(messages
            .iter()
            .all(|message| !message.text_content().contains("<tool_call>")));
        let tool = extractor.submit_tool();
        assert_eq!(tool["function"]["name"], "submit");
        assert_eq!(tool["function"]["parameters"]["type"], "object");
    }
}
