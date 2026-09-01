use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AgentMessageInputContent, ResponseItem, ResponseItemId};

/// Persistent settings published in submission order for subsequent turns.
///
/// The custom `Debug` implementation deliberately omits provider credentials.
#[derive(Clone, Default, PartialEq)]
pub struct ThreadSettingsOverrides {
    pub model_targets: Option<Vec<types::ModelTarget>>,
    pub auxiliary_targets: Option<HashMap<types::AuxiliaryTask, Vec<types::ModelTarget>>>,
    pub image_gen_targets: Option<types::ImageGenTargets>,
    pub context_window: Option<u32>,
    pub interaction_mode: Option<types::InteractionMode>,
    /// `None` preserves the current root; `Some(None)` clears it.
    pub project_root: Option<Option<PathBuf>>,
    pub workspace_roots: Option<Vec<PathBuf>>,
    pub temperature: Option<f32>,
    pub additional_params: Option<Value>,
    pub thinking_enabled: Option<bool>,
    pub reasoning_effort: Option<String>,
    pub max_tokens: Option<u32>,
}

impl ThreadSettingsOverrides {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

impl fmt::Debug for ThreadSettingsOverrides {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThreadSettingsOverrides")
            .field(
                "model_targets",
                &self.model_targets.as_ref().map(|targets| targets.len()),
            )
            .field(
                "auxiliary_targets",
                &self.auxiliary_targets.as_ref().map(HashMap::len),
            )
            .field("image_gen_targets", &self.image_gen_targets.is_some())
            .field("context_window", &self.context_window)
            .field("interaction_mode", &self.interaction_mode)
            .field("project_root", &self.project_root)
            .field("workspace_roots", &self.workspace_roots)
            .field("temperature", &self.temperature)
            .field("additional_params", &self.additional_params.is_some())
            .field("thinking_enabled", &self.thinking_enabled)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

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

/// User decision returned to an MCP server for an `elicitation/create` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElicitationAction {
    Accept,
    Decline,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElicitationResponse {
    pub action: ElicitationAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// Settings that can change the next sampling step of one active turn.
///
/// Nested options distinguish "leave unchanged" from "clear the current value".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TurnSettingsUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_summary: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<Option<String>>,
}

impl TurnSettingsUpdate {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TurnSettingsOutcome {
    Applied,
    TargetUnavailable { message: String },
    Rejected { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserShellLaunch {
    pub turn_id: String,
    pub item_id: String,
    pub attached_to_active_turn: bool,
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
    fn turn_settings_preserve_clear_vs_unchanged() {
        let unchanged = TurnSettingsUpdate::default();
        let explicit_clear = TurnSettingsUpdate {
            reasoning_effort: Some(None),
            ..Default::default()
        };
        assert_eq!(unchanged.reasoning_effort, None);
        assert_eq!(explicit_clear.reasoning_effort, Some(None));
    }

    #[test]
    fn elicitation_response_roundtrips_action_and_payload() {
        let response = ElicitationResponse {
            action: ElicitationAction::Accept,
            content: Some(serde_json::json!({"name":"Ada"})),
            meta: None,
        };
        let restored: ElicitationResponse =
            serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
        assert_eq!(restored, response);
    }

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
