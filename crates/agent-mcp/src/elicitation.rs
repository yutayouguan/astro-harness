use rmcp::model::{ElicitResult, ElicitationAction, ErrorData, Meta};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq)]
pub struct McpElicitationRequest {
    pub server_name: String,
    pub request_id: String,
    pub params: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpElicitationAction {
    Accept,
    Decline,
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpElicitationResponse {
    pub action: McpElicitationAction,
    pub content: Option<Value>,
    pub meta: Option<Value>,
}

type PendingKey = (String, String);

struct PendingCleanup<'a> {
    pending: &'a Mutex<HashMap<PendingKey, oneshot::Sender<McpElicitationResponse>>>,
    key: PendingKey,
}

impl Drop for PendingCleanup<'_> {
    fn drop(&mut self) {
        self.pending
            .lock()
            .expect("MCP elicitation mutex poisoned")
            .remove(&self.key);
    }
}

/// Correlates MCP server callbacks with asynchronous desktop responses.
pub struct McpElicitationBroker {
    request_tx: async_channel::Sender<McpElicitationRequest>,
    request_rx: async_channel::Receiver<McpElicitationRequest>,
    pending: Mutex<HashMap<PendingKey, oneshot::Sender<McpElicitationResponse>>>,
}

impl Default for McpElicitationBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl McpElicitationBroker {
    pub fn new() -> Self {
        let (request_tx, request_rx) = async_channel::unbounded();
        Self {
            request_tx,
            request_rx,
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn requests(&self) -> async_channel::Receiver<McpElicitationRequest> {
        self.request_rx.clone()
    }

    pub async fn request(
        &self,
        server_name: String,
        request_id: String,
        params: Value,
        cancellation: CancellationToken,
    ) -> Result<ElicitResult, ErrorData> {
        let key = (server_name.clone(), request_id.clone());
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().expect("MCP elicitation mutex poisoned");
            if pending.contains_key(&key) {
                return Err(ErrorData::internal_error(
                    "duplicate MCP elicitation request id",
                    None,
                ));
            }
            pending.insert(key.clone(), tx);
        }
        let _cleanup = PendingCleanup {
            pending: &self.pending,
            key: key.clone(),
        };
        if self
            .request_tx
            .send(McpElicitationRequest {
                server_name,
                request_id,
                params,
            })
            .await
            .is_err()
        {
            return Err(ErrorData::internal_error(
                "MCP elicitation UI channel is closed",
                None,
            ));
        }

        let response = tokio::select! {
            _ = cancellation.cancelled() => {
                return Err(ErrorData::internal_error("MCP elicitation was cancelled", None));
            }
            response = rx => response.map_err(|_| {
                ErrorData::internal_error("MCP elicitation response channel closed", None)
            })?,
        };
        let action = match response.action {
            McpElicitationAction::Accept => ElicitationAction::Accept,
            McpElicitationAction::Decline => ElicitationAction::Decline,
            McpElicitationAction::Cancel => ElicitationAction::Cancel,
        };
        let content = if action == ElicitationAction::Accept {
            Some(response.content.unwrap_or_else(|| serde_json::json!({})))
        } else {
            None
        };
        let meta = response
            .meta
            .map(serde_json::from_value::<Meta>)
            .transpose()
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
        let mut result = ElicitResult::new(action);
        if let Some(content) = content {
            result = result.with_content(content);
        }
        if let Some(meta) = meta {
            result = result.with_meta(meta);
        }
        Ok(result)
    }

    pub async fn resolve(
        &self,
        server_name: &str,
        request_id: &str,
        response: McpElicitationResponse,
    ) -> bool {
        self.pending
            .lock()
            .expect("MCP elicitation mutex poisoned")
            .remove(&(server_name.to_string(), request_id.to_string()))
            .is_some_and(|sender| sender.send(response).is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn accepted_response_defaults_content_to_empty_object() {
        let broker = Arc::new(McpElicitationBroker::new());
        let requests = broker.requests();
        let pending = {
            let broker = Arc::clone(&broker);
            tokio::spawn(async move {
                broker
                    .request(
                        "server".into(),
                        "7".into(),
                        serde_json::json!({"message":"name"}),
                        CancellationToken::new(),
                    )
                    .await
                    .unwrap()
            })
        };
        let request = requests.recv().await.unwrap();
        assert_eq!(request.request_id, "7");
        assert!(
            broker
                .resolve(
                    "server",
                    "7",
                    McpElicitationResponse {
                        action: McpElicitationAction::Accept,
                        content: None,
                        meta: None,
                    },
                )
                .await
        );
        assert_eq!(pending.await.unwrap().content, Some(serde_json::json!({})));
    }

    #[tokio::test]
    async fn duplicate_request_does_not_replace_the_original_waiter() {
        let broker = Arc::new(McpElicitationBroker::new());
        let requests = broker.requests();
        let first = {
            let broker = Arc::clone(&broker);
            tokio::spawn(async move {
                broker
                    .request(
                        "server".into(),
                        "7".into(),
                        serde_json::json!({}),
                        CancellationToken::new(),
                    )
                    .await
            })
        };
        requests.recv().await.unwrap();
        let duplicate = broker
            .request(
                "server".into(),
                "7".into(),
                serde_json::json!({}),
                CancellationToken::new(),
            )
            .await;
        assert!(duplicate.is_err());
        assert!(
            broker
                .resolve(
                    "server",
                    "7",
                    McpElicitationResponse {
                        action: McpElicitationAction::Decline,
                        content: None,
                        meta: None,
                    },
                )
                .await
        );
        assert_eq!(
            first.await.unwrap().unwrap().action,
            ElicitationAction::Decline
        );
    }
}
