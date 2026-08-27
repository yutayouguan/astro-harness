//! 统一补全请求与运行时配置。

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

/// Provider-independent native tool selection policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    /// Let the model decide whether to call a tool.
    Auto,
    /// Require at least one native tool call.
    Required,
    /// Disable native tool calls.
    None,
    /// Require one specific native tool by name.
    Specific(String),
}

/// 统一聊天补全请求（provider-agnostic）。
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub model: String,
    /// Stable base instructions, independent from role-bearing request input.
    pub instructions: String,
    /// Dynamic context and conversation items with explicit roles.
    pub input: Vec<Message>,
    /// Native tool schemas; never encoded into instruction or message text.
    pub tools: Vec<ToolDefinition>,
    /// Explicit native tool-selection contract. `None` keeps the provider default.
    pub tool_choice: Option<ToolChoice>,
    /// Whether the provider may emit parallel tool calls. `None` keeps its default.
    pub parallel_tool_calls: Option<bool>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: Option<ThinkingConfig>,
    pub additional_params: Value,
    /// Google Interactions：续写上一轮 interaction。
    pub previous_interaction_id: Option<String>,
}

impl Default for CompletionRequest {
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

impl CompletionRequest {
    /// Lower the explicit instruction field into a system message for providers whose
    /// wire protocol has no dedicated top-level instructions field.
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
        let request = CompletionRequest {
            instructions: "stable base".into(),
            input: vec![
                Message::developer("dynamic policy"),
                Message::user_text("hello"),
            ],
            tools: vec![ToolDefinition {
                name: "lookup".into(),
                description: "Lookup data".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
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
        assert_eq!(request.tools[0].name, "lookup");
    }
}
