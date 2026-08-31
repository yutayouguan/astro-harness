use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AgentMessageInputContent, ResponseItem, ResponseItemId};

/// User decision for an execution or patch approval request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Approved,
    ApprovedExecpolicyAmendment {
        proposed_execpolicy_amendment: Value,
    },
    ApprovedForSession,
    ApprovedMcpPolicyAmendment,
    NetworkPolicyAmendment {
        network_policy_amendment: Value,
    },
    Denied {
        rejection: String,
    },
    TimedOut,
    Abort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestUserInputAnswer {
    pub answers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestUserInputResponse {
    pub answers: HashMap<String, RequestUserInputAnswer>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionGrantScope {
    #[default]
    Turn,
    Session,
}

/// Permission fields stay provider-neutral in the shared protocol while preserving Codex's
/// typed response envelope and unknown-field rejection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestPermissionProfile {
    pub network: Option<Value>,
    pub file_system: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestPermissionsResponse {
    pub permissions: RequestPermissionProfile,
    #[serde(default)]
    pub scope: PermissionGrantScope,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strict_auto_review: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicToolResponse {
    pub content_items: Vec<DynamicToolCallOutputContentItem>,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DynamicToolCallOutputContentItem {
    #[serde(rename_all = "camelCase")]
    InputText { text: String },
    #[serde(rename_all = "camelCase")]
    InputImage {
        #[serde(rename = "imageUrl")]
        image_url: String,
    },
    #[serde(rename_all = "camelCase")]
    InputAudio {
        #[serde(rename = "audioUrl")]
        audio_url: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ReviewTarget {
    UncommittedChanges,
    BaseBranch { branch: String },
    Commit { sha: String, title: Option<String> },
    Custom { instructions: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRequest {
    pub target: ReviewTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_facing_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterAgentCommunication {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ResponseItemId>,
    pub author: String,
    pub recipient: String,
    #[serde(default)]
    pub other_recipients: Vec<String>,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internal_chat_message_metadata_passthrough: Option<Value>,
    pub trigger_turn: bool,
}

impl InterAgentCommunication {
    pub fn model_input_text(&self) -> Option<String> {
        (!self.content.trim().is_empty()).then(|| {
            format!(
                "<inter_agent_message from=\"{}\" to=\"{}\">\n{}\n</inter_agent_message>",
                self.author, self.recipient, self.content
            )
        })
    }

    pub fn to_model_input_item(&self) -> ResponseItem {
        let content = match &self.encrypted_content {
            Some(encrypted_content) => {
                let message_type = if self.trigger_turn {
                    "NEW_TASK"
                } else {
                    "MESSAGE"
                };
                vec![
                    AgentMessageInputContent::InputText {
                        text: format!(
                            "Message Type: {message_type}\nTask name: {}\nSender: {}\nPayload:\n",
                            self.recipient, self.author
                        ),
                    },
                    AgentMessageInputContent::EncryptedContent {
                        encrypted_content: encrypted_content.clone(),
                    },
                ]
            }
            None => vec![AgentMessageInputContent::InputText {
                text: self.content.clone(),
            }],
        };
        ResponseItem::AgentMessage {
            id: self.id.clone(),
            author: self.author.clone(),
            recipient: self.recipient.clone(),
            content,
            internal_chat_message_metadata_passthrough: self
                .internal_chat_message_metadata_passthrough
                .clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_payloads_use_codex_wire_shapes() {
        assert_eq!(
            serde_json::to_value(ReviewDecision::ApprovedForSession).unwrap(),
            serde_json::json!("approved_for_session")
        );
        assert_eq!(
            serde_json::to_value(ReviewTarget::BaseBranch {
                branch: "main".into()
            })
            .unwrap(),
            serde_json::json!({ "type": "baseBranch", "branch": "main" })
        );
        assert_eq!(
            serde_json::to_value(DynamicToolCallOutputContentItem::InputImage {
                image_url: "data:image/png;base64,AA==".into()
            })
            .unwrap(),
            serde_json::json!({
                "type": "inputImage",
                "imageUrl": "data:image/png;base64,AA=="
            })
        );
    }

    #[test]
    fn inter_agent_communication_keeps_structured_and_encrypted_input() {
        let communication = InterAgentCommunication {
            id: Some(ResponseItemId::with_suffix("mail", "7")),
            author: "/root/worker".into(),
            recipient: "/root".into(),
            other_recipients: Vec::new(),
            content: String::new(),
            encrypted_content: Some("ciphertext".into()),
            internal_chat_message_metadata_passthrough: None,
            trigger_turn: true,
        };

        assert!(communication.model_input_text().is_none());
        assert!(matches!(
            communication.to_model_input_item(),
            ResponseItem::AgentMessage { id: Some(id), author, recipient, content, .. }
                if id == "mail_7"
                    && author == "/root/worker"
                    && recipient == "/root"
                    && matches!(content.as_slice(), [
                        AgentMessageInputContent::InputText { text },
                        AgentMessageInputContent::EncryptedContent { encrypted_content }
                    ] if text.contains("Message Type: NEW_TASK") && encrypted_content == "ciphertext")
        ));
    }
}
