use rmcp::model::{ElicitResult, ElicitationAction, ErrorData, Meta};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq)]
pub struct McpElicitationRequest {
    pub server_name: String,
    pub request_id: String,
    pub params: Value,
    pub generation: u64,
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

const MAX_PENDING_ELICITATIONS: usize = 128;

struct PendingEntry {
    token: u64,
    sender: oneshot::Sender<McpElicitationResponse>,
    params: Value,
    turn_id: Option<String>,
}

struct PendingCleanup<'a> {
    pending: &'a Mutex<HashMap<PendingKey, PendingEntry>>,
    key: PendingKey,
    token: u64,
}

impl Drop for PendingCleanup<'_> {
    fn drop(&mut self) {
        let mut pending = self.pending.lock().expect("MCP elicitation mutex poisoned");
        if pending
            .get(&self.key)
            .is_some_and(|entry| entry.token == self.token)
        {
            pending.remove(&self.key);
            types::pending_interaction::changed();
        }
    }
}

/// Correlates MCP server callbacks with asynchronous desktop responses.
pub struct McpElicitationBroker {
    request_tx: async_channel::Sender<McpElicitationRequest>,
    request_rx: async_channel::Receiver<McpElicitationRequest>,
    pending: Mutex<HashMap<PendingKey, PendingEntry>>,
    next_token: AtomicU64,
}

impl Default for McpElicitationBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl McpElicitationBroker {
    pub fn new() -> Self {
        Self::with_capacity(MAX_PENDING_ELICITATIONS)
    }

    fn with_capacity(capacity: usize) -> Self {
        let (request_tx, request_rx) = async_channel::bounded(capacity);
        Self {
            request_tx,
            request_rx,
            pending: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(1),
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
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        {
            let mut pending = self.pending.lock().expect("MCP elicitation mutex poisoned");
            if pending.contains_key(&key) {
                return Err(ErrorData::internal_error(
                    "duplicate MCP elicitation request id",
                    None,
                ));
            }
            if pending.len() >= self.request_tx.capacity().unwrap_or(0) {
                return Err(ErrorData::internal_error(
                    "too many pending MCP elicitation requests",
                    None,
                ));
            }
            pending.insert(
                key.clone(),
                PendingEntry {
                    token,
                    sender: tx,
                    params: params.clone(),
                    turn_id: None,
                },
            );
        }
        types::pending_interaction::changed();
        let _cleanup = PendingCleanup {
            pending: &self.pending,
            key: key.clone(),
            token,
        };
        if self
            .request_tx
            .try_send(McpElicitationRequest {
                server_name,
                request_id,
                params,
                generation: token,
            })
            .is_err()
        {
            return Err(ErrorData::internal_error(
                "MCP elicitation UI channel is closed or full",
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
        let resolved = self
            .pending
            .lock()
            .expect("MCP elicitation mutex poisoned")
            .remove(&(server_name.to_string(), request_id.to_string()))
            .is_some_and(|entry| entry.sender.send(response).is_ok());
        types::pending_interaction::changed();
        resolved
    }

    pub fn pending_requests(&self) -> Vec<(McpElicitationRequest, Option<String>)> {
        self.pending
            .lock()
            .expect("MCP elicitation mutex poisoned")
            .iter()
            .map(|((server_name, request_id), entry)| {
                (
                    McpElicitationRequest {
                        server_name: server_name.clone(),
                        request_id: request_id.clone(),
                        params: entry.params.clone(),
                        generation: entry.token,
                    },
                    entry.turn_id.clone(),
                )
            })
            .collect()
    }

    pub fn bind_turn(&self, server: &str, id: &str, generation: u64, turn_id: String) {
        if let Some(entry) = self
            .pending
            .lock()
            .expect("MCP elicitation mutex poisoned")
            .get_mut(&(server.into(), id.into()))
        {
            if entry.token == generation {
                entry.turn_id = Some(turn_id);
                types::pending_interaction::changed();
            }
        }
    }

    pub fn resolve_generation(
        &self,
        server: &str,
        id: &str,
        token: u64,
        response: McpElicitationResponse,
    ) -> bool {
        let mut pending = self.pending.lock().expect("MCP elicitation mutex poisoned");
        let key = (server.to_string(), id.to_string());
        if !pending.get(&key).is_some_and(|entry| entry.token == token) {
            return false;
        }
        let resolved = pending
            .remove(&key)
            .is_some_and(|entry| entry.sender.send(response).is_ok());
        types::pending_interaction::changed();
        resolved
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

    #[tokio::test]
    async fn pending_requests_are_bounded_even_after_the_ui_receives_them() {
        let broker = Arc::new(McpElicitationBroker::with_capacity(1));
        let requests = broker.requests();
        let first = {
            let broker = Arc::clone(&broker);
            tokio::spawn(async move {
                broker
                    .request(
                        "server".into(),
                        "first".into(),
                        serde_json::json!({}),
                        CancellationToken::new(),
                    )
                    .await
            })
        };
        requests.recv().await.unwrap();

        let second = broker
            .request(
                "server".into(),
                "second".into(),
                serde_json::json!({}),
                CancellationToken::new(),
            )
            .await;
        assert!(second.is_err());
        assert!(
            broker
                .resolve(
                    "server",
                    "first",
                    McpElicitationResponse {
                        action: McpElicitationAction::Cancel,
                        content: None,
                        meta: None,
                    },
                )
                .await
        );
        assert_eq!(
            first.await.unwrap().unwrap().action,
            ElicitationAction::Cancel
        );
    }

    #[test]
    fn stale_cleanup_does_not_remove_a_reused_request_id() {
        let pending = Mutex::new(HashMap::new());
        let key = ("server".to_string(), "request".to_string());
        let (old_sender, _old_receiver) = oneshot::channel();
        pending.lock().unwrap().insert(
            key.clone(),
            PendingEntry {
                token: 1,
                sender: old_sender,
                params: Value::Null,
                turn_id: None,
            },
        );
        let cleanup = PendingCleanup {
            pending: &pending,
            key: key.clone(),
            token: 1,
        };
        let (new_sender, _new_receiver) = oneshot::channel();
        pending.lock().unwrap().insert(
            key.clone(),
            PendingEntry {
                token: 2,
                sender: new_sender,
                params: Value::Null,
                turn_id: None,
            },
        );

        drop(cleanup);

        assert_eq!(pending.lock().unwrap().get(&key).unwrap().token, 2);
    }
}
