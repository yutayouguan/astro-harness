use quick_xml::de::from_str as from_xml_str;
use quick_xml::se::to_string as to_xml_string;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ContentItem, ResponseItem, ResponseItemId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextItem {
    pub id: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookPromptItem {
    pub id: String,
    pub fragments: Vec<HookPromptFragment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookPromptFragment {
    pub text: String,
    pub hook_run_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "hook_prompt")]
struct HookPromptXml {
    #[serde(rename = "@hook_run_id")]
    hook_run_id: String,
    #[serde(rename = "$text")]
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMessageDelivery {
    Async,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMessageItem {
    pub id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<AgentMessageDelivery>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolItem {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub output: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<types::MediaAsset>,
    pub status: ToolStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_mode: Option<ToolExecutionMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionMode {
    Serial,
    Parallel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    InProgress,
    Completed,
    Failed,
    Declined,
    Interrupted,
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
    HookPrompt(HookPromptItem),
    AgentMessage(AgentMessageItem),
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
            | Self::Plan(item)
            | Self::Reasoning(item)
            | Self::SubAgentActivity(item)
            | Self::ContextCompaction(item)
            | Self::EnteredReviewMode(item)
            | Self::ExitedReviewMode(item) => &item.id,
            Self::HookPrompt(item) => &item.id,
            Self::AgentMessage(item) => &item.id,
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

impl HookPromptItem {
    pub fn from_fragments(id: Option<&str>, fragments: Vec<HookPromptFragment>) -> Self {
        Self {
            id: id
                .map(str::to_string)
                .unwrap_or_else(|| ResponseItemId::new("msg").to_string()),
            fragments,
        }
    }
}

impl HookPromptFragment {
    pub fn from_single_hook(text: impl Into<String>, hook_run_id: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            hook_run_id: hook_run_id.into(),
        }
    }
}

pub fn build_hook_prompt_message(fragments: &[HookPromptFragment]) -> Option<ResponseItem> {
    let content = fragments
        .iter()
        .filter(|fragment| !fragment.hook_run_id.trim().is_empty())
        .filter_map(|fragment| {
            serialize_hook_prompt_fragment(&fragment.text, &fragment.hook_run_id)
                .map(|text| ContentItem::InputText { text })
        })
        .collect::<Vec<_>>();

    if content.is_empty() {
        return None;
    }

    Some(ResponseItem::Message {
        id: Some(ResponseItemId::new("msg")),
        role: "user".to_string(),
        content,
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    })
}

pub fn parse_hook_prompt_message(
    id: Option<&str>,
    content: &[ContentItem],
) -> Option<HookPromptItem> {
    let fragments = content
        .iter()
        .map(|content_item| {
            let ContentItem::InputText { text } = content_item else {
                return None;
            };
            parse_hook_prompt_fragment(text)
        })
        .collect::<Option<Vec<_>>>()?;

    if fragments.is_empty() {
        return None;
    }

    Some(HookPromptItem::from_fragments(id, fragments))
}

pub fn parse_hook_prompt_fragment(text: &str) -> Option<HookPromptFragment> {
    let HookPromptXml { text, hook_run_id } = from_xml_str::<HookPromptXml>(text.trim()).ok()?;
    if hook_run_id.trim().is_empty() {
        return None;
    }
    Some(HookPromptFragment { text, hook_run_id })
}

fn serialize_hook_prompt_fragment(text: &str, hook_run_id: &str) -> Option<String> {
    if hook_run_id.trim().is_empty() {
        return None;
    }
    to_xml_string(&HookPromptXml {
        text: text.to_string(),
        hook_run_id: hook_run_id.to_string(),
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_status_roundtrips_codex_aligned_terminal_states() {
        for (status, wire) in [
            (ToolStatus::InProgress, "\"in_progress\""),
            (ToolStatus::Completed, "\"completed\""),
            (ToolStatus::Failed, "\"failed\""),
            (ToolStatus::Declined, "\"declined\""),
            (ToolStatus::Interrupted, "\"interrupted\""),
        ] {
            assert_eq!(serde_json::to_string(&status).unwrap(), wire);
            assert_eq!(serde_json::from_str::<ToolStatus>(wire).unwrap(), status);
        }
    }

    #[test]
    fn hook_prompt_roundtrips_multiple_fragments() {
        let original = vec![
            HookPromptFragment::from_single_hook("Retry with care & joy.", "hook-run-1"),
            HookPromptFragment::from_single_hook("Then summarize cleanly.", "hook-run-2"),
        ];
        let message = build_hook_prompt_message(&original).expect("hook prompt");

        let ResponseItem::Message { id, content, .. } = message else {
            panic!("expected hook prompt message");
        };
        assert!(id.as_ref().is_some_and(|id| id.starts_with("msg_")));
        let parsed = parse_hook_prompt_message(id.as_deref(), &content).expect("hook prompt");
        assert_eq!(parsed.id, id.unwrap().to_string());
        assert_eq!(parsed.fragments, original);
    }

    #[test]
    fn hook_prompt_parser_rejects_mixed_or_unattributed_content() {
        assert!(parse_hook_prompt_fragment("normal user text").is_none());
        assert!(parse_hook_prompt_fragment("<hook_prompt>retry</hook_prompt>").is_none());
        assert!(parse_hook_prompt_message(
            None,
            &[
                ContentItem::InputText {
                    text: "<hook_prompt hook_run_id=\"run-1\">retry</hook_prompt>".into(),
                },
                ContentItem::InputText {
                    text: "ordinary text".into(),
                },
            ],
        )
        .is_none());
    }
}
