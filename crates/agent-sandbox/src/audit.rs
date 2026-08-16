//! 沙箱进程启动的 append-only 安全审计。

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{SandboxError, SandboxPolicy, SandboxRunner};

const MAX_FIELD_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SandboxAuditKind {
    #[serde(rename = "sandbox.spawned")]
    Spawned,
    #[serde(rename = "sandbox.denied")]
    Denied,
    #[serde(rename = "sandbox.backend_unavailable")]
    BackendUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxAuditEvent {
    pub id: String,
    pub event: SandboxAuditKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub tool_name: String,
    pub profile_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_hash: Option<String>,
    pub backend: String,
    pub sandboxed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    pub network_access: bool,
    pub writable_root_count: usize,
    pub target: String,
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub created_at: String,
}

/// 调用方提供的非敏感沙箱审计上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxAuditMetadata {
    audit_root: PathBuf,
    session_id: Option<String>,
    turn_id: Option<String>,
    tool_name: String,
    profile_id: String,
}

impl SandboxAuditMetadata {
    pub fn new(
        audit_root: impl Into<PathBuf>,
        session_id: Option<String>,
        turn_id: Option<String>,
        tool_name: impl Into<String>,
        profile_id: impl Into<String>,
    ) -> Self {
        Self {
            audit_root: audit_root.into(),
            session_id,
            turn_id,
            tool_name: truncate(&tool_name.into()),
            profile_id: truncate(&profile_id.into()),
        }
    }

    pub fn with_tool_name(&self, tool_name: impl Into<String>) -> Self {
        let mut next = self.clone();
        next.tool_name = truncate(&tool_name.into());
        next
    }

    pub fn record(
        &self,
        kind: SandboxAuditKind,
        policy: Option<&SandboxPolicy>,
        target: &str,
        result: &str,
        duration_ms: Option<u64>,
    ) {
        let health = SandboxRunner.probe();
        let sandboxed =
            policy.is_some_and(|policy| policy.mode != types::SandboxMode::DangerFullAccess);
        let backend = match policy {
            Some(policy) if policy.mode == types::SandboxMode::DangerFullAccess => {
                "unrestricted".to_string()
            }
            Some(_) => format!("{:?}", health.backend).to_ascii_lowercase(),
            None => "unknown".to_string(),
        };
        let event = SandboxAuditEvent {
            id: Uuid::new_v4().to_string(),
            event: kind,
            session_id: self.session_id.clone(),
            turn_id: self.turn_id.clone(),
            tool_name: self.tool_name.clone(),
            profile_id: self.profile_id.clone(),
            policy_hash: policy.map(policy_hash),
            backend,
            sandboxed,
            mode: policy.map(|policy| format!("{:?}", policy.mode).to_ascii_lowercase()),
            network_access: policy.is_some_and(|policy| policy.network_access),
            writable_root_count: policy.map_or(0, |policy| policy.writable_roots.len()),
            target: truncate(target),
            result: truncate(result),
            duration_ms,
            created_at: Utc::now().to_rfc3339(),
        };
        try_append_sandbox_audit(&self.audit_root, event);
    }

    pub fn record_prepare_error(
        &self,
        policy: Option<&SandboxPolicy>,
        target: &str,
        error: &SandboxError,
        duration_ms: Option<u64>,
    ) {
        let kind = if matches!(error, SandboxError::BackendUnavailable(_)) {
            SandboxAuditKind::BackendUnavailable
        } else {
            SandboxAuditKind::Denied
        };
        self.record(kind, policy, target, "prepare_failed", duration_ms);
    }
}

fn policy_hash(policy: &SandboxPolicy) -> String {
    let digest = Sha256::digest(policy.profile_hash_material());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn truncate(value: &str) -> String {
    let mut chars = value.chars();
    let head: String = chars.by_ref().take(MAX_FIELD_CHARS).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

fn audit_dir(base: &Path) -> PathBuf {
    base.join("audit")
}

pub fn sandbox_audit_path(base: &Path) -> PathBuf {
    audit_dir(base).join("sandbox.jsonl")
}

pub fn append_sandbox_audit(base: &Path, event: &SandboxAuditEvent) -> anyhow::Result<()> {
    let dir = audit_dir(base);
    fs::create_dir_all(&dir)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(sandbox_audit_path(base))?;
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    file.write_all(&line)?;
    Ok(())
}

pub fn try_append_sandbox_audit(base: &Path, event: SandboxAuditEvent) {
    if let Err(error) = append_sandbox_audit(base, &event) {
        tracing::warn!(%error, "sandbox audit append failed");
    }
}

pub fn list_recent_sandbox_audits(
    base: &Path,
    limit: usize,
) -> anyhow::Result<Vec<SandboxAuditEvent>> {
    let path = sandbox_audit_path(base);
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
            Err(error) => tracing::warn!(%error, "skip malformed sandbox audit line"),
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
    use types::SandboxMode;

    #[test]
    fn audit_roundtrip_uses_policy_hash_without_paths() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("secret-workspace-name");
        fs::create_dir_all(&workspace).unwrap();
        let policy =
            SandboxPolicy::new(SandboxMode::WorkspaceWrite, &workspace, Vec::new(), false).unwrap();
        let metadata = SandboxAuditMetadata::new(
            dir.path(),
            Some("session-1".into()),
            Some("turn-1".into()),
            "terminal",
            ":workspace",
        );
        metadata.record(
            SandboxAuditKind::Spawned,
            Some(&policy),
            "sh",
            "spawned",
            Some(3),
        );

        let raw = fs::read_to_string(sandbox_audit_path(dir.path())).unwrap();
        assert!(!raw.contains("secret-workspace-name"));
        let events = list_recent_sandbox_audits(dir.path(), 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, SandboxAuditKind::Spawned);
        assert_eq!(events[0].policy_hash.as_deref().map(str::len), Some(64));
        assert!(events[0].sandboxed);
    }

    #[test]
    fn backend_unavailable_error_uses_dedicated_event() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = SandboxAuditMetadata::new(dir.path(), None, None, "terminal", ":workspace");
        metadata.record_prepare_error(
            None,
            "sh",
            &SandboxError::BackendUnavailable("missing backend".into()),
            Some(1),
        );

        let events = list_recent_sandbox_audits(dir.path(), 10).unwrap();
        assert_eq!(events[0].event, SandboxAuditKind::BackendUnavailable);
        assert_eq!(events[0].result, "prepare_failed");
        assert_eq!(events[0].backend, "unknown");
    }
}
