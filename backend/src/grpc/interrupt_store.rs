//! Session 级 interrupt 挂起表（进程内 + `sessions/{id}/interrupt.json`）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent::{Interrupt, InterruptPending, ResumeItem};
use tokio::sync::RwLock;

/// `{memory_dir}/sessions/{session_id}/interrupt.json`
pub fn interrupt_file_path(memory_dir: &Path, session_id: &str) -> PathBuf {
    memory_dir
        .join("sessions")
        .join(session_id)
        .join("interrupt.json")
}

/// 将未决 interrupts 写入旁路文件。
pub fn save_interrupt_file(
    memory_dir: &Path,
    session_id: &str,
    interrupts: &[Interrupt],
) -> std::io::Result<()> {
    let path = interrupt_file_path(memory_dir, session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(interrupts)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// 读取旁路文件；无效或缺失返回 None。
pub fn load_interrupt_file(memory_dir: &Path, session_id: &str) -> Option<InterruptPending> {
    let path = interrupt_file_path(memory_dir, session_id);
    let raw = std::fs::read_to_string(path).ok()?;
    let interrupts: Vec<Interrupt> = serde_json::from_str(&raw).ok()?;
    if interrupts.is_empty() {
        return None;
    }
    Some(InterruptPending::new(interrupts))
}

/// 删除旁路文件（忽略缺失）。
pub fn clear_interrupt_file(memory_dir: &Path, session_id: &str) {
    let path = interrupt_file_path(memory_dir, session_id);
    let _ = std::fs::remove_file(path);
}

/// session_id → 未决 InterruptPending。
#[derive(Clone)]
pub struct InterruptStore {
    inner: Arc<RwLock<HashMap<String, InterruptPending>>>,
    memory_dir: PathBuf,
}

impl InterruptStore {
    pub fn new(memory_dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            memory_dir,
        }
    }

    pub async fn insert(&self, session_id: &str, pending: InterruptPending) {
        let _ = save_interrupt_file(&self.memory_dir, session_id, pending.interrupts());
        self.inner
            .write()
            .await
            .insert(session_id.to_string(), pending);
    }

    pub async fn get(&self, session_id: &str) -> Option<InterruptPending> {
        if let Some(p) = self.inner.read().await.get(session_id).cloned() {
            return Some(p);
        }
        let loaded = load_interrupt_file(&self.memory_dir, session_id)?;
        self.inner
            .write()
            .await
            .insert(session_id.to_string(), loaded.clone());
        Some(loaded)
    }

    pub async fn clear(&self, session_id: &str) {
        clear_interrupt_file(&self.memory_dir, session_id);
        self.inner.write().await.remove(session_id);
    }

    /// 取消挂起：若有 pending 则 cancel_all 并清除。
    pub async fn cancel_pending(&self, session_id: &str) {
        clear_interrupt_file(&self.memory_dir, session_id);
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
    use agent::{Interrupt, ResumeItem};

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

    #[test]
    fn interrupt_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let interrupts = vec![Interrupt {
            id: "i1".into(),
            reason: "confirmation".into(),
            message: "ok?".into(),
            ..Default::default()
        }];
        save_interrupt_file(dir.path(), "sess-1", &interrupts).unwrap();
        let path = interrupt_file_path(dir.path(), "sess-1");
        assert!(path.is_file());
        let pending = load_interrupt_file(dir.path(), "sess-1").unwrap();
        assert_eq!(pending.interrupts().len(), 1);
        assert_eq!(pending.interrupts()[0].id, "i1");
        clear_interrupt_file(dir.path(), "sess-1");
        assert!(load_interrupt_file(dir.path(), "sess-1").is_none());
    }
}
