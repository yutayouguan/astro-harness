//! Bounded, read-only home inspection. No bootstrap, SQLite, locks, network or cleanup.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_SAMPLES: usize = 30;
const MAX_DEPTH: usize = 24;
const SCAN_TIME: Duration = Duration::from_secs(2);

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

fn validate_desktop(doc: &toml_edit::DocumentMut) -> anyhow::Result<Option<i64>> {
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
    let mut domains = BTreeMap::<String, StorageDomain>::new();
    for dir in crate::DOMAIN_DIRS.iter().copied().chain(["backups"]) {
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
    };
    scanner.walk(Path::new(""), 0);
    let legacy_settings = scanner.legacy_settings;
    report.domains = domains.into_values().collect();
    // Direct status checks are independent of a potentially partial size scan.
    let marker = safe_metadata(&root, Path::new("backups/layout-in-progress.json"));
    if marker.is_err() {
        report.state = HomeState::Unreadable;
        issue(
            &mut report,
            "scan_unreadable",
            "backups/layout-in-progress.json",
        );
    } else if marker.is_ok_and(|v| v.is_some()) {
        report.state = HomeState::MigrationIncomplete;
        issue(
            &mut report,
            "migration_incomplete",
            "backups/layout-in-progress.json",
        );
    } else if crate::RETIRED_LAYOUT_PATHS
        .iter()
        .any(|p| root.join(p).symlink_metadata().is_ok())
    {
        report.state = HomeState::LayoutMigrationRequired;
        issue(&mut report, "layout_migration_required", "");
    }
    report.config_present = root.join("config.toml").symlink_metadata().is_ok();
    match read_small(&root, Path::new("config.toml"), MAX_CONFIG_BYTES) {
        Ok(Some(text)) => {
            report.config_present = true;
            match text
                .parse::<toml_edit::DocumentMut>()
                .ok()
                .and_then(|doc| validate_desktop(&doc).ok())
            {
                Some(version) => {
                    report.settings_version = version;
                }
                None => {
                    if report.state == HomeState::Ready {
                        report.state = HomeState::InvalidConfig;
                    }
                    issue(&mut report, "config_invalid", "config.toml");
                }
            }
        }
        Ok(None) => {}
        Err(_) => {
            if report.state == HomeState::Ready {
                report.state = HomeState::Unreadable;
            }
            issue(&mut report, "config_unreadable", "config.toml");
        }
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
    report
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
            let id = if self.domains.contains_key(top) {
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
            if let Some(policy) = cleanup_policy(&name, &meta, self.now) {
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

fn cleanup_policy(name: &str, meta: &fs::Metadata, now: SystemTime) -> Option<&'static str> {
    let age = now.duration_since(meta.modified().ok()?).ok()?.as_secs();
    if name.starts_with("models/cache/") && age >= 7 * 86400 {
        return Some("cache_7_days");
    }
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
        let report = inspect_at(root, &[], now, MAX_ENTRIES);
        let paths: BTreeSet<_> = report
            .cleanup_preview
            .iter()
            .map(|c| c.path.as_str())
            .collect();
        assert_eq!(
            paths,
            BTreeSet::from(["models/cache/old.json", "logs/agent.log.2020-01-01"])
        );
        assert!(root.join("models/cache/old.json").exists());
    }

    #[test]
    fn scan_and_preview_sizes_are_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for n in 0..40 {
            write(root, &format!("models/cache/{n}.json"), "x");
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
