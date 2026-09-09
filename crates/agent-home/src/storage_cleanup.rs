//! Explicit, recoverable cleanup. Preparation is read-only and tokens are one-use.
//! No client paths are accepted by execute. Source and destination directories are
//! pinned capabilities; a file changed during movement is retained for recovery.
mod io;
use cap_std::fs::Dir;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

pub const MAX_BATCH: usize = 20;
const MAX_PLANS: usize = 2;
const LIFETIME: Duration = Duration::from_secs(300);
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const CACHE_FILE_CAP: u64 = 512 * 1024;
const LOG_FILE_CAP: u64 = 8 * 1024 * 1024;
const CONFIG_CAP: u64 = 2 * 1024 * 1024;
const RECOVERY_ROOT: &str = crate::layout::STORAGE_CLEANUP_RECOVERY_SUBDIR;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupError {
    UnsafePath,
    Unavailable,
    NotReady,
    NoCandidates,
    UnknownPlan,
    Expired,
    Changed,
    Busy,
    Budget,
    ConfirmationRequired,
}
impl CleanupError {
    pub fn code(self) -> &'static str {
        match self {
            Self::UnsafePath => "cleanup_unsafe_path",
            Self::Unavailable => "cleanup_unavailable",
            Self::NotReady => "cleanup_not_ready",
            Self::NoCandidates => "cleanup_no_candidates",
            Self::UnknownPlan => "cleanup_unknown_plan",
            Self::Expired => "cleanup_expired",
            Self::Changed => "cleanup_changed",
            Self::Busy => "cleanup_busy",
            Self::Budget => "cleanup_budget",
            Self::ConfirmationRequired => "cleanup_confirmation_required",
        }
    }
}
impl std::fmt::Display for CleanupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for CleanupError {}
type Result<T> = std::result::Result<T, CleanupError>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupItem {
    pub path: String,
    pub bytes: u64,
    pub policy: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
    pub token: String,
    pub root_path: String,
    pub expires_at_ms: i64,
    pub items: Vec<CleanupItem>,
    pub total_bytes: u64,
    pub omitted_files: bool,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOutcome {
    pub path: String,
    pub status: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub batch_id: String,
    pub recovery_path: String,
    pub moved_files: usize,
    pub moved_bytes: u64,
    pub unverified_files: usize,
    pub manifest_complete: bool,
    pub outcomes: Vec<CleanupOutcome>,
}

struct Entry {
    public: CleanupItem,
    relative: PathBuf,
    parent: PathBuf,
    snapshot: io::Snapshot,
}
struct PendingPlan {
    root: io::Root,
    config_digest: Option<String>,
    prepared: Instant,
    public: CleanupPlan,
    parents: BTreeMap<PathBuf, Dir>,
    entries: Vec<Entry>,
}

#[derive(Default)]
pub struct CleanupService {
    plans: Mutex<HashMap<String, PendingPlan>>,
    execution: Mutex<()>,
}
impl CleanupService {
    pub fn prepare(&self, base: &Path) -> Result<CleanupPlan> {
        let root = io::Root::open(base)?;
        let (doc, config_digest) = configuration(&root)?;
        let (parents, mut entries, omitted_files) = collect_entries(&root, &doc)?;
        if entries.is_empty() {
            return Err(CleanupError::NoCandidates);
        }
        if configuration(&root)?.1 != config_digest {
            return Err(CleanupError::Changed);
        }
        entries.sort_by(|a, b| a.relative.cmp(&b.relative));
        let public = CleanupPlan {
            token: uuid::Uuid::new_v4().to_string(),
            root_path: root.path.to_string_lossy().into_owned(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + LIFETIME.as_millis() as i64,
            total_bytes: entries.iter().map(|e| e.public.bytes).sum(),
            omitted_files,
            items: entries.iter().map(|e| e.public.clone()).collect(),
        };
        let mut plans = self.plans.lock().map_err(|_| CleanupError::Busy)?;
        plans.retain(|_, plan| {
            plan.prepared.elapsed() <= LIFETIME
                && chrono::Utc::now().timestamp_millis() < plan.public.expires_at_ms
        });
        if plans.len() >= MAX_PLANS {
            return Err(CleanupError::Busy);
        }
        plans.insert(
            public.token.clone(),
            PendingPlan {
                root,
                config_digest,
                prepared: Instant::now(),
                public: public.clone(),
                parents,
                entries,
            },
        );
        Ok(public)
    }

    pub fn discard(&self, token: &str) {
        if uuid::Uuid::parse_str(token).is_err() {
            return;
        }
        if let Ok(mut plans) = self.plans.lock() {
            plans.remove(token);
        }
    }

    pub fn execute(&self, base: &Path, token: &str, confirmed: bool) -> Result<CleanupResult> {
        if !confirmed {
            return Err(CleanupError::ConfirmationRequired);
        }
        if uuid::Uuid::parse_str(token).is_err() {
            return Err(CleanupError::UnknownPlan);
        }
        let _execution = self.execution.try_lock().map_err(|_| CleanupError::Busy)?;
        let plan = self
            .plans
            .lock()
            .map_err(|_| CleanupError::Busy)?
            .remove(token)
            .ok_or(CleanupError::UnknownPlan)?;
        if plan.prepared.elapsed() > LIFETIME
            || chrono::Utc::now().timestamp_millis() >= plan.public.expires_at_ms
        {
            return Err(CleanupError::Expired);
        }
        let current = io::Root::open(base)?;
        if current.path != plan.root.path || !io::same_dir(&current.dir, &plan.root.dir)? {
            return Err(CleanupError::Changed);
        }
        let lock_dir = io::walk(
            &plan.root.dir,
            Path::new(crate::config_file::CONFIG_LOCK_DIRECTORY),
            true,
        )?;
        let _config_lock = io::try_lock(&lock_dir, crate::config_file::CONFIG_LOCK_FILENAME)?;
        let (doc, digest) = configuration(&plan.root)?;
        if digest != plan.config_digest {
            return Err(CleanupError::Changed);
        }
        let mut locks = Vec::new();
        for (path, dir) in &plan.parents {
            let current = io::walk(&plan.root.dir, path, false)?;
            if !io::same_dir(dir, &current)? {
                return Err(CleanupError::Changed);
            }
            if plan
                .entries
                .iter()
                .any(|entry| &entry.parent == path && entry.public.policy == "cache_expired")
            {
                locks.push(io::try_lock(dir, crate::cache::CACHE_LOCK_FILENAME)?);
            }
        }
        // Validate every approved object before making the first move.
        let mut budget = MAX_TOTAL_BYTES;
        for entry in &plan.entries {
            validate_entry(&plan, entry, &doc, &mut budget)?;
        }
        move_to_recovery(&plan, &doc)
    }
}

fn file_cap(policy: &str) -> Result<u64> {
    match policy {
        "cache_expired" => Ok(CACHE_FILE_CAP),
        "logs_30_days" => Ok(LOG_FILE_CAP),
        _ => Err(CleanupError::UnsafePath),
    }
}

fn configuration(root: &io::Root) -> Result<(toml_edit::DocumentMut, Option<String>)> {
    // Unlike the display-only inspector, mutation planning never reopens an
    // ambient path beneath the root. Every component is checked through its fd.
    for path in crate::RETIRED_LAYOUT_PATHS
        .iter()
        .map(|p| PathBuf::from(*p))
        .chain([
            crate::layout_migration_marker(Path::new("")),
            crate::extension_migration_marker(Path::new("")),
        ])
    {
        if io::metadata(&root.dir, &path)?.is_some() {
            return Err(CleanupError::NotReady);
        }
    }
    let (text, digest) = match root.dir.symlink_metadata("config.toml") {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), None),
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            let (snapshot, body, _) = io::read_file(
                &root.dir,
                std::ffi::OsStr::new("config.toml"),
                CONFIG_CAP,
                &mut (CONFIG_CAP + 1),
            )?;
            (
                String::from_utf8(body).map_err(|_| CleanupError::NotReady)?,
                Some(snapshot.digest),
            )
        }
        _ => return Err(CleanupError::UnsafePath),
    };
    let doc = text.parse().map_err(|_| CleanupError::NotReady)?;
    let version =
        crate::storage_diagnostics::validate_desktop(&doc).map_err(|_| CleanupError::NotReady)?;
    if version.is_none() {
        for name in [
            "models/providers.json",
            "tools/enabled.json",
            "skills/enabled.json",
        ] {
            if io::metadata(&root.dir, Path::new(name))?.is_some() {
                return Err(CleanupError::NotReady);
            }
        }
        if let Some(meta) = io::metadata(&root.dir, Path::new("agents"))? {
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err(CleanupError::UnsafePath);
            }
            let agents = io::walk(&root.dir, Path::new("agents"), false)?;
            for (index, item) in agents
                .entries()
                .map_err(|_| CleanupError::Unavailable)?
                .enumerate()
            {
                if index >= 1024 {
                    return Err(CleanupError::Budget);
                }
                let item = item.map_err(|_| CleanupError::Unavailable)?;
                if !item
                    .file_type()
                    .map_err(|_| CleanupError::Unavailable)?
                    .is_dir()
                {
                    continue;
                }
                let directory = io::walk(&agents, Path::new(&item.file_name()), false)?;
                for name in ["config.json", "skills-enabled.json"] {
                    if io::metadata(&directory, Path::new(name))?.is_some() {
                        return Err(CleanupError::NotReady);
                    }
                }
            }
        }
    }
    Ok((doc, digest))
}

fn eligible(
    root: &Path,
    relative: &Path,
    kind: &str,
    doc: &toml_edit::DocumentMut,
    body: &[u8],
    meta: &std::fs::Metadata,
    now: SystemTime,
) -> Result<()> {
    if relative
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(CleanupError::UnsafePath);
    }
    let name = relative.to_string_lossy().replace('\\', "/");
    match kind {
        "logs_30_days"
            if crate::storage_diagnostics::cleanup_policy(&name, meta, now)
                == Some("logs_30_days") =>
        {
            Ok(())
        }
        "cache_expired" => {
            for domain in [crate::cache::Domain::Models, crate::cache::Domain::Mcp] {
                let policy = crate::cache::policy_from_document(doc, domain)
                    .map_err(|_| CleanupError::NotReady)?;
                if !policy.enabled {
                    continue;
                }
                let Some(directory) = cache_relative(root, domain, &policy)? else {
                    continue;
                };
                if relative.parent() != Some(directory.as_path()) {
                    continue;
                }
                let Ok(text) = std::str::from_utf8(body) else {
                    continue;
                };
                if crate::cache::expired_owned_entry(
                    text,
                    &relative.file_name().unwrap().to_string_lossy(),
                    domain,
                    policy.ttl_seconds,
                    now.duration_since(SystemTime::UNIX_EPOCH)
                        .map_err(|_| CleanupError::NotReady)?
                        .as_secs(),
                )
                .unwrap_or(false)
                {
                    return Ok(());
                }
            }
            Err(CleanupError::Changed)
        }
        _ => Err(CleanupError::Changed),
    }
}

fn cache_relative(
    root: &Path,
    domain: crate::cache::Domain,
    policy: &crate::cache::Policy,
) -> Result<Option<PathBuf>> {
    let default = match domain {
        crate::cache::Domain::Models => crate::models_cache_dir(root),
        crate::cache::Domain::Mcp => crate::mcp_cache_dir(root),
    };
    let directory = policy
        .directory
        .as_ref()
        .map(|p| {
            if p.is_absolute() {
                p.clone()
            } else {
                root.join(p)
            }
        })
        .unwrap_or(default);
    let Ok(relative) = directory.strip_prefix(root) else {
        return Ok(None);
    };
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || crate::cache::CACHE_PROTECTED_SUBDIRS
            .iter()
            .any(|p| relative.starts_with(p))
    {
        return Err(CleanupError::UnsafePath);
    }
    Ok(Some(relative.to_path_buf()))
}

type Collected = (BTreeMap<PathBuf, Dir>, Vec<Entry>, bool);
fn collect_entries(root: &io::Root, doc: &toml_edit::DocumentMut) -> Result<Collected> {
    let mut scopes = Vec::new();
    let mut omitted = false;
    for domain in [crate::cache::Domain::Models, crate::cache::Domain::Mcp] {
        let policy =
            crate::cache::policy_from_document(doc, domain).map_err(|_| CleanupError::NotReady)?;
        match cache_relative(&root.path, domain, &policy)? {
            Some(relative) if policy.enabled => scopes.push((relative, Some(domain))),
            None => omitted = true,
            _ => {}
        }
    }
    if scopes.len() == 2
        && (scopes[0].0.starts_with(&scopes[1].0) || scopes[1].0.starts_with(&scopes[0].0))
    {
        scopes.clear();
        omitted = true;
    }
    scopes.push((PathBuf::from("logs"), None));
    let mut parents = BTreeMap::new();
    let mut entries = Vec::new();
    let mut budget = MAX_TOTAL_BYTES;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut inspected = 0;
    'scopes: for (parent, domain) in scopes {
        if io::metadata(&root.dir, &parent)?.is_none() {
            continue;
        }
        let dir = io::walk(&root.dir, &parent, false)?;
        parents.insert(
            parent.clone(),
            dir.try_clone().map_err(|_| CleanupError::Unavailable)?,
        );
        for item in dir.entries().map_err(|_| CleanupError::Unavailable)? {
            if entries.len() == MAX_BATCH || inspected >= 8192 || Instant::now() >= deadline {
                omitted = true;
                break 'scopes;
            }
            inspected += 1;
            let item = item.map_err(|_| CleanupError::Unavailable)?;
            let name = item.file_name();
            let name_text = name.to_string_lossy();
            if !item
                .file_type()
                .map_err(|_| CleanupError::Unavailable)?
                .is_file()
            {
                continue;
            }
            let relative = parent.join(&name);
            let kind = if let Some(domain) = domain {
                if !name_text.starts_with(&format!("astro-cache-v1-{}-", domain.key())) {
                    continue;
                }
                "cache_expired"
            } else {
                let date = name_text
                    .strip_prefix("agent.log.")
                    .or_else(|| name_text.strip_prefix("errors.log."));
                if date
                    .is_none_or(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err())
                {
                    continue;
                }
                let file = io::open_file(&dir, &name)?;
                if crate::storage_diagnostics::cleanup_policy(
                    &relative.to_string_lossy().replace('\\', "/"),
                    &file.metadata().map_err(|_| CleanupError::Unavailable)?,
                    SystemTime::now(),
                ) != Some("logs_30_days")
                {
                    continue;
                }
                "logs_30_days"
            };
            let Ok((snapshot, body, meta)) =
                io::read_file(&dir, &name, file_cap(kind)?, &mut budget)
            else {
                omitted = true;
                continue;
            };
            if eligible(
                &root.path,
                &relative,
                kind,
                doc,
                &body,
                &meta,
                SystemTime::now(),
            )
            .is_err()
            {
                continue;
            }
            let public = CleanupItem {
                path: relative.to_string_lossy().replace('\\', "/"),
                bytes: snapshot.bytes,
                policy: kind.into(),
            };
            entries.push(Entry {
                public,
                relative,
                parent: parent.clone(),
                snapshot,
            });
        }
    }
    Ok((parents, entries, omitted))
}

fn validate_entry(
    plan: &PendingPlan,
    entry: &Entry,
    doc: &toml_edit::DocumentMut,
    budget: &mut u64,
) -> Result<()> {
    let parent = plan
        .parents
        .get(&entry.parent)
        .ok_or(CleanupError::UnsafePath)?;
    let (snapshot, body, meta) = io::read_file(
        parent,
        entry.relative.file_name().ok_or(CleanupError::UnsafePath)?,
        file_cap(&entry.public.policy)?,
        budget,
    )?;
    if !entry.snapshot.matches(&snapshot) {
        return Err(CleanupError::Changed);
    }
    eligible(
        &plan.root.path,
        &entry.relative,
        &entry.public.policy,
        doc,
        &body,
        &meta,
        SystemTime::now(),
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryEntry {
    source: String,
    stored_name: String,
    bytes: u64,
    sha256: String,
    status: &'static str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryManifest {
    version: u32,
    batch_id: String,
    created_at: String,
    state: &'static str,
    entries: Vec<RecoveryEntry>,
}

fn move_to_recovery(plan: &PendingPlan, doc: &toml_edit::DocumentMut) -> Result<CleanupResult> {
    move_to_recovery_with(plan, doc, |_| {})
}

fn move_to_recovery_with(
    plan: &PendingPlan,
    doc: &toml_edit::DocumentMut,
    mut before_move: impl FnMut(usize),
) -> Result<CleanupResult> {
    let batch = uuid::Uuid::new_v4().to_string();
    let recovery_root = io::walk(&plan.root.dir, Path::new(RECOVERY_ROOT), true)?;
    recovery_root
        .create_dir(&batch)
        .map_err(|_| CleanupError::Unavailable)?;
    let dir = io::walk(&recovery_root, Path::new(&batch), false)?;
    io::private_directory(&dir)?;
    let mut manifest = RecoveryManifest {
        version: 1,
        batch_id: batch.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        state: "in_progress",
        entries: plan
            .entries
            .iter()
            .enumerate()
            .map(|(n, e)| RecoveryEntry {
                source: e.public.path.clone(),
                stored_name: format!("{n:02}.data"),
                bytes: e.snapshot.bytes,
                sha256: e.snapshot.digest.clone(),
                status: "pending",
            })
            .collect(),
    };
    io::write_manifest(
        &dir,
        &serde_json::to_vec_pretty(&manifest).map_err(|_| CleanupError::Unavailable)?,
    )?;
    io::sync_directory(&recovery_root)?;
    let mut result = CleanupResult {
        batch_id: batch.clone(),
        recovery_path: plan
            .root
            .path
            .join(RECOVERY_ROOT)
            .join(&batch)
            .to_string_lossy()
            .into_owned(),
        moved_files: 0,
        moved_bytes: 0,
        unverified_files: 0,
        manifest_complete: true,
        outcomes: vec![],
    };
    let mut budget = MAX_TOTAL_BYTES;
    let mut verification_budget = MAX_TOTAL_BYTES;
    for (index, entry) in plan.entries.iter().enumerate() {
        let parent = &plan.parents[&entry.parent];
        let stored = &manifest.entries[index].stored_name;
        let config_unchanged =
            configuration(&plan.root).is_ok_and(|(_, digest)| digest == plan.config_digest);
        let status = if !config_unchanged || validate_entry(plan, entry, doc, &mut budget).is_err()
        {
            "changed_not_moved"
        } else {
            before_move(index);
            if parent
                .rename(entry.relative.file_name().unwrap(), &dir, stored)
                .is_err()
            {
                "move_failed"
            } else {
                // Retain any unexpectedly changed object. Never delete it or
                // overwrite a concurrently-created source to attempt rollback.
                match io::read_file(
                    &dir,
                    std::ffi::OsStr::new(stored),
                    entry.snapshot.bytes,
                    &mut verification_budget,
                ) {
                    Ok((snapshot, _, _)) if entry.snapshot.matches(&snapshot) => {
                        result.moved_files += 1;
                        result.moved_bytes += snapshot.bytes;
                        "moved"
                    }
                    _ => {
                        result.unverified_files += 1;
                        "changed_in_recovery"
                    }
                }
            }
        };
        manifest.entries[index].status = status;
        result.outcomes.push(CleanupOutcome {
            path: entry.public.path.clone(),
            status: status.into(),
        });
        if io::sync_directory(parent)
            .and_then(|_| {
                io::write_manifest(
                    &dir,
                    &serde_json::to_vec_pretty(&manifest).map_err(|_| CleanupError::Unavailable)?,
                )
            })
            .is_err()
        {
            result.manifest_complete = false;
            break;
        }
    }
    for entry in plan.entries.iter().skip(result.outcomes.len()) {
        result.outcomes.push(CleanupOutcome {
            path: entry.public.path.clone(),
            status: "not_attempted".into(),
        });
    }
    if result.manifest_complete {
        manifest.state = if result.moved_files == plan.public.items.len() {
            "complete"
        } else {
            "partial"
        };
        result.manifest_complete = io::write_manifest(
            &dir,
            &serde_json::to_vec_pretty(&manifest).map_err(|_| CleanupError::Unavailable)?,
        )
        .is_ok();
    }
    Ok(result)
}

/// Validate the recovery location before Desktop reveals it. No arbitrary path argument.
pub fn recovery_path(base: &Path, batch: &str) -> Result<PathBuf> {
    let id = uuid::Uuid::parse_str(batch).map_err(|_| CleanupError::UnsafePath)?;
    if id.to_string() != batch {
        return Err(CleanupError::UnsafePath);
    }
    let root = io::Root::open(base)?;
    let relative = Path::new(RECOVERY_ROOT).join(batch);
    let dir = io::walk(&root.dir, &relative, false)?;
    let (_, body, _) = io::read_file(
        &dir,
        std::ffi::OsStr::new("manifest.json"),
        128 * 1024,
        &mut (128 * 1024 + 1),
    )?;
    let value: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| CleanupError::Unavailable)?;
    if value["version"] != 1 || value["batchId"] != batch {
        return Err(CleanupError::UnsafePath);
    }
    Ok(root.path.join(relative))
}

#[cfg(test)]
mod tests;
