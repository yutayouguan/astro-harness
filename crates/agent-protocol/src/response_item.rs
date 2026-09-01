use std::fmt;
use std::ops::Deref;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// A Responses API item ID. New synthetic IDs require an explicit prefix;
/// deserialization remains permissive for provider and legacy rollout IDs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResponseItemId(String);

impl ResponseItemId {
    pub fn new(prefix: &str) -> Self {
        Self::with_suffix(prefix, uuid::Uuid::now_v7())
    }

    pub fn with_suffix(prefix: &str, suffix: impl fmt::Display) -> Self {
        Self(format!("{prefix}_{suffix}"))
    }

    pub fn from_server(value: String) -> Self {
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_prefixed(&self) -> bool {
        self.split_once('_')
            .is_some_and(|(prefix, suffix)| !prefix.is_empty() && !suffix.is_empty())
    }
}

impl Deref for ResponseItemId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for ResponseItemId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ResponseItemId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<ResponseItemId> for String {
    fn from(value: ResponseItemId) -> Self {
        value.0
    }
}

impl From<String> for ResponseItemId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ResponseItemId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl PartialEq<str> for ResponseItemId {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for ResponseItemId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

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
        id: Option<ResponseItemId>,
        role: String,
        tools: Vec<Value>,
    },
    Message {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<ResponseItemId>,
        role: String,
        content: Vec<ContentItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<MessagePhase>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    AgentMessage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<ResponseItemId>,
        author: String,
        recipient: String,
        content: Vec<AgentMessageInputContent>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    Reasoning {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<ResponseItemId>,
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
        id: Option<ResponseItemId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        status: Value,
        action: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    FunctionCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<ResponseItemId>,
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
        id: Option<ResponseItemId>,
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
        id: Option<ResponseItemId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        output: FunctionCallOutputPayload,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    CustomToolCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<ResponseItemId>,
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
        id: Option<ResponseItemId>,
        call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        output: FunctionCallOutputPayload,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    ToolSearchOutput {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<ResponseItemId>,
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
        id: Option<ResponseItemId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    ImageGenerationCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<ResponseItemId>,
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
        id: Option<ResponseItemId>,
        encrypted_content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    CompactionTrigger {},
    ContextCompaction {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<ResponseItemId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        internal_chat_message_metadata_passthrough: Option<Value>,
    },
    #[serde(other)]
    Other,
}

impl ResponseItem {
    pub fn text_message(role: impl Into<String>, text: impl Into<String>) -> Self {
        let role = role.into();
        let content = if role == "assistant" {
            vec![ContentItem::OutputText { text: text.into() }]
        } else {
            vec![ContentItem::InputText { text: text.into() }]
        };
        Self::Message {
            id: None,
            role,
            content,
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
    }

    pub fn user_text(text: impl Into<String>) -> Self {
        Self::text_message("user", text)
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self::text_message("assistant", text)
    }

    pub fn developer_text(text: impl Into<String>) -> Self {
        Self::text_message("developer", text)
    }

    pub fn role(&self) -> Option<&str> {
        match self {
            Self::Message { role, .. } => Some(role),
            _ => None,
        }
    }

    pub fn text(&self) -> String {
        match self {
            Self::Message { content, .. } => content
                .iter()
                .filter_map(|part| match part {
                    ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                        Some(text.as_str())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Self::FunctionCallOutput { output, .. } | Self::CustomToolCallOutput { output, .. } => {
                output.to_text().unwrap_or_default()
            }
            Self::ToolSearchOutput { tools, .. } => {
                serde_json::to_string(tools).unwrap_or_default()
            }
            Self::Reasoning {
                content, summary, ..
            } => content
                .as_ref()
                .into_iter()
                .flatten()
                .chain(summary.iter())
                .filter_map(|value| value.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            Self::FunctionCall {
                name, arguments, ..
            } => format!("{name}: {arguments}"),
            Self::CustomToolCall { name, input, .. } => format!("{name}: {input}"),
            Self::ToolSearchCall { arguments, .. } => arguments.to_string(),
            _ => String::new(),
        }
    }

    pub fn text_content(&self) -> String {
        self.text()
    }

    pub fn content_str(&self) -> String {
        self.text()
    }

    pub fn compressed_text(&self) -> Option<&str> {
        self.metadata()?.get("astro_compressed_output")?.as_str()
    }

    pub fn provider_view_text(&self) -> String {
        self.compressed_text()
            .map(str::to_owned)
            .unwrap_or_else(|| self.text())
    }

    pub fn is_tool_output(&self) -> bool {
        matches!(
            self,
            Self::FunctionCallOutput { .. }
                | Self::CustomToolCallOutput { .. }
                | Self::ToolSearchOutput { .. }
        )
    }

    pub fn tool_name(&self) -> Option<&str> {
        match self {
            Self::FunctionCall { name, .. } | Self::CustomToolCall { name, .. } => Some(name),
            Self::FunctionCallOutput { name, .. } | Self::CustomToolCallOutput { name, .. } => {
                name.as_deref()
            }
            Self::ToolSearchCall { .. } | Self::ToolSearchOutput { .. } => Some("tool_search"),
            _ => None,
        }
    }

    pub fn call_id(&self) -> Option<&str> {
        match self {
            Self::FunctionCall { call_id, .. }
            | Self::CustomToolCall { call_id, .. }
            | Self::CustomToolCallOutput { call_id, .. } => Some(call_id),
            Self::FunctionCallOutput { call_id, .. }
            | Self::ToolSearchCall { call_id, .. }
            | Self::ToolSearchOutput { call_id, .. } => call_id.as_deref(),
            _ => None,
        }
    }

    pub fn metadata(&self) -> Option<&Value> {
        match self {
            Self::Message {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::AgentMessage {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::Reasoning {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::LocalShellCall {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::FunctionCall {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::ToolSearchCall {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::FunctionCallOutput {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::CustomToolCall {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::CustomToolCallOutput {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::ToolSearchOutput {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::WebSearchCall {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::ImageGenerationCall {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::Compaction {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::ContextCompaction {
                internal_chat_message_metadata_passthrough,
                ..
            } => internal_chat_message_metadata_passthrough.as_ref(),
            Self::AdditionalTools { .. } | Self::CompactionTrigger {} | Self::Other => None,
        }
    }

    pub fn metadata_mut(&mut self) -> Option<&mut Option<Value>> {
        match self {
            Self::Message {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::AgentMessage {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::Reasoning {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::LocalShellCall {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::FunctionCall {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::ToolSearchCall {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::FunctionCallOutput {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::CustomToolCall {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::CustomToolCallOutput {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::ToolSearchOutput {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::WebSearchCall {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::ImageGenerationCall {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::Compaction {
                internal_chat_message_metadata_passthrough,
                ..
            }
            | Self::ContextCompaction {
                internal_chat_message_metadata_passthrough,
                ..
            } => Some(internal_chat_message_metadata_passthrough),
            Self::AdditionalTools { .. } | Self::CompactionTrigger {} | Self::Other => None,
        }
    }
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

/// Responses-compatible structured content returned by a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FunctionCallOutputContentItem {
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
    EncryptedContent {
        encrypted_content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FunctionCallOutputBody {
    Text(String),
    ContentItems(Vec<FunctionCallOutputContentItem>),
}

impl Default for FunctionCallOutputBody {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

/// The model-facing output payload. `success` is local metadata and is not
/// serialized into the Responses API `output` field.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FunctionCallOutputPayload {
    pub body: FunctionCallOutputBody,
    pub success: Option<bool>,
}

impl FunctionCallOutputPayload {
    pub fn from_text(content: String) -> Self {
        Self {
            body: FunctionCallOutputBody::Text(content),
            success: None,
        }
    }

    pub fn from_content_items(content_items: Vec<FunctionCallOutputContentItem>) -> Self {
        Self {
            body: FunctionCallOutputBody::ContentItems(content_items),
            success: None,
        }
    }

    pub fn text_content(&self) -> Option<&str> {
        match &self.body {
            FunctionCallOutputBody::Text(content) => Some(content),
            FunctionCallOutputBody::ContentItems(_) => None,
        }
    }

    pub fn to_text(&self) -> Option<String> {
        match &self.body {
            FunctionCallOutputBody::Text(content) => Some(content.clone()),
            FunctionCallOutputBody::ContentItems(items) => {
                let text = items
                    .iter()
                    .filter_map(|item| match item {
                        FunctionCallOutputContentItem::InputText { text }
                            if !text.trim().is_empty() =>
                        {
                            Some(text.as_str())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                (!text.is_empty()).then_some(text)
            }
        }
    }
}

impl Serialize for FunctionCallOutputPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.body {
            FunctionCallOutputBody::Text(content) => serializer.serialize_str(content),
            FunctionCallOutputBody::ContentItems(items) => items.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for FunctionCallOutputPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self {
            body: FunctionCallOutputBody::deserialize(deserializer)?,
            success: None,
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_item_id_requires_prefix_only_when_created_locally() {
        let generated = ResponseItemId::new("msg");
        assert!(generated.as_str().starts_with("msg_"));
        assert!(generated.is_prefixed());

        let legacy: ResponseItemId = serde_json::from_str(r#""legacy""#).unwrap();
        assert_eq!(legacy.as_str(), "legacy");
        assert!(!legacy.is_prefixed());
    }

    #[test]
    fn function_output_payload_preserves_responses_wire_shape() {
        let text = FunctionCallOutputPayload {
            body: FunctionCallOutputBody::Text("done".into()),
            success: Some(true),
        };
        assert_eq!(
            serde_json::to_value(&text).unwrap(),
            serde_json::json!("done")
        );

        let structured = FunctionCallOutputPayload::from_content_items(vec![
            FunctionCallOutputContentItem::InputText {
                text: "caption".into(),
            },
            FunctionCallOutputContentItem::InputImage {
                image_url: "data:image/png;base64,AA==".into(),
                detail: Some(ImageDetail::Low),
            },
        ]);
        assert_eq!(
            serde_json::to_value(&structured).unwrap(),
            serde_json::json!([
                {"type": "input_text", "text": "caption"},
                {
                    "type": "input_image",
                    "image_url": "data:image/png;base64,AA==",
                    "detail": "low"
                }
            ])
        );
        assert_eq!(structured.to_text().as_deref(), Some("caption"));
    }
}
