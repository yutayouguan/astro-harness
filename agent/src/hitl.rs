//! Hermes 风格 HITL 阻塞闸门：工具执行路径 park，resume 完成 oneshot。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{oneshot, Mutex, RwLock};
use uuid::Uuid;

use crate::interrupt::{Interrupt, ResumeItem};
use crate::schema_validate::validate_against_schema;

/// 默认等待用户响应超时（秒），对齐 Hermes clarify。
pub const HITL_DEFAULT_TIMEOUT_SECS: u64 = 600;

/// 一次 HITL 请求（A2UI + schema）。
#[derive(Debug, Clone)]
pub struct HitlRequest {
    pub tool_call_id: String,
    pub reason: String,
    pub message: String,
    pub operations: Value,
    pub response_schema: Value,
    pub timeout: Duration,
}

impl HitlRequest {
    pub fn with_defaults(
        tool_call_id: String,
        reason: String,
        message: String,
        operations: Value,
        response_schema: Value,
    ) -> Self {
        Self {
            tool_call_id,
            reason,
            message,
            operations,
            response_schema,
            timeout: Duration::from_secs(HITL_DEFAULT_TIMEOUT_SECS),
        }
    }
}

/// 用户决议。
#[derive(Debug, Clone)]
pub struct HitlResolution {
    pub interrupt_id: String,
    /// `resolved` | `cancelled` | `timeout`
    pub status: String,
    pub payload_json: String,
}

impl HitlResolution {
    /// 写入会话的 tool result 文本。
    pub fn to_tool_result(&self) -> String {
        match self.status.as_str() {
            "timeout" => {
                "User did not respond to the confirmation/clarification within the timeout. Proceed with a reasonable default or ask again later.".to_string()
            }
            "cancelled" => {
                "User cancelled the confirmation/clarification. Do not proceed with the pending action.".to_string()
            }
            _ => {
                if self.payload_json.trim().is_empty() {
                    r#"{"status":"resolved"}"#.to_string()
                } else {
                    self.payload_json.clone()
                }
            }
        }
    }
}

struct Waiting {
    tx: oneshot::Sender<HitlResolution>,
    interrupt: Interrupt,
}

/// 单会话活闸门；可并存多个等待项（同批多 HITL）。
pub struct HitlGate {
    session_id: String,
    waiting: Mutex<HashMap<String, Waiting>>,
}

impl HitlGate {
    pub fn new(session_id: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            session_id: session_id.into(),
            waiting: Mutex::new(HashMap::new()),
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 当前未决 Interrupt 列表（供 UI / interrupt.json）。
    pub async fn pending_interrupts(&self) -> Vec<Interrupt> {
        self.waiting
            .lock()
            .await
            .values()
            .map(|w| w.interrupt.clone())
            .collect()
    }

    pub async fn is_waiting(&self) -> bool {
        !self.waiting.lock().await.is_empty()
    }

    /// 注册等待并阻塞至 resume / cancel / timeout。返回 (interrupt 快照, resolution)。
    pub async fn request(&self, req: HitlRequest) -> (Interrupt, HitlResolution) {
        let id = Uuid::new_v4().to_string();
        let interrupt = Interrupt {
            id: id.clone(),
            reason: req.reason.clone(),
            message: req.message.clone(),
            tool_call_id: req.tool_call_id.clone(),
            response_schema_json: req.response_schema.to_string(),
            expires_at: String::new(),
            metadata_json: String::new(),
        };
        let rx = self.begin_wait(interrupt.clone()).await;
        let resolution = self.finish_wait(&id, rx, req.timeout).await;
        (interrupt, resolution)
    }

    /// 预注册 interrupt 并返回 receiver，便于先 emit 再 await。
    /// **不**取消其它未决等待（支持同批多 HITL）。
    pub async fn begin_wait(&self, interrupt: Interrupt) -> oneshot::Receiver<HitlResolution> {
        let (tx, rx) = oneshot::channel();
        let id = interrupt.id.clone();
        self.waiting
            .lock()
            .await
            .insert(id, Waiting { tx, interrupt });
        rx
    }

    pub async fn finish_wait(
        &self,
        interrupt_id: &str,
        rx: oneshot::Receiver<HitlResolution>,
        timeout: Duration,
    ) -> HitlResolution {
        let resolution = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => HitlResolution {
                interrupt_id: interrupt_id.to_string(),
                status: "cancelled".into(),
                payload_json: String::new(),
            },
            Err(_) => HitlResolution {
                interrupt_id: interrupt_id.to_string(),
                status: "timeout".into(),
                payload_json: String::new(),
            },
        };
        self.waiting.lock().await.remove(interrupt_id);
        resolution
    }

    /// 由 `interrupt_resume` 完成等待；对 `resolved` 做 schema 校验。
    pub async fn resolve(&self, items: &[ResumeItem]) -> Result<(), String> {
        if items.is_empty() {
            return Err("resume 列表为空".into());
        }
        let mut map = self.waiting.lock().await;
        if map.is_empty() {
            return Err("当前没有等待中的 HITL".into());
        }

        // 先全部校验再发送，避免部分完成
        let mut prepared = Vec::with_capacity(items.len());
        for item in items {
            if item.status != "resolved" && item.status != "cancelled" {
                return Err(format!("invalid resume status: {}", item.status));
            }
            let Some(waiting) = map.get(&item.interrupt_id) else {
                return Err(format!("unknown interrupt id: {}", item.interrupt_id));
            };
            if item.status == "resolved" {
                validate_resume_payload(&waiting.interrupt, &item.payload_json)?;
            }
            prepared.push((
                item.interrupt_id.clone(),
                HitlResolution {
                    interrupt_id: item.interrupt_id.clone(),
                    status: item.status.clone(),
                    payload_json: item.payload_json.clone(),
                },
            ));
        }

        for (id, resolution) in prepared {
            if let Some(waiting) = map.remove(&id) {
                let _ = waiting.tx.send(resolution);
            }
        }
        Ok(())
    }

    /// 取消全部等待（chat cancel）。
    pub async fn cancel_all(&self) {
        let mut map = self.waiting.lock().await;
        for (_, w) in map.drain() {
            let _ = w.tx.send(HitlResolution {
                interrupt_id: w.interrupt.id,
                status: "cancelled".into(),
                payload_json: String::new(),
            });
        }
    }
}

fn validate_resume_payload(interrupt: &Interrupt, payload_json: &str) -> Result<(), String> {
    let payload: Value = if payload_json.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(payload_json).map_err(|e| format!("invalid json: {e}"))?
    };
    if interrupt.response_schema_json.trim().is_empty() {
        return Ok(());
    }
    let schema: Value = serde_json::from_str(&interrupt.response_schema_json)
        .map_err(|e| format!("invalid response_schema_json: {e}"))?;
    validate_against_schema(&schema, &payload).map_err(|e| e)
}

/// 进程内 session → 活闸门。
#[derive(Clone, Default)]
pub struct HitlRegistry {
    inner: Arc<RwLock<HashMap<String, Arc<HitlGate>>>>,
}

impl HitlRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, gate: Arc<HitlGate>) {
        self.inner
            .write()
            .await
            .insert(gate.session_id().to_string(), gate);
    }

    pub async fn get(&self, session_id: &str) -> Option<Arc<HitlGate>> {
        self.inner.read().await.get(session_id).cloned()
    }

    pub async fn remove(&self, session_id: &str) -> Option<Arc<HitlGate>> {
        self.inner.write().await.remove(session_id)
    }

    pub async fn cancel_and_remove(&self, session_id: &str) {
        if let Some(gate) = self.remove(session_id).await {
            gate.cancel_all().await;
        }
    }
}

/// 工具名是否为 interactive（整批强制串行）。
pub fn is_interactive_tool(name: &str) -> bool {
    matches!(name, "confirm" | "clarify")
}

/// 工具名是否需独占 `&mut MemoryManager`（整批强制串行）。
pub fn is_exclusive_tool(name: &str) -> bool {
    matches!(
        name,
        "memory"
            | "session_search"
            | "create_agent"
            | "delegate"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn resolve_unblocks_request() {
        let gate = HitlGate::new("s1");
        let gate2 = gate.clone();
        let handle = tokio::spawn(async move {
            gate2
                .request(HitlRequest::with_defaults(
                    "tc1".into(),
                    "confirmation".into(),
                    "ok?".into(),
                    json!([]),
                    json!({
                        "type": "object",
                        "properties": { "approved": { "type": "boolean" } },
                        "required": ["approved"]
                    }),
                ))
                .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let id = gate.pending_interrupts().await[0].id.clone();
        gate.resolve(&[ResumeItem {
            interrupt_id: id,
            status: "resolved".into(),
            payload_json: r#"{"approved":true}"#.into(),
        }])
        .await
        .unwrap();
        let (_interrupt, res) = handle.await.unwrap();
        assert_eq!(res.status, "resolved");
        assert!(res.to_tool_result().contains("approved"));
    }

    #[tokio::test]
    async fn multi_hitl_does_not_cancel_sibling() {
        let gate = HitlGate::new("s2");
        let i1 = Interrupt {
            id: "a".into(),
            reason: "confirmation".into(),
            response_schema_json: r#"{"type":"object","required":["approved"],"properties":{"approved":{"type":"boolean"}}}"#.into(),
            ..Default::default()
        };
        let i2 = Interrupt {
            id: "b".into(),
            reason: "confirmation".into(),
            response_schema_json: r#"{"type":"object","required":["approved"],"properties":{"approved":{"type":"boolean"}}}"#.into(),
            ..Default::default()
        };
        let rx1 = gate.begin_wait(i1).await;
        let _rx2 = gate.begin_wait(i2).await;
        assert_eq!(gate.pending_interrupts().await.len(), 2);
        gate.resolve(&[ResumeItem {
            interrupt_id: "a".into(),
            status: "resolved".into(),
            payload_json: r#"{"approved":true}"#.into(),
        }])
        .await
        .unwrap();
        let res1 = rx1.await.unwrap();
        assert_eq!(res1.status, "resolved");
        assert_eq!(gate.pending_interrupts().await.len(), 1);
        assert_eq!(gate.pending_interrupts().await[0].id, "b");
    }

    #[tokio::test]
    async fn resolve_rejects_bad_schema() {
        let gate = HitlGate::new("s3");
        let i1 = Interrupt {
            id: "a".into(),
            reason: "confirmation".into(),
            response_schema_json: r#"{"type":"object","required":["approved"],"properties":{"approved":{"type":"boolean"}}}"#.into(),
            ..Default::default()
        };
        let _rx = gate.begin_wait(i1).await;
        let err = gate
            .resolve(&[ResumeItem {
                interrupt_id: "a".into(),
                status: "resolved".into(),
                payload_json: r#"{"approved":"yes"}"#.into(),
            }])
            .await
            .unwrap_err();
        assert!(err.contains("boolean") || err.contains("type"));
        assert_eq!(gate.pending_interrupts().await.len(), 1);
    }

    #[test]
    fn delegate_is_exclusive() {
        assert!(is_exclusive_tool("delegate"));
        assert!(!is_exclusive_tool("web_search"));
    }
}
