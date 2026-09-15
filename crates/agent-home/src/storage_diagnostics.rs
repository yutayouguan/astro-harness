//! Bounded, read-only home inspection. No bootstrap, SQLite, locks, network or cleanup.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_SAMPLES: usize = 30;
const MAX_DEPTH: usize = 24;
const SCAN_TIME: Duration = Duration::from_secs(2);
const MAX_CACHE_ENTRY_BYTES: u64 = 512 * 1024;
const MAX_CACHE_READ_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeState {
    NewInstall,
    Ready,
    LayoutMigrationRequired,
    SettingsMigrationRequired,
    MigrationIncomplete,
    InvalidConfig,
    Unreadable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageIssue {
    pub code: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageDomain {
    pub id: String,
    pub bytes: u64,
    pub files: u64,
    pub skipped_links: u64,
    pub preview_bytes: u64,
    pub preview_files: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupCandidate {
    pub path: String,
    pub bytes: u64,
    pub policy: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePolicyReport {
    pub domain: String,
    pub directory: String,
    pub enabled: bool,
    pub ttl_seconds: u64,
    pub max_size_mb: u64,
    pub status: &'static str,
}

struct CacheInspection {
    domain: crate::cache::Domain,
    relative: PathBuf,
    policy: crate::cache::Policy,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageReport {
    pub root_path: String,
    pub config_path: String,
    pub state: HomeState,
    pub settings_version: Option<i64>,
    pub config_present: bool,
    pub partial: bool,
    pub inspected_entries: usize,
    pub domains: Vec<StorageDomain>,
    pub issues: Vec<StorageIssue>,
    pub cleanup_preview: Vec<CleanupCandidate>,
    pub cache_policies: Vec<CachePolicyReport>,
    pub preview_partial: bool,
}

#[derive(Default, Deserialize)]
struct DesktopShape {
    settings_version: Option<i64>,
    #[serde(default)]
    tools: BTreeMap<String, bool>,
    #[serde(default)]
    skills: BTreeMap<String, bool>,
    #[serde(default)]
    agent_skills: BTreeMap<String, BTreeMap<String, bool>>,
    #[serde(default)]
    agents: BTreeMap<String, serde_json::Value>,
    providers: Option<ProviderShape>,
}

#[derive(Deserialize)]
struct ProviderShape {
    #[serde(default)]
    providers: Vec<ProviderEntry>,
    active_provider_id: Option<String>,
}
#[derive(Deserialize)]
struct ProviderEntry {
    id: String,
    kind: String,
    endpoint: String,
    model: String,
    enabled: bool,
}

pub(crate) fn validate_desktop(doc: &toml_edit::DocumentMut) -> anyhow::Result<Option<i64>> {
    let shape: DesktopShape = crate::settings::get(doc, &["desktop"])?.unwrap_or_default();
    anyhow::ensure!(
        shape.settings_version.is_none_or(|v| v == 1),
        "unsupported version"
    );
    // Decode booleans and Agent wire settings without returning their values.
    let _ = (shape.tools, shape.skills, shape.agent_skills);
    for agent in shape.agents.into_values() {
        crate::settings::agent_from_wire(agent)?;
    }
    if let Some(providers) = shape.providers {
        let mut ids = BTreeSet::new();
        for entry in &providers.providers {
            anyhow::ensure!(
                !entry.id.trim().is_empty() && ids.insert(&entry.id),
                "invalid provider id"
            );
            anyhow::ensure!(!entry.kind.is_empty(), "invalid provider kind");
            let _ = (&entry.endpoint, &entry.model, entry.enabled);
        }
        if let Some(active) = providers.active_provider_id.filter(|id| !id.is_empty()) {
            anyhow::ensure!(
                providers.providers.iter().any(|p| p.id == active),
                "active provider missing"
            );
        }
    }
    Ok(shape.settings_version)
}

/// Inspect an explicit root without creating it. References are local UI-owned
/// paths only; outside-root references and links are reported as skipped.
pub fn inspect_home(base: &Path, references: &[String]) -> StorageReport {
    inspect_at(base, references, SystemTime::now(), MAX_ENTRIES)
}

fn inspect_at(base: &Path, references: &[String], now: SystemTime, limit: usize) -> StorageReport {
    let root = std::path::absolute(base).unwrap_or_else(|_| base.to_path_buf());
    let mut report = StorageReport {
        root_path: root.to_string_lossy().into_owned(),
        config_path: crate::config_path(&root).to_string_lossy().into_owned(),
        state: HomeState::Ready,
        settings_version: None,
        config_present: false,
        partial: false,
        inspected_entries: 0,
        domains: vec![],
        issues: vec![],
        cleanup_preview: vec![],
        cache_policies: vec![],
        preview_partial: false,
    };
    match fs::symlink_metadata(&root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            report.state = HomeState::NewInstall;
            return report;
        }
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
        _ => {
            report.state = HomeState::Unreadable;
            issue(&mut report, "root_unreadable", "");
            return report;
        }
    }
    let document = inspect_configuration(&root, &mut report);
    let caches = inspect_cache_policies(&root, document.as_ref(), &mut report);
    let mut domains = BTreeMap::<String, StorageDomain>::new();
    for dir in crate::DOMAIN_DIRS
        .iter()
        .copied()
        .chain(["backups", "mcp", "hooks"])
    {
        let id = dir.split('/').next().unwrap();
        domains.entry(id.into()).or_insert_with(|| StorageDomain {
            id: id.into(),
            ..Default::default()
        });
    }
    let mut scanner = Scanner {
        root: &root,
        now,
        deadline: Instant::now() + SCAN_TIME,
        remaining: limit,
        report: &mut report,
        domains: &mut domains,
        legacy_settings: false,
        caches: &caches,
        cache_bytes_remaining: MAX_CACHE_READ_BYTES,
    };
    scanner.walk(Path::new(""), 0);
    let legacy_settings = scanner.legacy_settings;
    report.domains = domains.into_values().collect();
    // Direct status checks are independent of a potentially partial size scan.
    for marker in [
        PathBuf::from("backups/layout-in-progress.json"),
        crate::extension_migration_marker(&root)
            .strip_prefix(&root)
            .unwrap()
            .to_path_buf(),
    ] {
        match safe_metadata(&root, &marker) {
            Ok(Some(_)) => {
                report.state = HomeState::MigrationIncomplete;
                issue(
                    &mut report,
                    "migration_incomplete",
                    &marker.to_string_lossy(),
                );
            }
            Err(_) => {
                if report.state != HomeState::MigrationIncomplete {
                    report.state = HomeState::Unreadable;
                }
                issue(&mut report, "scan_unreadable", &marker.to_string_lossy());
            }
            Ok(None) => {}
        }
    }
    if report.state != HomeState::MigrationIncomplete
        && report.state != HomeState::Unreadable
        && crate::RETIRED_LAYOUT_PATHS
            .iter()
            .any(|p| root.join(p).symlink_metadata().is_ok())
    {
        report.state = HomeState::LayoutMigrationRequired;
        issue(&mut report, "layout_migration_required", "");
    }
    if report.state == HomeState::Ready && report.settings_version.is_none() && legacy_settings {
        report.state = HomeState::SettingsMigrationRequired;
        issue(&mut report, "settings_migration_required", "");
    }
    if report.state == HomeState::Ready && report.settings_version.is_none() && report.partial {
        report.state = HomeState::Unreadable;
        issue(&mut report, "scan_unreadable", "");
    }
    if report.state == HomeState::Ready
        && !report.config_present
        && report.domains.iter().all(|d| d.files == 0)
        && !report.partial
    {
        report.state = HomeState::NewInstall;
    }
    for reference in references.iter().take(100) {
        check_reference(&root, Path::new(reference), &mut report);
    }
    if references.len() > 100 {
        report.partial = true;
    }
    // Active generated wallpaper is stored separately from WebView preferences.
    match read_small(&root, Path::new("ui/style/active.json"), 128 * 1024) {
        Ok(Some(text)) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(value) => {
                if let Some(path) = value.pointer("/wallpaper/path").and_then(|v| v.as_str()) {
                    let relative = Path::new(path);
                    if relative.is_absolute()
                        || relative
                            .components()
                            .any(|c| !matches!(c, Component::Normal(_)))
                    {
                        issue(&mut report, "resource_skipped", "ui/style/active.json");
                    } else {
                        check_reference(&root, &root.join("ui/style").join(relative), &mut report);
                    }
                }
            }
            Err(_) => issue(
                &mut report,
                "resource_manifest_invalid",
                "ui/style/active.json",
            ),
        },
        Err(_) => issue(&mut report, "resource_skipped", "ui/style/active.json"),
        Ok(None) => {}
    }
    if report.state != HomeState::Ready {
        report.cleanup_preview.clear();
        for domain in &mut report.domains {
            domain.preview_bytes = 0;
            domain.preview_files = 0;
        }
    }
    report
}

fn inspect_configuration(
    root: &Path,
    report: &mut StorageReport,
) -> Option<toml_edit::DocumentMut> {
    report.config_present = root.join("config.toml").symlink_metadata().is_ok();
    match read_small(root, Path::new("config.toml"), MAX_CONFIG_BYTES) {
        Ok(Some(text)) => match text.parse::<toml_edit::DocumentMut>() {
            Ok(doc) => match validate_desktop(&doc) {
                Ok(version) => {
                    report.settings_version = version;
                    Some(doc)
                }
                Err(_) => {
                    report.state = HomeState::InvalidConfig;
                    issue(report, "config_invalid", "config.toml");
                    None
                }
            },
            Err(_) => {
                report.state = HomeState::InvalidConfig;
                issue(report, "config_invalid", "config.toml");
                None
            }
        },
        Ok(None) => Some(toml_edit::DocumentMut::new()),
        Err(_) => {
            report.state = HomeState::Unreadable;
            issue(report, "config_unreadable", "config.toml");
            None
        }
    }
}

fn inspect_cache_policies(
    root: &Path,
    doc: Option<&toml_edit::DocumentMut>,
    report: &mut StorageReport,
) -> Vec<CacheInspection> {
    let Some(doc) = doc else {
        return vec![];
    };
    let mut inspections = Vec::new();
    for domain in [crate::cache::Domain::Models, crate::cache::Domain::Mcp] {
        let policy = match crate::cache::policy_from_document(doc, domain) {
            Ok(policy) => policy,
            Err(_) => {
                report.state = HomeState::InvalidConfig;
                issue(
                    report,
                    "cache_policy_invalid",
                    &format!("config.toml [cache.{}]", domain.key()),
                );
                continue;
            }
        };
        let default = match domain {
            crate::cache::Domain::Models => crate::models_cache_dir(root),
            crate::cache::Domain::Mcp => crate::mcp_cache_dir(root),
        };
        let directory = policy
            .directory
            .as_ref()
            .map(|path| {
                if path.is_absolute() {
                    path.clone()
                } else {
                    root.join(path)
                }
            })
            .unwrap_or(default);
        let mut status = "in_home";
        match directory.strip_prefix(root) {
            Err(_) => {
                status = "external";
                report.preview_partial = true;
                issue(
                    report,
                    "cache_external_not_scanned",
                    &directory.to_string_lossy(),
                );
            }
            Ok(relative) => {
                let safe = !relative.as_os_str().is_empty()
                    && relative
                        .components()
                        .all(|c| matches!(c, Component::Normal(_)))
                    && safe_metadata(root, relative).is_ok_and(|m| m.is_none_or(|m| m.is_dir()));
                if !safe {
                    status = "unverified";
                    report.preview_partial = true;
                    issue(
                        report,
                        "cache_directory_unverified",
                        &relative.to_string_lossy(),
                    );
                } else if crate::cache::directory(root, domain, &policy).is_err() {
                    status = "invalid";
                    report.state = HomeState::InvalidConfig;
                    issue(report, "cache_policy_invalid", &relative.to_string_lossy());
                } else {
                    inspections.push(CacheInspection {
                        domain,
                        relative: relative.to_path_buf(),
                        policy: policy.clone(),
                    });
                }
            }
        }
        report.cache_policies.push(CachePolicyReport {
            domain: domain.key().into(),
            directory: directory.to_string_lossy().into_owned(),
            enabled: policy.enabled,
            ttl_seconds: policy.ttl_seconds,
            max_size_mb: policy.max_size_mb,
            status,
        });
    }
    if inspections.len() == 2
        && (inspections[0]
            .relative
            .starts_with(&inspections[1].relative)
            || inspections[1]
                .relative
                .starts_with(&inspections[0].relative))
    {
        for policy in &mut report.cache_policies {
            if policy.status == "in_home" {
                policy.status = "overlap";
            }
        }
        inspections.clear();
        report.preview_partial = true;
        issue(report, "cache_directories_overlap", "config.toml");
    }
    inspections
}

fn issue(report: &mut StorageReport, code: &str, path: &str) {
    if report.issues.len() < 100
        && !report
            .issues
            .iter()
            .any(|i| i.code == code && i.path == path)
    {
        report.issues.push(StorageIssue {
            code: code.into(),
            path: path.into(),
        });
    }
}

struct Scanner<'a> {
    root: &'a Path,
    now: SystemTime,
    deadline: Instant,
    remaining: usize,
    report: &'a mut StorageReport,
    domains: &'a mut BTreeMap<String, StorageDomain>,
    legacy_settings: bool,
    caches: &'a [CacheInspection],
    cache_bytes_remaining: u64,
}
impl Scanner<'_> {
    fn walk(&mut self, relative: &Path, depth: usize) {
        if self.remaining == 0 || Instant::now() >= self.deadline || depth > MAX_DEPTH {
            self.report.partial = true;
            return;
        }
        let entries = match fs::read_dir(self.root.join(relative)) {
            Ok(v) => v,
            Err(_) => {
                self.report.partial = true;
                issue(self.report, "scan_unreadable", &relative.to_string_lossy());
                return;
            }
        };
        for entry in entries {
            if self.remaining == 0 || Instant::now() >= self.deadline {
                self.report.partial = true;
                break;
            }
            self.remaining -= 1;
            self.report.inspected_entries += 1;
            let entry = match entry {
                Ok(e) => e,
                Err(_) => {
                    self.report.partial = true;
                    continue;
                }
            };
            let rel = relative.join(entry.file_name());
            let meta = match entry.path().symlink_metadata() {
                Ok(m) => m,
                Err(_) => {
                    self.report.partial = true;
                    continue;
                }
            };
            let name = rel.to_string_lossy().replace('\\', "/");
            if is_legacy_setting(&name) {
                self.legacy_settings = true;
            }
            let top = name.split('/').next().unwrap_or("other");
            let cache = self
                .caches
                .iter()
                .find(|cache| rel.starts_with(&cache.relative));
            let id = if let Some(cache) = cache {
                cache.domain.key()
            } else if self.domains.contains_key(top) {
                top
            } else {
                "other"
            };
            let domain = self
                .domains
                .entry(id.into())
                .or_insert_with(|| StorageDomain {
                    id: id.into(),
                    ..Default::default()
                });
            if meta.file_type().is_symlink() {
                domain.skipped_links += 1;
                continue;
            }
            if meta.is_dir() {
                self.walk(&rel, depth + 1);
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            domain.bytes = domain.bytes.saturating_add(meta.len());
            domain.files += 1;
            let cache_candidate = cache
                .filter(|c| c.policy.enabled && rel.parent() == Some(c.relative.as_path()))
                .filter(|c| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(&format!("astro-cache-v1-{}-", c.domain.key()))
                });
            let mut candidate_policy = cleanup_policy(&name, &meta, self.now);
            if let Some(cache) = cache_candidate {
                // Charge the EOF-probe byte too; concurrent growth cannot
                // exceed the byte budget reserved for this entry.
                let cap = meta.len().min(MAX_CACHE_ENTRY_BYTES);
                let reserved = meta.len().saturating_add(1);
                if meta.len() > cap || reserved > self.cache_bytes_remaining {
                    self.report.preview_partial = true;
                    issue(self.report, "cache_entry_unverified", &name);
                } else {
                    self.cache_bytes_remaining =
                        self.cache_bytes_remaining.saturating_sub(reserved);
                    let expired =
                        read_small(self.root, &rel, cap)
                            .ok()
                            .flatten()
                            .and_then(|text| {
                                crate::cache::expired_owned_entry(
                                    &text,
                                    &entry.file_name().to_string_lossy(),
                                    cache.domain,
                                    cache.policy.ttl_seconds,
                                    self.now
                                        .duration_since(SystemTime::UNIX_EPOCH)
                                        .ok()?
                                        .as_secs(),
                                )
                                .ok()
                            });
                    match expired {
                        Some(true) => candidate_policy = Some("cache_expired"),
                        Some(false) => {}
                        None => {
                            self.report.preview_partial = true;
                            issue(self.report, "cache_entry_unverified", &name);
                        }
                    }
                }
            }
            if let Some(policy) = candidate_policy {
                domain.preview_bytes = domain.preview_bytes.saturating_add(meta.len());
                domain.preview_files += 1;
                if self.report.cleanup_preview.len() < MAX_SAMPLES {
                    self.report.cleanup_preview.push(CleanupCandidate {
                        path: name,
                        bytes: meta.len(),
                        policy,
                    });
                }
            }
        }
    }
}

fn is_legacy_setting(name: &str) -> bool {
    matches!(
        name,
        "models/providers.json" | "tools/enabled.json" | "skills/enabled.json"
    ) || (name.starts_with("agents/")
        && name.split('/').count() == 3
        && (name.ends_with("/config.json") || name.ends_with("/skills-enabled.json")))
}

pub(crate) fn cleanup_policy(
    name: &str,
    meta: &fs::Metadata,
    now: SystemTime,
) -> Option<&'static str> {
    let age = now.duration_since(meta.modified().ok()?).ok()?.as_secs();
    let filename = name.strip_prefix("logs/")?;
    let date = filename
        .strip_prefix("agent.log.")
        .or_else(|| filename.strip_prefix("errors.log."))?;
    let cutoff =
        chrono::DateTime::<chrono::Utc>::from(now).date_naive() - chrono::Duration::days(30);
    if !date.contains('/')
        && chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok_and(|date| date <= cutoff)
        && age >= 30 * 86400
    {
        Some("logs_30_days")
    } else {
        None
    }
}

fn safe_metadata(root: &Path, relative: &Path) -> std::io::Result<Option<fs::Metadata>> {
    let mut path = root.to_path_buf();
    let mut result = None;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(std::io::ErrorKind::InvalidInput.into());
        };
        path.push(name);
        let meta = match path.symlink_metadata() {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if meta.file_type().is_symlink() {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }
        result = Some(meta);
    }
    Ok(result)
}

fn read_small(root: &Path, relative: &Path, cap: u64) -> std::io::Result<Option<String>> {
    let Some(meta) = safe_metadata(root, relative)? else {
        return Ok(None);
    };
    if !meta.is_file() || meta.len() > cap {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let file = fs::File::open(root.join(relative))?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() > cap {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.dev() != opened.dev() || meta.ino() != opened.ino() {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
    }
    let mut bytes = Vec::new();
    file.take(cap + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| std::io::ErrorKind::InvalidData.into())
}

fn check_reference(root: &Path, path: &Path, report: &mut StorageReport) {
    let Ok(relative) = path.strip_prefix(root) else {
        issue(report, "resource_outside_root", "");
        return;
    };
    match safe_metadata(root, relative) {
        Ok(Some(meta)) if meta.is_file() => {}
        Ok(_) => issue(report, "resource_missing", &relative.to_string_lossy()),
        Err(_) => issue(report, "resource_skipped", &relative.to_string_lossy()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, body: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn owned_cache(root: &Path, directory: &str, domain: &str, id: u64, written_at: u64) -> String {
        let key = format!("{id:064x}");
        let relative = format!("{directory}/astro-cache-v1-{domain}-{key}.json");
        write(
            root,
            &relative,
            &serde_json::json!({"version":1,"domain":domain,"key":key,"written_at":written_at,
            "payload":{"private":"CACHE_PAYLOAD_MUST_NOT_APPEAR"}})
            .to_string(),
        );
        relative
    }

    #[test]
    fn missing_home_is_not_created() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("missing");
        assert_eq!(inspect_home(&root, &[]).state, HomeState::NewInstall);
        assert!(!root.exists());
    }

    #[test]
    fn inspect_never_opens_databases_or_returns_configuration_values() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(
            root,
            "config.toml",
            "[private]\nsecret = 'SENSITIVE_TEST_VALUE'\n",
        );
        write(root, ".env", "TOKEN=DO_NOT_READ_OR_RETURN");
        write(root, "sessions/state.db", "not a SQLite database");
        let before = fs::read(root.join("config.toml")).unwrap();
        let report = inspect_home(root, &[]);
        assert_eq!(report.state, HomeState::Ready);
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("SENSITIVE_TEST_VALUE"));
        assert!(!serialized.contains("DO_NOT_READ_OR_RETURN"));
        assert_eq!(fs::read(root.join("config.toml")).unwrap(), before);
        assert!(!root.join("sessions/state.db-wal").exists());
        assert!(!root.join("sessions/state.db-shm").exists());
        assert!(!root.join("security").exists());
    }

    #[test]
    fn invalid_config_is_reported_without_echoing_values_or_overwriting() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for body in [
            "secret = 'NEVER_ECHO_ME",
            "[desktop.tools]\nexec_command = 'NEVER_ECHO_ME'",
            "[desktop]\nsettings_version = 99",
        ] {
            write(root, "config.toml", body);
            let report = inspect_home(root, &[]);
            assert_eq!(report.state, HomeState::InvalidConfig);
            assert!(!serde_json::to_string(&report)
                .unwrap()
                .contains("NEVER_ECHO_ME"));
            assert_eq!(fs::read_to_string(root.join("config.toml")).unwrap(), body);
        }
    }

    #[test]
    fn layout_settings_and_incomplete_migration_are_distinct() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(root, "models/providers.json", "{}");
        assert_eq!(
            inspect_home(root, &[]).state,
            HomeState::SettingsMigrationRequired
        );
        write(root, "config.toml", "[desktop]\nsettings_version = 1\n");
        assert_eq!(inspect_home(root, &[]).state, HomeState::Ready);
        fs::create_dir(root.join("data")).unwrap();
        assert_eq!(
            inspect_home(root, &[]).state,
            HomeState::LayoutMigrationRequired
        );
        write(root, "backups/layout-in-progress.json", "{}");
        assert_eq!(
            inspect_home(root, &[]).state,
            HomeState::MigrationIncomplete
        );
    }

    #[test]
    fn previews_only_old_model_cache_and_dated_runtime_logs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let now = SystemTime::now();
        for (name, days) in [
            ("models/cache/old.json", 8),
            ("models/cache/new.json", 1),
            ("logs/agent.log.2020-01-01", 31),
            ("logs/errors.log.2020-01-01", 29),
            ("logs/unknown.log", 99),
            ("security/audit/sandbox.jsonl", 99),
            ("backups/layout/backup.tar.gz", 99),
            ("browser/profile/Cookies", 99),
            ("sessions/state.db", 99),
            ("ui/wallpapers/a.png", 99),
            ("workspace/file.txt", 99),
        ] {
            write(root, name, "123");
            fs::File::options()
                .write(true)
                .open(root.join(name))
                .unwrap()
                .set_times(
                    fs::FileTimes::new().set_modified(now - Duration::from_secs(days * 86400)),
                )
                .unwrap();
        }
        let owned = owned_cache(
            root,
            "models/cache",
            "models",
            1,
            now.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                - 601,
        );
        let report = inspect_at(root, &[], now, MAX_ENTRIES);
        let paths: BTreeSet<_> = report
            .cleanup_preview
            .iter()
            .map(|c| c.path.as_str())
            .collect();
        assert_eq!(
            paths,
            BTreeSet::from([owned.as_str(), "logs/agent.log.2020-01-01"])
        );
        assert!(root.join("models/cache/old.json").exists());
        assert!(!serde_json::to_string(&report)
            .unwrap()
            .contains("CACHE_PAYLOAD_MUST_NOT_APPEAR"));
    }

    #[test]
    fn scan_and_preview_sizes_are_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for n in 0..40 {
            owned_cache(root, "models/cache", "models", n, 0);
        }
        let report = inspect_at(
            root,
            &[],
            SystemTime::now() + Duration::from_secs(10 * 86400),
            MAX_ENTRIES,
        );
        assert_eq!(report.cleanup_preview.len(), MAX_SAMPLES);
        assert_eq!(
            report.domains.iter().map(|d| d.preview_files).sum::<u64>(),
            40
        );
        let limited = inspect_at(root, &[], SystemTime::now(), 2);
        assert!(limited.partial);
        assert!(limited.inspected_entries <= 2);
    }

    #[test]
    fn cache_preview_uses_configured_ttl_directory_and_enabled_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let now = SystemTime::now();
        let seconds = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        write(root, "config.toml", "[cache.models]\nttl_seconds=900\ndirectory='custom/models'\n[cache.mcp]\nenabled=false\n");
        let expired = owned_cache(root, "custom/models", "models", 1, seconds - 901);
        owned_cache(root, "custom/models", "models", 2, seconds - 900);
        owned_cache(root, "models/cache", "models", 3, 0); // retired default location is not the configured cache
        owned_cache(root, "mcp/cache", "mcp", 4, 0); // disabled domain is retained
        let report = inspect_at(root, &[], now, MAX_ENTRIES);
        assert_eq!(report.cleanup_preview.len(), 1);
        assert_eq!(report.cleanup_preview[0].path, expired);
        assert_eq!(report.cache_policies[0].ttl_seconds, 900);
        assert!(!report.cache_policies[1].enabled);
        write(root, "config.toml", "[cache.models]\nttl_seconds=900\ndirectory='custom/models'\n[cache.mcp]\nttl_seconds=1800\n");
        owned_cache(root, "mcp/cache", "mcp", 5, seconds - 1700);
        let report = inspect_at(root, &[], now, MAX_ENTRIES);
        assert_eq!(report.cache_policies[1].ttl_seconds, 1800);
        assert_eq!(
            report
                .domains
                .iter()
                .find(|d| d.id == "mcp")
                .unwrap()
                .preview_files,
            1
        );
        assert_eq!(report.cleanup_preview.len(), 2);
    }

    #[test]
    fn unverified_and_future_cache_files_never_become_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let now = SystemTime::now();
        let seconds = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let bad = owned_cache(root, "models/cache", "models", 1, 0);
        write(
            root,
            &bad,
            r#"{"version":1,"domain":"mcp","key":"wrong","written_at":0,"payload":"SECRET"}"#,
        );
        let large = owned_cache(root, "models/cache", "models", 2, 0);
        write(
            root,
            &large,
            &"x".repeat(MAX_CACHE_ENTRY_BYTES as usize + 1),
        );
        owned_cache(root, "models/cache", "models", 3, seconds + 100);
        let report = inspect_at(root, &[], now, MAX_ENTRIES);
        assert!(report.cleanup_preview.is_empty());
        assert!(report.preview_partial);
        assert_eq!(
            report
                .issues
                .iter()
                .filter(|i| i.code == "cache_entry_unverified")
                .count(),
            2
        );
        assert!(!serde_json::to_string(&report).unwrap().contains("SECRET"));
    }

    #[test]
    fn external_overlapping_and_invalid_cache_policies_are_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let external = tempfile::tempdir().unwrap();
        write(
            root,
            "config.toml",
            &format!(
                "[cache.models]\ndirectory={}\n",
                serde_json::to_string(&external.path().to_string_lossy()).unwrap()
            ),
        );
        let report = inspect_home(root, &[]);
        assert_eq!(report.cache_policies[0].status, "external");
        assert!(report.preview_partial);
        assert_eq!(fs::read_dir(external.path()).unwrap().count(), 0);
        write(root, "config.toml", "[cache.models]\ndirectory='shared-cache'\n[cache.mcp]\ndirectory='shared-cache/nested'\n");
        let report = inspect_home(root, &[]);
        assert!(report.cache_policies.iter().all(|p| p.status == "overlap"));
        assert!(!root.join("shared-cache").exists());
        write(root, "config.toml", "[cache.models]\nttl_seconds=0\n");
        assert_eq!(inspect_home(root, &[]).state, HomeState::InvalidConfig);
        write(
            root,
            "config.toml",
            "[cache.models]\ndirectory='ui/cache'\n",
        );
        assert_eq!(inspect_home(root, &[]).state, HomeState::InvalidConfig);
    }

    #[test]
    fn extension_migration_blocks_all_cleanup_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        owned_cache(root, "models/cache", "models", 1, 0);
        write(root, "backups/extensions-in-progress.json", "{}");
        let report = inspect_home(root, &[]);
        assert_eq!(report.state, HomeState::MigrationIncomplete);
        assert!(report
            .issues
            .iter()
            .any(|i| i.path == "backups/extensions-in-progress.json"));
        assert!(report.cleanup_preview.is_empty());
        assert_eq!(
            report.domains.iter().map(|d| d.preview_files).sum::<u64>(),
            0
        );
    }

    #[test]
    fn missing_wallpaper_does_not_invalidate_configuration() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(root, "config.toml", "[desktop]\nsettings_version=1\n");
        write(
            root,
            "ui/style/active.json",
            r#"{"wallpaper":{"path":"themes/demo/missing.png"}}"#,
        );
        let report = inspect_home(
            root,
            &[root
                .join("ui/wallpapers/missing.jpg")
                .to_string_lossy()
                .into_owned()],
        );
        assert_eq!(report.state, HomeState::Ready);
        assert_eq!(
            report
                .issues
                .iter()
                .filter(|i| i.code == "resource_missing")
                .count(),
            2
        );
    }

    #[cfg(unix)]
    #[test]
    fn never_follows_links_into_external_files_or_directory_loops() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let external = tempfile::tempdir().unwrap();
        write(external.path(), "config.toml", "password='OUTSIDE_SECRET'");
        std::os::unix::fs::symlink(
            external.path().join("config.toml"),
            root.join("config.toml"),
        )
        .unwrap();
        std::os::unix::fs::symlink(root, root.join("loop")).unwrap();
        let report = inspect_home(root, &[]);
        assert_eq!(report.state, HomeState::Unreadable);
        assert_eq!(
            report.domains.iter().map(|d| d.skipped_links).sum::<u64>(),
            2
        );
        assert!(!serde_json::to_string(&report)
            .unwrap()
            .contains("OUTSIDE_SECRET"));
        assert!(!root.join("security").exists());
    }
}
