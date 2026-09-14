use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Independently persisted state associated with one thread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAttachment {
    pub id: String,
    pub thread_id: String,
    pub attachment_type: String,
    pub identity_key: String,
    pub payload: Value,
    pub created_at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadAttachmentAddOutcome {
    Created,
    Existing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAttachmentAddResult {
    pub outcome: ThreadAttachmentAddOutcome,
    pub attachment: ThreadAttachment,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAttachmentPage {
    pub data: Vec<ThreadAttachment>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadAttachmentOperation {
    Created,
    Deleted,
}
