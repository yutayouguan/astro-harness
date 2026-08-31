use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Codex/Responses native history item.
///
/// This is intentionally the persisted shape instead of a chat-completions
/// message projection. Keeping calls and outputs as distinct items preserves
/// their protocol identity across process restarts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseItem {
    AdditionalTools {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        role: String,
        tools: Vec<Value>,
    },
    Message {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        role: String,
        content: Vec<ContentItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<MessagePhase>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    AgentMessage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        author: String,
        recipient: String,
        content: Vec<AgentMessageInputContent>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    Reasoning {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default)]
        summary: Vec<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<Vec<Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    LocalShellCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        status: Value,
        action: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    FunctionCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        arguments: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted_function_args: Option<Vec<String>>,
        call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    ToolSearchCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        execution: String,
        arguments: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    FunctionCallOutput {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        output: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    CustomToolCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        call_id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        input: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    CustomToolCallOutput {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        output: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    ToolSearchOutput {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        status: String,
        execution: String,
        tools: Vec<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    WebSearchCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    ImageGenerationCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revised_prompt: Option<String>,
        result: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    #[serde(alias = "compaction_summary")]
    Compaction {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        encrypted_content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    CompactionTrigger {},
    ContextCompaction {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentItem {
    InputText {
        text: String,
    },
    InputImage {
        image_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
    InputAudio {
        audio_url: String,
    },
    OutputText {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessageInputContent {
    InputText { text: String },
    EncryptedContent { encrypted_content: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetail {
    Auto,
    Low,
    High,
    Original,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessagePhase {
    Commentary,
    FinalAnswer,
}
