use proto::astro_service_client::AstroServiceClient;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::commands::common::friendly_error;
use crate::infra::grpc::{default_grpc_address, endpoint_url};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAttachmentDto {
    pub id: String,
    pub thread_id: String,
    pub attachment_type: String,
    pub identity_key: String,
    pub payload: Value,
    pub created_at: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAttachmentAddDto {
    pub outcome: String,
    pub attachment: ThreadAttachmentDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAttachmentPageDto {
    pub data: Vec<ThreadAttachmentDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadAttachmentChangedDto {
    thread_id: String,
    attachment_type: String,
    identity_key: String,
    attachment_id: String,
    operation: String,
}

fn decode(attachment: proto::ThreadAttachment) -> Result<ThreadAttachmentDto, String> {
    Ok(ThreadAttachmentDto {
        id: attachment.id,
        thread_id: attachment.thread_id,
        attachment_type: attachment.attachment_type,
        identity_key: attachment.identity_key,
        payload: serde_json::from_str(&attachment.payload_json).map_err(|e| e.to_string())?,
        created_at: attachment.created_at,
    })
}

async fn client() -> Result<AstroServiceClient<tonic::transport::Channel>, String> {
    AstroServiceClient::connect(endpoint_url(&default_grpc_address()))
        .await
        .map_err(|error| friendly_error(&error.to_string()))
}

#[tauri::command]
pub async fn add_thread_attachment(
    app: AppHandle,
    thread_id: String,
    attachment_type: String,
    identity_key: String,
    payload: Value,
) -> Result<ThreadAttachmentAddDto, String> {
    let mut client = client().await?;
    let response = client
        .add_thread_attachment(proto::AddThreadAttachmentRequest {
            thread_id: thread_id.clone(),
            attachment_type: attachment_type.clone(),
            identity_key: identity_key.clone(),
            payload_json: payload.to_string(),
        })
        .await
        .map_err(|error| error.message().to_string())?
        .into_inner();
    let attachment = decode(response.attachment.ok_or("missing attachment response")?)?;
    if response.outcome == "created" {
        let _ = app.emit(
            "thread-attachments-changed",
            ThreadAttachmentChangedDto {
                thread_id,
                attachment_type,
                identity_key,
                attachment_id: attachment.id.clone(),
                operation: "created".into(),
            },
        );
    }
    Ok(ThreadAttachmentAddDto {
        outcome: response.outcome,
        attachment,
    })
}

#[tauri::command]
pub async fn list_thread_attachments(
    thread_id: String,
    cursor: Option<String>,
    limit: Option<u32>,
) -> Result<ThreadAttachmentPageDto, String> {
    let mut client = client().await?;
    let response = client
        .list_thread_attachments(proto::ListThreadAttachmentsRequest {
            thread_id,
            cursor: cursor.unwrap_or_default(),
            limit: limit.unwrap_or(50),
        })
        .await
        .map_err(|error| error.message().to_string())?
        .into_inner();
    Ok(ThreadAttachmentPageDto {
        data: response
            .data
            .into_iter()
            .map(decode)
            .collect::<Result<Vec<_>, _>>()?,
        next_cursor: (!response.next_cursor.is_empty()).then_some(response.next_cursor),
    })
}

#[tauri::command]
pub async fn remove_thread_attachment(
    app: AppHandle,
    thread_id: String,
    attachment_type: String,
    identity_key: String,
) -> Result<bool, String> {
    let mut client = client().await?;
    let response = client
        .remove_thread_attachment(proto::RemoveThreadAttachmentRequest {
            thread_id: thread_id.clone(),
            attachment_type: attachment_type.clone(),
            identity_key: identity_key.clone(),
        })
        .await
        .map_err(|error| error.message().to_string())?
        .into_inner();
    if response.removed {
        let attachment = response.attachment.ok_or("missing removed attachment")?;
        let _ = app.emit(
            "thread-attachments-changed",
            ThreadAttachmentChangedDto {
                thread_id,
                attachment_type,
                identity_key,
                attachment_id: attachment.id,
                operation: "deleted".into(),
            },
        );
    }
    Ok(response.removed)
}
