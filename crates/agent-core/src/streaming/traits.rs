//! Rig 风格流式分层 trait：`StreamingCompletion` / `StreamingChat` / `StreamingPrompt`。

use async_trait::async_trait;
use providers::types::message::Message as ProviderMessage;
use types::message::Message;

use super::types::AssistantContentStream;

/// 底层流式 completion：直接接收 Provider 格式消息列表。
#[async_trait]
pub trait StreamingCompletion: Send + Sync {
    /// 对给定 messages 与 tools schema 发起流式 completion。
    async fn stream_completion(
        &self,
        messages: Vec<ProviderMessage>,
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream>;
}

/// 带 Astro 会话历史的流式 chat 抽象。
#[async_trait]
pub trait StreamingChat: Send + Sync {
    /// 将 system prompt 与会话历史转换为 Provider 消息后流式请求。
    async fn stream_chat(
        &self,
        system_prompt: &str,
        history: &[Message],
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream>;
}

/// 无历史的一次性流式 prompt 抽象。
#[async_trait]
pub trait StreamingPrompt: Send + Sync {
    /// 将单条 user prompt 包装为历史后流式请求。
    async fn stream_prompt(
        &self,
        system_prompt: &str,
        prompt: &str,
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream>;
}
