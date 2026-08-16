//! 权限评估与审批的 append-only 审计日志。
//!
//! 记录仅包含截断后的 capability 摘要，不保存命令正文、凭证或环境变量。

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use types::{ApprovalsReviewer, GrantScope, PermissionCapability, PermissionRequest};
use uuid::Uuid;

use crate::config::LoadedPermissionSettings;

const MAX_TARGETS: usize = 10;
const MAX_TARGET_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionAuditKind {
    #[serde(rename = "permission.evaluated")]
    Evaluated,
    #[serde(rename = "permission.requested")]
    Requested,
    #[serde(rename = "permission.reviewed")]
    Reviewed,
    #[serde(rename = "permission.granted")]
    Granted,
    #[serde(rename = "permission.denied")]
    Denied,
    #[serde(rename = "permission.applied")]
    Applied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionAuditCapability {
    pub kind: String,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionAuditEvent {
    pub id: String,
    pub event: PermissionAuditKind,
    pub request_id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub tool_call_id: String,
    pub tool_name: String,
    pub profile_id: String,
    pub snapshot_hash: String,
    pub scope: GrantScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<ApprovalsReviewer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub capabilities: Vec<PermissionAuditCapability>,
    pub created_at: String,
}

impl PermissionAuditEvent {
    pub fn new(
        event: PermissionAuditKind,
        request: &PermissionRequest,
        profile_id: impl Into<String>,
        snapshot_hash: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            event,
            request_id: request.request_id.clone(),
            session_id: request.session_id.clone(),
            turn_id: request.turn_id.clone(),
            tool_call_id: request.tool_call_id.clone(),
            tool_name: request.tool_name.clone(),
            profile_id: profile_id.into(),
            snapshot_hash: snapshot_hash.into(),
            scope: request.requested_scope,
            reviewer: None,
            result: None,
            duration_ms: None,
            capabilities: request
                .capabilities
                .iter()
                .map(summarize_capability)
                .collect(),
            created_at: Utc::now().to_rfc3339(),
        }
    }

    pub fn with_reviewer(mut self, reviewer: ApprovalsReviewer) -> Self {
        self.reviewer = Some(reviewer);
        self
    }

    pub fn with_result(mut self, result: impl Into<String>) -> Self {
        self.result = Some(truncate(&result.into(), MAX_TARGET_CHARS));
        self
    }

    pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }
}

pub fn permission_snapshot_hash(
    settings: &LoadedPermissionSettings,
    active_profile_id: &str,
) -> String {
    let bytes = serde_json::to_vec(&(
        active_profile_id,
        &settings.permissions,
        &settings.selection,
    ))
    .unwrap_or_default();
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn summarize_capability(capability: &PermissionCapability) -> PermissionAuditCapability {
    let (kind, targets): (&str, Vec<String>) = match capability {
        PermissionCapability::FileRead { paths } => ("file_read", paths.clone()),
        PermissionCapability::FileWrite { paths } => ("file_write", paths.clone()),
        PermissionCapability::ProcessSpawn { program, cwd } => (
            "process_spawn",
            std::iter::once(program.clone())
                .chain(cwd.iter().cloned())
                .collect(),
        ),
        PermissionCapability::Network { hosts } => ("network", hosts.clone()),
        PermissionCapability::ExternalSideEffect { category, target } => {
            (category.as_str(), vec![target.clone()])
        }
    };
    PermissionAuditCapability {
        kind: truncate(kind, MAX_TARGET_CHARS),
        targets: targets
            .into_iter()
            .take(MAX_TARGETS)
            .map(|target| truncate(&target, MAX_TARGET_CHARS))
            .collect(),
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

fn audit_dir(base: &Path) -> PathBuf {
    base.join("audit")
}

pub fn permission_audit_path(base: &Path) -> PathBuf {
    audit_dir(base).join("permissions.jsonl")
}

pub fn append_permission_audit(base: &Path, event: &PermissionAuditEvent) -> anyhow::Result<()> {
    let dir = audit_dir(base);
    fs::create_dir_all(&dir)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(permission_audit_path(base))?;
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    file.write_all(&line)?;
    Ok(())
}

pub fn try_append_permission_audit(base: &Path, event: PermissionAuditEvent) {
    if let Err(error) = append_permission_audit(base, &event) {
        tracing::warn!(%error, "permission audit append failed");
    }
}

pub fn list_recent_permission_audits(
    base: &Path,
    limit: usize,
) -> anyhow::Result<Vec<PermissionAuditEvent>> {
    let path = permission_audit_path(base);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let mut events = Vec::new();
    for line in BufReader::new(fs::File::open(path)?).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(&line) {
            Ok(event) => events.push(event),
            Err(error) => tracing::warn!(%error, "skip malformed permission audit line"),
        }
    }
    if events.len() > limit {
        Ok(events.split_off(events.len() - limit))
    } else {
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::{PermissionReason, SessionPermissions};

    fn request() -> PermissionRequest {
        PermissionRequest {
            request_id: "request-1".into(),
            session_id: "session-1".into(),
            turn_id: Some("turn-1".into()),
            tool_call_id: "call-1".into(),
            tool_name: "terminal".into(),
            summary: "run command".into(),
            capabilities: vec![PermissionCapability::ProcessSpawn {
                program: "/bin/sh".into(),
                cwd: Some("/tmp/example".into()),
            }],
            reason: PermissionReason::UntrustedCommand,
            requested_scope: GrantScope::Once,
            command_preview: Some("secret command body".into()),
            affected_paths: Vec::new(),
            network_hosts: Vec::new(),
        }
    }

    #[test]
    fn append_roundtrip_omits_command_preview() {
        let dir = tempfile::tempdir().unwrap();
        let event = PermissionAuditEvent::new(
            PermissionAuditKind::Requested,
            &request(),
            ":workspace",
            "hash",
        )
        .with_reviewer(ApprovalsReviewer::User);
        append_permission_audit(dir.path(), &event).unwrap();
        let raw = fs::read_to_string(permission_audit_path(dir.path())).unwrap();
        assert!(raw.contains("permission.requested"));
        assert!(!raw.contains("secret command body"));
        assert_eq!(
            list_recent_permission_audits(dir.path(), 10).unwrap(),
            vec![event]
        );
    }

    #[test]
    fn snapshot_hash_changes_with_selection() {
        let mut first = LoadedPermissionSettings::default();
        let first_hash = permission_snapshot_hash(&first, ":workspace");
        first.selection = SessionPermissions::read_only();
        let second_hash = permission_snapshot_hash(&first, ":read-only");
        assert_ne!(first_hash, second_hash);
        assert_eq!(first_hash.len(), 64);
    }
}
