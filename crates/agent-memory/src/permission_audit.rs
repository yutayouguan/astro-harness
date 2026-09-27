//! 权限评估与审批的 append-only 审计日志。
//!
//! 记录仅包含截断后的 capability 摘要，不保存命令正文、凭证或环境变量。

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use types::{ApprovalsReviewer, GrantScope, PermissionCapability, PermissionRequest};
use uuid::Uuid;

use crate::config::LoadedPermissionSettings;

const MAX_TARGETS: usize = 10;
const MAX_TARGET_CHARS: usize = 160;
pub const MAX_PERMISSION_AUDIT_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const PERMISSION_AUDIT_ARCHIVE_COUNT: usize = 3;

static AUDIT_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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
    #[serde(rename = "permission.revoked")]
    Revoked,
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
    /// 审批决策理由（smart approval 模型返回或人工审批标记），最长 500 字符。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
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
            reasoning: None,
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

    pub fn with_reasoning(mut self, reasoning: impl Into<String>) -> Self {
        let text = reasoning.into();
        self.reasoning = Some(if text.chars().count() > 500 {
            let truncated: String = text.chars().take(500).collect();
            format!("{truncated}...")
        } else {
            text
        });
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
    home::security_audit_dir(base)
}

pub fn permission_audit_path(base: &Path) -> PathBuf {
    audit_dir(base).join("permissions.jsonl")
}

pub fn permission_audit_archive_path(base: &Path, index: usize) -> PathBuf {
    audit_dir(base).join(format!("permissions.{index}.jsonl"))
}

/// 删除当前权限审计及其轮转归档，返回已删除文件数与字节数。
pub fn clear_permission_audits(base: &Path) -> anyhow::Result<(usize, u64)> {
    let _guard = AUDIT_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow::anyhow!("permission audit write lock poisoned"))?;
    let paths = std::iter::once(permission_audit_path(base)).chain(
        (1..=PERMISSION_AUDIT_ARCHIVE_COUNT)
            .map(|index| permission_audit_archive_path(base, index)),
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

pub fn append_permission_audit(base: &Path, event: &PermissionAuditEvent) -> anyhow::Result<()> {
    append_permission_audit_with_policy(
        base,
        event,
        MAX_PERMISSION_AUDIT_FILE_BYTES,
        PERMISSION_AUDIT_ARCHIVE_COUNT,
    )
}

fn append_permission_audit_with_policy(
    base: &Path,
    event: &PermissionAuditEvent,
    max_file_bytes: u64,
    archive_count: usize,
) -> anyhow::Result<()> {
    let _guard = AUDIT_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow::anyhow!("permission audit write lock poisoned"))?;
    let dir = audit_dir(base);
    fs::create_dir_all(&dir)?;
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    rotate_permission_audit_if_needed(
        base,
        u64::try_from(line.len()).unwrap_or(u64::MAX),
        max_file_bytes,
        archive_count,
    )?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(permission_audit_path(base))?;
    file.write_all(&line)?;
    Ok(())
}

fn rotate_permission_audit_if_needed(
    base: &Path,
    incoming_bytes: u64,
    max_file_bytes: u64,
    archive_count: usize,
) -> anyhow::Result<()> {
    let active = permission_audit_path(base);
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
            permission_audit_archive_path(base, index - 1)
        };
        if !source.is_file() {
            continue;
        }
        let destination = permission_audit_archive_path(base, index);
        if destination.exists() {
            fs::remove_file(&destination)?;
        }
        fs::rename(source, destination)?;
    }
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
    list_permission_audits_before(base, None, limit)
}

/// 读取游标之前最近的权限审计，结果保持从旧到新的文件顺序。
pub fn list_permission_audits_before(
    base: &Path,
    before: Option<(&str, &str)>,
    limit: usize,
) -> anyhow::Result<Vec<PermissionAuditEvent>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut events = VecDeque::with_capacity(limit.min(1024));
    let paths = (1..=PERMISSION_AUDIT_ARCHIVE_COUNT)
        .rev()
        .map(|index| permission_audit_archive_path(base, index))
        .chain(std::iter::once(permission_audit_path(base)));
    for path in paths {
        if !path.is_file() {
            continue;
        }
        for line in BufReader::new(fs::File::open(path)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<PermissionAuditEvent>(&line) {
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
                Err(error) => tracing::warn!(%error, "skip malformed permission audit line"),
            }
        }
    }
    Ok(events.into_iter().collect())
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
            tool_name: "exec_command".into(),
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

    fn test_event(id: usize) -> PermissionAuditEvent {
        let mut event = PermissionAuditEvent::new(
            PermissionAuditKind::Requested,
            &request(),
            ":workspace",
            "hash",
        );
        event.id = format!("event-{id}");
        event.created_at = format!("2026-08-17T00:00:0{id}Z");
        event
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

    #[test]
    fn rotation_retains_only_newest_archives_and_query_crosses_files() {
        let dir = tempfile::tempdir().unwrap();
        let sample = test_event(0);
        let max_file_bytes = serde_json::to_vec(&sample).unwrap().len() as u64 + 2;

        for id in 0..5 {
            append_permission_audit_with_policy(dir.path(), &test_event(id), max_file_bytes, 2)
                .unwrap();
        }

        assert!(permission_audit_path(dir.path()).is_file());
        assert!(permission_audit_archive_path(dir.path(), 1).is_file());
        assert!(permission_audit_archive_path(dir.path(), 2).is_file());
        assert!(!permission_audit_archive_path(dir.path(), 3).is_file());
        let events = list_recent_permission_audits(dir.path(), 10).unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            vec!["event-2", "event-3", "event-4"]
        );
        assert_eq!(
            list_recent_permission_audits(dir.path(), 2)
                .unwrap()
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            vec!["event-3", "event-4"]
        );
        assert!(list_recent_permission_audits(dir.path(), 0)
            .unwrap()
            .is_empty());
        let before =
            list_permission_audits_before(dir.path(), Some(("2026-08-17T00:00:04Z", "event-4")), 2)
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
    fn clear_removes_active_and_rotated_permission_audits() {
        let dir = tempfile::tempdir().unwrap();
        let sample = test_event(0);
        let max_file_bytes = serde_json::to_vec(&sample).unwrap().len() as u64 + 2;
        for id in 0..4 {
            append_permission_audit_with_policy(dir.path(), &test_event(id), max_file_bytes, 2)
                .unwrap();
        }
        let expected_bytes = [
            permission_audit_path(dir.path()),
            permission_audit_archive_path(dir.path(), 1),
            permission_audit_archive_path(dir.path(), 2),
        ]
        .iter()
        .map(|path| path.metadata().unwrap().len())
        .sum::<u64>();

        let (files_removed, bytes_removed) = clear_permission_audits(dir.path()).unwrap();

        assert_eq!(files_removed, 3);
        assert_eq!(bytes_removed, expected_bytes);
        assert!(list_recent_permission_audits(dir.path(), 10)
            .unwrap()
            .is_empty());
        assert_eq!(clear_permission_audits(dir.path()).unwrap(), (0, 0));
    }
}

/// 可授权的写入根净化：绝对路径、非 Astro 自身目录、非常见敏感目录。
///
/// 由 `request_permissions` 的 preflight（会话级授权）与权限设置页（永久可写目录）
/// 共用，避免两处规则漂移。
///
/// 两侧都做规范化再比较：macOS 上 `/var` 与 `/private/var` 是同一目录的两种写法，
/// 只规范化一边会让「目录已存在」的路径绕过 Astro 自身目录 / 敏感目录的判定。
pub fn sanitize_write_root(
    raw: &str,
    memory_dir: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("空路径".to_string());
    }
    let path = std::path::PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(format!("{trimmed}：必须是绝对路径"));
    }
    let normalized = path.canonicalize().unwrap_or_else(|_| path.clone());
    let memory_root = memory_dir
        .canonicalize()
        .unwrap_or_else(|_| memory_dir.to_path_buf());
    if normalized.starts_with(&memory_root) || path.starts_with(memory_dir) {
        return Err(format!("{trimmed}：Astro 自身目录不可授予"));
    }
    if let Some(raw_home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        let home = raw_home.canonicalize().unwrap_or_else(|_| raw_home.clone());
        for sensitive in [".ssh", ".aws", ".gnupg", "Library/Keychains"] {
            for base in [&home, &raw_home] {
                let sensitive_dir = base.join(sensitive);
                let sensitive_dir = sensitive_dir.canonicalize().unwrap_or(sensitive_dir);
                if normalized.starts_with(&sensitive_dir) || path.starts_with(&sensitive_dir) {
                    return Err(format!("{trimmed}：敏感目录不可授予"));
                }
            }
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod write_root_tests {
    use super::*;

    #[test]
    fn existing_paths_inside_astro_home_are_still_rejected() {
        let memory = tempfile::tempdir().unwrap();
        let inside = memory.path().join("workspace-out");
        std::fs::create_dir_all(&inside).unwrap();
        // 已存在的目录会被 canonicalize（macOS 上 /var → /private/var）：只规范化
        // 请求侧就会漏判，这里锁定「两侧都规范化」的行为。
        assert!(sanitize_write_root(&inside.display().to_string(), memory.path()).is_err());
        assert!(sanitize_write_root("", memory.path()).is_err());
        assert!(sanitize_write_root("relative/out", memory.path()).is_err());

        let outside = tempfile::tempdir().unwrap();
        assert!(sanitize_write_root(
            &outside.path().join("shared-out").display().to_string(),
            memory.path()
        )
        .is_ok());

        if let Some(home) = std::env::var_os("HOME") {
            let ssh = std::path::PathBuf::from(home).join(".ssh/id_rsa");
            assert!(sanitize_write_root(&ssh.display().to_string(), memory.path()).is_err());
        }
    }
}
