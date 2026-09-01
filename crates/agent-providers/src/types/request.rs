//! 统一补全请求与运行时配置。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use agent_protocol::ResponseItem;

use super::message::{Message, ToolDefinition};

/// 单次模型调用的运行时配置。
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// API 密钥。
    pub api_key: String,
    /// 自定义 API 基址；为空时使用供应商默认。
    pub base_url: Option<String>,
    /// 模型名称或部署 ID。
    pub model: String,
    /// 采样温度。
    pub temperature: f32,
    /// 最大生成 token 数。
    pub max_tokens: u32,
    /// DeepSeek V4 等：是否开启 thinking。
    pub thinking_enabled: bool,
    /// DeepSeek：`high` | `max`（仅 thinking 开启时生效）。
    pub reasoning_effort: String,
    /// Provider 扩展参数（对齐 Rig additional_params），合并进请求 JSON。
    pub additional_params: serde_json::Value,
    /// Google Interactions：续写上一轮 interaction（工具多轮保留 thought/signature）。
    pub previous_interaction_id: Option<String>,
    /// API 协议模式覆盖（空 = profile 默认；`"responses"` = Responses API）。
    pub api_mode: String,
}

impl Default for ProviderConfig {
    /// 返回适用于 OpenAI 风格模型的默认配置。
    fn default() -> Self {
        ProviderConfig {
            api_key: String::new(),
            base_url: None,
            model: "gpt-5.6".to_string(),
            temperature: 0.7,
            max_tokens: 4096,
            thinking_enabled: false,
            reasoning_effort: "high".to_string(),
            additional_params: serde_json::Value::Null,
            previous_interaction_id: None,
            api_mode: String::new(),
        }
    }
}

/// Thinking / 推理配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThinkingConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
    #[serde(default)]
    pub effort: String,
}

/// 统一的原生工具选择策略（provider 无关）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    /// 由模型自行决定是否调用工具。
    Auto,
    /// 强制至少调用一次原生工具。
    Required,
    /// 禁用原生工具调用。
    None,
    /// 强制调用指定名称的工具。
    Specific(String),
}

/// Agent 交给模型的原生 Responses prompt。
#[derive(Debug, Clone)]
pub struct Prompt {
    pub model: String,
    /// 稳定的基础指令，独立于带角色的对话输入。
    pub instructions: String,
    /// 原生 Responses input。Agent 历史不得经 `Message` 降级后进入此字段。
    pub input: Vec<ResponseItem>,
    /// 原生工具 schema；不编码到指令或消息文本中。
    pub tools: Vec<ToolDefinition>,
    /// 显式工具选择策略。`None` 保持 provider 默认。
    pub tool_choice: Option<ToolChoice>,
    /// 是否允许 provider 并行发起工具调用。`None` 保持默认。
    pub parallel_tool_calls: Option<bool>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: Option<ThinkingConfig>,
    pub additional_params: Value,
}

impl Default for Prompt {
    fn default() -> Self {
        Self {
            model: String::new(),
            instructions: String::new(),
            input: Vec::new(),
            tools: Vec::new(),
            tool_choice: None,
            parallel_tool_calls: None,
            temperature: None,
            max_tokens: None,
            thinking: None,
            additional_params: Value::Null,
        }
    }
}

/// 非 Agent Chat/Anthropic/Gemini 等兼容调用请求。
///
/// 此类型不进入 Agent target/fallback 链，也不能用于恢复 Agent 历史。
#[derive(Debug, Clone)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub instructions: String,
    pub input: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: Option<ToolChoice>,
    pub parallel_tool_calls: Option<bool>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: Option<ThinkingConfig>,
    pub additional_params: Value,
    pub previous_interaction_id: Option<String>,
}

impl Default for ChatCompletionRequest {
    fn default() -> Self {
        Self {
            model: String::new(),
            instructions: String::new(),
            input: Vec::new(),
            tools: Vec::new(),
            tool_choice: None,
            parallel_tool_calls: None,
            temperature: None,
            max_tokens: None,
            thinking: None,
            additional_params: Value::Null,
            previous_interaction_id: None,
        }
    }
}

impl ChatCompletionRequest {
    /// 将 instructions 字段降级为 system 消息，供线路协议没有顶层指令字段的 provider 使用。
    pub fn input_with_instructions(&self) -> Vec<Message> {
        let mut messages = Vec::with_capacity(self.input.len() + 1);
        if !self.instructions.trim().is_empty() {
            messages.push(Message::system(&self.instructions));
        }
        messages.extend(self.input.iter().cloned());
        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::message::Role;

    #[test]
    fn lowering_keeps_contract_layers_distinct() {
        let request = ChatCompletionRequest {
            instructions: "stable base".into(),
            input: vec![
                Message::developer("dynamic policy"),
                Message::user_text("hello"),
            ],
            tools: vec![ToolDefinition::function(
                "lookup",
                "Lookup data",
                serde_json::json!({"type": "object"}),
            )],
            ..Default::default()
        };

        let input = request.input_with_instructions();
        assert_eq!(input[0].role(), Role::System);
        assert_eq!(input[1].role(), Role::Developer);
        assert_eq!(input[2].role(), Role::User);
        assert!(!request.instructions.contains("lookup"));
        assert!(request
            .input
            .iter()
            .all(|message| !message.text_content().contains("lookup")));
        assert_eq!(request.tools[0].name(), "lookup");
    }
}
