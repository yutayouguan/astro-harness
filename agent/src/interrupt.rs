//! AG-UI 风格 interrupt 挂起与 resume 校验。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// 一次 HITL 挂起项（对齐 AG-UI Interrupt）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Interrupt {
    pub id: String,
    /// `tool_call` | `input_required` | `confirmation`
    pub reason: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub tool_call_id: String,
    /// JSON Schema 字符串；空则跳过 schema 校验，仅做 MVP 字段检查。
    #[serde(default)]
    pub response_schema_json: String,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub metadata_json: String,
}

/// 客户端对单个 interrupt 的回复。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResumeItem {
    pub interrupt_id: String,
    /// `resolved` | `cancelled`
    pub status: String,
    #[serde(default)]
    pub payload_json: String,
}

/// Session 级未决 interrupts。
#[derive(Debug, Clone)]
pub struct InterruptPending {
    interrupts: Vec<Interrupt>,
    /// 成功 apply 后的 resume 项（供注入 tool result）。
    resolutions: Vec<ResumeItem>,
}

#[derive(Debug, Error)]
pub enum InterruptError {
    #[error("partial resume: missing interrupt ids: {0}")]
    Partial(String),
    #[error("unknown interrupt id: {0}")]
    UnknownId(String),
    #[error("duplicate resume for interrupt id: {0}")]
    Duplicate(String),
    #[error("invalid resume status: {0}")]
    BadStatus(String),
    #[error("payload validation failed for {0}: {1}")]
    Payload(String, String),
    #[error("interrupt expired: {0}")]
    Expired(String),
    #[error("no pending interrupts")]
    Empty,
}

impl InterruptPending {
    pub fn new(interrupts: Vec<Interrupt>) -> Self {
        Self {
            interrupts,
            resolutions: Vec::new(),
        }
    }

    pub fn interrupts(&self) -> &[Interrupt] {
        &self.interrupts
    }

    pub fn resolutions(&self) -> &[ResumeItem] {
        &self.resolutions
    }

    pub fn is_cleared(&self) -> bool {
        self.interrupts.is_empty()
    }

    /// 将全部未决 interrupt 标为 cancelled 并清空。
    pub fn cancel_all(&mut self) {
        self.resolutions = self
            .interrupts
            .iter()
            .map(|i| ResumeItem {
                interrupt_id: i.id.clone(),
                status: "cancelled".into(),
                payload_json: String::new(),
            })
            .collect();
        self.interrupts.clear();
    }

    /// 必须覆盖每一个 open interrupt；成功则清空 pending。
    pub fn apply_resume(&mut self, items: &[ResumeItem]) -> Result<(), InterruptError> {
        if self.interrupts.is_empty() {
            return Err(InterruptError::Empty);
        }

        let mut seen = std::collections::HashSet::new();
        for item in items {
            if !seen.insert(item.interrupt_id.as_str()) {
                return Err(InterruptError::Duplicate(item.interrupt_id.clone()));
            }
            if item.status != "resolved" && item.status != "cancelled" {
                return Err(InterruptError::BadStatus(item.status.clone()));
            }
            let Some(interrupt) = self.interrupts.iter().find(|i| i.id == item.interrupt_id) else {
                return Err(InterruptError::UnknownId(item.interrupt_id.clone()));
            };
            if !interrupt.expires_at.is_empty() {
                if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(&interrupt.expires_at) {
                    if chrono::Utc::now() > exp {
                        return Err(InterruptError::Expired(interrupt.id.clone()));
                    }
                }
            }
            if item.status == "resolved" {
                validate_payload(interrupt, &item.payload_json)?;
            }
        }

        let missing: Vec<_> = self
            .interrupts
            .iter()
            .filter(|i| !seen.contains(i.id.as_str()))
            .map(|i| i.id.clone())
            .collect();
        if !missing.is_empty() {
            return Err(InterruptError::Partial(missing.join(",")));
        }

        self.resolutions = items.to_vec();
        self.interrupts.clear();
        Ok(())
    }
}

fn validate_payload(interrupt: &Interrupt, payload_json: &str) -> Result<(), InterruptError> {
    let payload: Value = if payload_json.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(payload_json).map_err(|e| {
            InterruptError::Payload(interrupt.id.clone(), format!("invalid json: {e}"))
        })?
    };

    if !interrupt.response_schema_json.trim().is_empty() {
        let schema: Value = serde_json::from_str(&interrupt.response_schema_json).map_err(|e| {
            InterruptError::Payload(
                interrupt.id.clone(),
                format!("invalid response_schema_json: {e}"),
            )
        })?;
        crate::schema_validate::validate_against_schema(&schema, &payload).map_err(|e| {
            InterruptError::Payload(interrupt.id.clone(), e)
        })?;
        return Ok(());
    }

    // 无 schema 时回退 reason 轻量检查
    match interrupt.reason.as_str() {
        "confirmation" | "tool_call" => {
            let approved = payload.get("approved").and_then(|v| v.as_bool());
            if approved.is_none() {
                return Err(InterruptError::Payload(
                    interrupt.id.clone(),
                    "expected boolean field `approved`".into(),
                ));
            }
        }
        "input_required" => {
            let value = payload.get("value").and_then(|v| v.as_str());
            if value.map(|s| s.is_empty()).unwrap_or(true) {
                return Err(InterruptError::Payload(
                    interrupt.id.clone(),
                    "expected non-empty string field `value`".into(),
                ));
            }
        }
        _ => {
            if !payload.is_object() {
                return Err(InterruptError::Payload(
                    interrupt.id.clone(),
                    "payload must be a JSON object".into(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_partial_resume() {
        let mut p = InterruptPending::new(vec![
            Interrupt {
                id: "i1".into(),
                reason: "confirmation".into(),
                ..Default::default()
            },
            Interrupt {
                id: "i2".into(),
                reason: "confirmation".into(),
                ..Default::default()
            },
        ]);
        let err = p
            .apply_resume(&[ResumeItem {
                interrupt_id: "i1".into(),
                status: "resolved".into(),
                payload_json: r#"{"approved":true}"#.into(),
            }])
            .unwrap_err();
        assert!(err.to_string().contains("partial"));
    }

    #[test]
    fn accepts_full_resume() {
        let mut p = InterruptPending::new(vec![Interrupt {
            id: "i1".into(),
            reason: "confirmation".into(),
            ..Default::default()
        }]);
        p.apply_resume(&[ResumeItem {
            interrupt_id: "i1".into(),
            status: "resolved".into(),
            payload_json: r#"{"approved":true}"#.into(),
        }])
        .unwrap();
        assert!(p.is_cleared());
        assert_eq!(p.resolutions().len(), 1);
    }

    #[test]
    fn rejects_bad_confirm_payload() {
        let mut p = InterruptPending::new(vec![Interrupt {
            id: "i1".into(),
            reason: "confirmation".into(),
            ..Default::default()
        }]);
        let err = p
            .apply_resume(&[ResumeItem {
                interrupt_id: "i1".into(),
                status: "resolved".into(),
                payload_json: r#"{"ok":true}"#.into(),
            }])
            .unwrap_err();
        assert!(err.to_string().contains("approved"));
    }

    #[test]
    fn cancel_all_clears() {
        let mut p = InterruptPending::new(vec![Interrupt {
            id: "i1".into(),
            reason: "input_required".into(),
            ..Default::default()
        }]);
        p.cancel_all();
        assert!(p.is_cleared());
        assert_eq!(p.resolutions()[0].status, "cancelled");
    }
}
