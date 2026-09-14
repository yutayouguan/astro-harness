use serde_json::Value;
use tonic::{Request, Response, Status};

use super::AstroServiceImpl;

fn to_proto(attachment: agent_protocol::ThreadAttachment) -> proto::ThreadAttachment {
    proto::ThreadAttachment {
        id: attachment.id,
        thread_id: attachment.thread_id,
        attachment_type: attachment.attachment_type,
        identity_key: attachment.identity_key,
        payload_json: attachment.payload.to_string(),
        created_at: attachment.created_at,
    }
}

async fn store(service: &AstroServiceImpl) -> Result<session::SessionStore, Status> {
    memory::ensure_workspace(&service.memory_dir)
        .map_err(|error| Status::internal(error.to_string()))?;
    session::SessionStore::open_sessions_dir(&home::sessions_dir(&service.memory_dir))
        .await
        .map_err(|error| Status::internal(error.to_string()))
}

pub(crate) async fn add(
    service: &AstroServiceImpl,
    request: Request<proto::AddThreadAttachmentRequest>,
) -> Result<Response<proto::AddThreadAttachmentResponse>, Status> {
    let request = request.into_inner();
    let payload = serde_json::from_str::<Value>(&request.payload_json).map_err(|error| {
        Status::invalid_argument(format!("invalid attachment payload: {error}"))
    })?;
    let result = store(service)
        .await?
        .add_thread_attachment(
            &request.thread_id,
            &request.attachment_type,
            &request.identity_key,
            &payload,
        )
        .await
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
    Ok(Response::new(proto::AddThreadAttachmentResponse {
        outcome: match result.outcome {
            agent_protocol::ThreadAttachmentAddOutcome::Created => "created",
            agent_protocol::ThreadAttachmentAddOutcome::Existing => "existing",
        }
        .into(),
        attachment: Some(to_proto(result.attachment)),
    }))
}

pub(crate) async fn list(
    service: &AstroServiceImpl,
    request: Request<proto::ListThreadAttachmentsRequest>,
) -> Result<Response<proto::ListThreadAttachmentsResponse>, Status> {
    let request = request.into_inner();
    let limit = if request.limit == 0 {
        50
    } else {
        request.limit as usize
    };
    let cursor = (!request.cursor.is_empty()).then_some(request.cursor.as_str());
    let page = store(service)
        .await?
        .list_thread_attachments(&request.thread_id, cursor, limit)
        .await
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
    Ok(Response::new(proto::ListThreadAttachmentsResponse {
        data: page.data.into_iter().map(to_proto).collect(),
        next_cursor: page.next_cursor.unwrap_or_default(),
    }))
}

pub(crate) async fn remove(
    service: &AstroServiceImpl,
    request: Request<proto::RemoveThreadAttachmentRequest>,
) -> Result<Response<proto::RemoveThreadAttachmentResponse>, Status> {
    let request = request.into_inner();
    let removed = store(service)
        .await?
        .remove_thread_attachment(
            &request.thread_id,
            &request.attachment_type,
            &request.identity_key,
        )
        .await
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
    Ok(Response::new(proto::RemoveThreadAttachmentResponse {
        removed: removed.is_some(),
        attachment: removed.map(to_proto),
    }))
}
