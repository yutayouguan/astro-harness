//! Batch API 与模型目录 Tauri 命令：批量请求创建/查询、OpenRouter 模型目录。

use proto::astro_service_client::AstroServiceClient;

use crate::infra::grpc::{default_grpc_address, endpoint_url};
use super::common::friendly_error;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// Keep unused import quiet for FileListRequest if we later wire gRPC ListFiles
#[allow(dead_code)]
/// 构造 proto FileListRequest（内部辅助）。
fn _proto_file_list_request() -> proto::FileListRequest {
    proto::FileListRequest {
        path: String::new(),
        depth: 1,
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn create_batch(requests_json: String) -> Result<String, String> {
    let grpc_address = default_grpc_address();
    let endpoint = endpoint_url(&grpc_address);
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|e| friendly_error(&e.to_string()))?;
    let result = client
        .create_batch(proto::BatchCreateRequest {
            agent_id: String::new(),
            requests_json,
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(result.response_json)
}

#[tauri::command]
pub async fn get_batch(batch_id: String) -> Result<String, String> {
    let grpc_address = default_grpc_address();
    let endpoint = endpoint_url(&grpc_address);
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|e| friendly_error(&e.to_string()))?;
    let result = client
        .get_batch(proto::BatchStatusRequest {
            agent_id: String::new(),
            batch_id,
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(result.response_json)
}

#[tauri::command]
pub async fn list_batches() -> Result<String, String> {
    let grpc_address = default_grpc_address();
    let endpoint = endpoint_url(&grpc_address);
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|e| friendly_error(&e.to_string()))?;
    let result = client
        .list_batches(proto::BatchListRequest {
            agent_id: String::new(),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(result.response_json)
}

#[tauri::command]
pub async fn get_batch_results(batch_id: String) -> Result<String, String> {
    let grpc_address = default_grpc_address();
    let endpoint = endpoint_url(&grpc_address);
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|e| friendly_error(&e.to_string()))?;
    let result = client
        .get_batch_results(proto::BatchStatusRequest {
            agent_id: String::new(),
            batch_id,
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();
    Ok(result.results_jsonl)
}

/// 返回 OpenRouter 全量模型目录（缓存刷新后读取）。
#[tauri::command]
pub async fn list_model_catalog(
    force_refresh: bool,
) -> Result<Vec<crate::meta::openrouter_meta::ModelCatalogEntry>, String> {
    crate::meta::openrouter_meta::ensure_cache(force_refresh).await?;
    Ok(crate::meta::openrouter_meta::all_entries())
}
