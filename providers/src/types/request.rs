//! 统一补全请求。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::message::{Message, ToolDefinition};

/// Thinking / 推理配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThinkingConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
    #[serde(default)]
    pub effort: String,
}

/// 统一聊天补全请求（provider-agnostic）。
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
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
            messages: Vec::new(),
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            thinking: None,
            additional_params: Value::Null,
            previous_interaction_id: None,
        }
    }
}
