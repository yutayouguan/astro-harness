//! Agent streaming contract: OpenAI-compatible Responses API only.

use async_trait::async_trait;

use super::types::AssistantContentStream;

#[async_trait]
pub trait StreamingResponses: Send + Sync {
    async fn stream_response(
        &self,
        instructions: String,
        input: Vec<agent_protocol::ResponseItem>,
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream>;
}
