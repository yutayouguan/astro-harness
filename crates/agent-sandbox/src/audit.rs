//! 沙箱进程启动的 append-only 安全审计。

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{SandboxError, SandboxPolicy, SandboxRunner};

const MAX_FIELD_CHARS: usize = 160;
pub const MAX_SANDBOX_AUDIT_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const SANDBOX_AUDIT_ARCHIVE_COUNT: usize = 3;

static AUDIT_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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

pub fn sandbox_audit_archive_path(base: &Path, index: usize) -> PathBuf {
    audit_dir(base).join(format!("sandbox.{index}.jsonl"))
}

/// 删除当前沙箱审计及其轮转归档，返回已删除文件数与字节数。
pub fn clear_sandbox_audits(base: &Path) -> anyhow::Result<(usize, u64)> {
    let _guard = AUDIT_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow::anyhow!("sandbox audit write lock poisoned"))?;
    let paths = std::iter::once(sandbox_audit_path(base)).chain(
        (1..=SANDBOX_AUDIT_ARCHIVE_COUNT).map(|index| sandbox_audit_archive_path(base, index)),
    );
    let mut files_removed = 0usize;
    let mut bytes_removed = 0u64;
    for path in paths {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        fs::remove_file(&path)?;
        files_removed += 1;
        bytes_removed = bytes_removed.saturating_add(metadata.len());
    }
    Ok((files_removed, bytes_removed))
}

pub fn append_sandbox_audit(base: &Path, event: &SandboxAuditEvent) -> anyhow::Result<()> {
    append_sandbox_audit_with_policy(
        base,
        event,
        MAX_SANDBOX_AUDIT_FILE_BYTES,
        SANDBOX_AUDIT_ARCHIVE_COUNT,
    )
}

fn append_sandbox_audit_with_policy(
    base: &Path,
    event: &SandboxAuditEvent,
    max_file_bytes: u64,
    archive_count: usize,
) -> anyhow::Result<()> {
    let _guard = AUDIT_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow::anyhow!("sandbox audit write lock poisoned"))?;
    let dir = audit_dir(base);
    fs::create_dir_all(&dir)?;
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    rotate_sandbox_audit_if_needed(
        base,
        u64::try_from(line.len()).unwrap_or(u64::MAX),
        max_file_bytes,
        archive_count,
    )?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(sandbox_audit_path(base))?;
    file.write_all(&line)?;
    Ok(())
}

fn rotate_sandbox_audit_if_needed(
    base: &Path,
    incoming_bytes: u64,
    max_file_bytes: u64,
    archive_count: usize,
) -> anyhow::Result<()> {
    let active = sandbox_audit_path(base);
    let current_bytes = active.metadata().map(|meta| meta.len()).unwrap_or(0);
    if current_bytes == 0 || current_bytes.saturating_add(incoming_bytes) <= max_file_bytes {
        return Ok(());
    }
    if archive_count == 0 {
        fs::remove_file(active)?;
        return Ok(());
    }
    for index in (1..=archive_count).rev() {
        let source = if index == 1 {
            active.clone()
        } else {
            sandbox_audit_archive_path(base, index - 1)
        };
        if !source.is_file() {
            continue;
        }
        let destination = sandbox_audit_archive_path(base, index);
        if destination.exists() {
            fs::remove_file(&destination)?;
        }
        fs::rename(source, destination)?;
    }
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
    list_sandbox_audits_before(base, None, limit)
}

/// 读取游标之前最近的沙箱审计，结果保持从旧到新的文件顺序。
pub fn list_sandbox_audits_before(
    base: &Path,
    before: Option<(&str, &str)>,
    limit: usize,
) -> anyhow::Result<Vec<SandboxAuditEvent>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut events = VecDeque::with_capacity(limit.min(1024));
    let paths = (1..=SANDBOX_AUDIT_ARCHIVE_COUNT)
        .rev()
        .map(|index| sandbox_audit_archive_path(base, index))
        .chain(std::iter::once(sandbox_audit_path(base)));
    for path in paths {
        if !path.is_file() {
            continue;
        }
        for line in BufReader::new(fs::File::open(path)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<SandboxAuditEvent>(&line) {
                Ok(event) => {
                    if before.is_some_and(|cursor| {
                        (event.created_at.as_str(), event.id.as_str()) >= cursor
                    }) {
                        continue;
                    }
                    events.push_back(event);
                    if events.len() > limit {
                        events.pop_front();
                    }
                }
                Err(error) => tracing::warn!(%error, "skip malformed sandbox audit line"),
            }
        }
    }
    Ok(events.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::SandboxMode;

    fn test_event(id: usize) -> SandboxAuditEvent {
        SandboxAuditEvent {
            id: format!("event-{id}"),
            event: SandboxAuditKind::Spawned,
            session_id: Some("session-1".into()),
            turn_id: Some("turn-1".into()),
            tool_name: "terminal".into(),
            profile_id: ":workspace".into(),
            policy_hash: Some("0".repeat(64)),
            backend: "seatbelt".into(),
            sandboxed: true,
            mode: Some("workspacewrite".into()),
            network_access: false,
            writable_root_count: 1,
            target: "sh".into(),
            result: "spawned".into(),
            duration_ms: Some(1),
            created_at: format!("2026-08-17T00:00:0{id}Z"),
        }
    }

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

    #[test]
    fn rotation_retains_only_newest_archives_and_query_crosses_files() {
        let dir = tempfile::tempdir().unwrap();
        let sample = test_event(0);
        let max_file_bytes = serde_json::to_vec(&sample).unwrap().len() as u64 + 2;

        for id in 0..5 {
            append_sandbox_audit_with_policy(dir.path(), &test_event(id), max_file_bytes, 2)
                .unwrap();
        }

        assert!(sandbox_audit_path(dir.path()).is_file());
        assert!(sandbox_audit_archive_path(dir.path(), 1).is_file());
        assert!(sandbox_audit_archive_path(dir.path(), 2).is_file());
        assert!(!sandbox_audit_archive_path(dir.path(), 3).is_file());
        let events = list_recent_sandbox_audits(dir.path(), 10).unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            vec!["event-2", "event-3", "event-4"]
        );
        assert_eq!(
            list_recent_sandbox_audits(dir.path(), 2)
                .unwrap()
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            vec!["event-3", "event-4"]
        );
        let before =
            list_sandbox_audits_before(dir.path(), Some(("2026-08-17T00:00:04Z", "event-4")), 2)
                .unwrap();
        assert_eq!(
            before
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            vec!["event-2", "event-3"]
        );
    }

    #[test]
    fn clear_removes_active_and_rotated_sandbox_audits() {
        let dir = tempfile::tempdir().unwrap();
        let sample = test_event(0);
        let max_file_bytes = serde_json::to_vec(&sample).unwrap().len() as u64 + 2;
        for id in 0..4 {
            append_sandbox_audit_with_policy(dir.path(), &test_event(id), max_file_bytes, 2)
                .unwrap();
        }
        let expected_bytes = [
            sandbox_audit_path(dir.path()),
            sandbox_audit_archive_path(dir.path(), 1),
            sandbox_audit_archive_path(dir.path(), 2),
        ]
        .iter()
        .map(|path| path.metadata().unwrap().len())
        .sum::<u64>();

        let (files_removed, bytes_removed) = clear_sandbox_audits(dir.path()).unwrap();

        assert_eq!(files_removed, 3);
        assert_eq!(bytes_removed, expected_bytes);
        assert!(list_recent_sandbox_audits(dir.path(), 10)
            .unwrap()
            .is_empty());
        assert_eq!(clear_sandbox_audits(dir.path()).unwrap(), (0, 0));
    }
}
