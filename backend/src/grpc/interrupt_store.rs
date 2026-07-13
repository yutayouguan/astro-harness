//! Session 级 interrupt 挂起表（进程内）。

use std::collections::HashMap;
use std::sync::Arc;

use agent::{Interrupt, InterruptPending, ResumeItem};
use tokio::sync::RwLock;

/// session_id → 未决 InterruptPending。
#[derive(Clone, Default)]
pub struct InterruptStore {
    inner: Arc<RwLock<HashMap<String, InterruptPending>>>,
}

impl InterruptStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, session_id: &str, pending: InterruptPending) {
        self.inner
            .write()
            .await
            .insert(session_id.to_string(), pending);
    }

    pub async fn get(&self, session_id: &str) -> Option<InterruptPending> {
        self.inner.read().await.get(session_id).cloned()
    }

    pub async fn clear(&self, session_id: &str) {
        self.inner.write().await.remove(session_id);
    }

    /// 取消挂起：若有 pending 则 cancel_all 并清除。
    pub async fn cancel_pending(&self, session_id: &str) {
        let mut map = self.inner.write().await;
        if let Some(mut p) = map.remove(session_id) {
            p.cancel_all();
        }
    }
}

/// 从 RunFinished.interrupts_json 解析 agent Interrupt 列表。
pub fn parse_pending_interrupts(raw: &str) -> Vec<Interrupt> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// 解析 ChatRequest.resume_json / InterruptResume 列表。
pub fn parse_resume_items_json(raw: &str) -> Result<Vec<ResumeItem>, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("resume_json 无效: {e}"))?;
    let arr = value
        .as_array()
        .ok_or_else(|| "resume_json 须为 JSON 数组".to_string())?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let interrupt_id = item
            .get("interrupt_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "resume 项缺少 interrupt_id".to_string())?
            .to_string();
        let status = item
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("resolved")
            .to_string();
        let payload_json = match item.get("payload_json").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => item
                .get("payload")
                .map(|v| v.to_string())
                .unwrap_or_default(),
        };
        out.push(ResumeItem {
            interrupt_id,
            status,
            payload_json,
        });
    }
    Ok(out)
}

/// 将 proto InterruptResumeItem 转为 ResumeItem。
pub fn resume_items_from_proto(items: &[proto::InterruptResumeItem]) -> Vec<ResumeItem> {
    items
        .iter()
        .map(|i| ResumeItem {
            interrupt_id: i.interrupt_id.clone(),
            status: if i.status.is_empty() {
                "resolved".into()
            } else {
                i.status.clone()
            },
            payload_json: i.payload_json.clone(),
        })
        .collect()
}

/// 格式化为注入会话的用户消息。
pub fn format_resume_user_message(items: &[ResumeItem]) -> String {
    let payload: Vec<_> = items
        .iter()
        .map(|i| {
            serde_json::json!({
                "interrupt_id": i.interrupt_id,
                "status": i.status,
                "payload": serde_json::from_str::<serde_json::Value>(&i.payload_json)
                    .unwrap_or(serde_json::Value::Null),
            })
        })
        .collect();
    format!(
        "[interrupt_resume] {}",
        serde_json::to_string(&payload).unwrap_or_else(|_| "[]".into())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::ResumeItem;

    #[test]
    fn parse_resume_items_json_accepts_payload_object() {
        let raw = r#"[{"interrupt_id":"i1","status":"resolved","payload":{"approved":true}}]"#;
        let items = parse_resume_items_json(raw).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].interrupt_id, "i1");
        assert!(items[0].payload_json.contains("approved"));
    }

    #[test]
    fn format_resume_user_message_prefix() {
        let msg = format_resume_user_message(&[ResumeItem {
            interrupt_id: "i1".into(),
            status: "resolved".into(),
            payload_json: r#"{"approved":true}"#.into(),
        }]);
        assert!(msg.starts_with("[interrupt_resume] "));
    }
}
