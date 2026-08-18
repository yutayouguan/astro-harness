use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextItem {
    pub id: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolItem {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub output: Option<Value>,
    pub status: ToolStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionItem {
    pub id: String,
    pub namespace: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum TurnItem {
    UserMessage(TextItem),
    HookPrompt(TextItem),
    AgentMessage(TextItem),
    Plan(TextItem),
    Reasoning(TextItem),
    CommandExecution(ToolItem),
    DynamicToolCall(ToolItem),
    McpToolCall(ToolItem),
    CollabAgentToolCall(ToolItem),
    SubAgentActivity(TextItem),
    WebSearch(ToolItem),
    ImageView(ToolItem),
    ImageGeneration(ToolItem),
    FileChange(ToolItem),
    ContextCompaction(TextItem),
    EnteredReviewMode(TextItem),
    ExitedReviewMode(TextItem),
    Extension(ExtensionItem),
}

impl TurnItem {
    pub fn id(&self) -> &str {
        match self {
            Self::UserMessage(item)
            | Self::HookPrompt(item)
            | Self::AgentMessage(item)
            | Self::Plan(item)
            | Self::Reasoning(item)
            | Self::SubAgentActivity(item)
            | Self::ContextCompaction(item)
            | Self::EnteredReviewMode(item)
            | Self::ExitedReviewMode(item) => &item.id,
            Self::CommandExecution(item)
            | Self::DynamicToolCall(item)
            | Self::McpToolCall(item)
            | Self::CollabAgentToolCall(item)
            | Self::WebSearch(item)
            | Self::ImageView(item)
            | Self::ImageGeneration(item)
            | Self::FileChange(item) => &item.id,
            Self::Extension(item) => &item.id,
        }
    }
}
