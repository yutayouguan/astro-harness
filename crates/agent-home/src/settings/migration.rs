//! Explicit, conservative migration of editable JSON settings to config.toml.

use super::{get, lock_file, path, put, read_document, write_document};
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use toml_edit::DocumentMut;

#[derive(Debug, serde::Serialize)]
pub struct MigrationReport {
    pub already_migrated: bool,
    pub applied: bool,
    pub sources: Vec<PathBuf>,
    pub backup: Option<PathBuf>,
}

fn version(doc: &DocumentMut) -> Result<Option<i64>> {
    let Some(desktop) = doc.get("desktop") else {
        return Ok(None);
    };
    ensure!(desktop.is_table_like(), "desktop must be a table");
    desktop
        .get("settings_version")
        .map(|value| {
            value
                .as_integer()
                .context("desktop.settings_version must be an integer")
        })
        .transpose()
}

pub fn canonical_agent_id(id: &str) -> String {
    if matches!(id, "workspace" | "default") {
        "default".into()
    } else {
        crate::normalize_agent_id(id)
    }
}

/// Known data-layout variants are migration inputs, never runtime fallback paths.
fn legacy_sources(base: &Path) -> Result<Vec<(PathBuf, Vec<String>)>> {
    let mut sources = Vec::new();
    for (relative, section) in [
        ("providers.json", "providers"),
        ("models/providers.json", "providers"),
        ("tools-enabled.json", "tools"),
        ("tools/enabled.json", "tools"),
        ("skills-enabled.json", "skills"),
        ("skills/enabled.json", "skills"),
    ] {
        let file = base.join(relative);
        if file.try_exists()? {
            sources.push((file, vec!["desktop".into(), section.into()]));
        }
    }
    let agents = base.join("agents");
    if agents.is_dir() {
        ensure!(
            agents.canonicalize()?.starts_with(base.canonicalize()?),
            "agent configuration directory escapes Astro home"
        );
        for entry in fs::read_dir(agents)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let id = canonical_agent_id(&entry.file_name().to_string_lossy());
            for (file, section) in [
                ("config.json", "agents"),
                ("skills-enabled.json", "agent_skills"),
            ] {
                let file = entry.path().join(file);
                if file.try_exists()? {
                    sources.push((file, vec!["desktop".into(), section.into(), id.clone()]));
                }
            }
        }
    }
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    if !sources.is_empty() {
        let root = base.canonicalize()?;
        for (source, _) in &sources {
            ensure!(
                source.canonicalize()?.starts_with(&root),
                "migration input escapes Astro home: {}",
                source.display()
            );
        }
    }
    Ok(sources)
}

pub fn require_migrated(base: &Path, doc: &DocumentMut) -> Result<()> {
    if let Some(version) = version(doc)? {
        ensure!(
            version == 1,
            "unsupported desktop settings version {version}"
        );
        return Ok(());
    }
    ensure!(legacy_sources(base)?.is_empty(),
        "editable JSON settings require migration: stop Astro, then run cargo run -p home --bin astro-migrate-config -- --root <Astro home> --apply (omit --apply for a dry run)");
    Ok(())
}

fn reject_plaintext_credentials(value: &Value) -> Result<()> {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                ensure!(!matches!(key.to_ascii_lowercase().as_str(), "api_key" | "password" | "access_token" | "refresh_token" | "secret_key"),
                "legacy settings contain a credential field; move credentials to secure storage before migration");
                reject_plaintext_credentials(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                reject_plaintext_credentials(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Dry-run is read-only. Apply requires the application to be stopped. Sources
/// remain untouched; a completed migration never reimports subsequent JSON edits.
pub fn migrate(base: &Path, apply: bool) -> Result<MigrationReport> {
    ensure!(base.is_dir(), "Astro home does not exist");
    // Layout migration must finish first. In particular, retaining a flat
    // providers.json here must not leave the new runtime permanently blocked.
    if apply {
        crate::require_current_layout(base)?;
    }
    let config_path = path(base);
    if apply
        && config_path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        anyhow::bail!("refusing to replace a symlink configuration");
    }
    let _guard = if apply {
        Some(lock_file(&config_path)?)
    } else {
        None
    };
    let mut doc = read_document(&config_path)?;
    if let Some(version) = version(&doc)? {
        ensure!(
            version == 1,
            "unsupported desktop settings version {version}"
        );
        return Ok(MigrationReport {
            already_migrated: true,
            applied: false,
            sources: vec![],
            backup: None,
        });
    }
    let sources = legacy_sources(base)?;
    let mut snapshots = Vec::new();
    let mut sections = BTreeMap::<Vec<String>, Value>::new();
    for (file, keys) in &sources {
        ensure!(
            !fs::symlink_metadata(file)?.file_type().is_symlink(),
            "refusing symlink migration input: {}",
            file.display()
        );
        let bytes = fs::read(file)?;
        let mut value: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid JSON in {}", file.display()))?;
        reject_plaintext_credentials(&value)?;
        if keys.get(1).map(String::as_str) == Some("agents") {
            let mut agent: crate::AgentRuntimeConfig =
                serde_json::from_value(value).map_err(|_| {
                    anyhow::anyhow!(
                        "invalid Agent configuration fields in {} (contents omitted)",
                        file.display()
                    )
                })?;
            agent.id = canonical_agent_id(&agent.id);
            ensure!(
                Some(&agent.id) == keys.get(2),
                "agent ID does not match its directory: {}",
                file.display()
            );
            value = super::agent_to_wire(&agent)?;
        } else {
            super::omit_null_fields(&mut value);
        }
        ensure!(
            value.is_object(),
            "settings section must be an object: {}",
            file.display()
        );
        if let Some(previous) = sections.insert(keys.clone(), value.clone()) {
            ensure!(
                previous == value,
                "conflicting legacy files for {}; resolve before migration",
                keys.join(".")
            );
        }
        snapshots.push((file.clone(), bytes));
    }
    for (keys, value) in sections {
        let keys: Vec<_> = keys.iter().map(String::as_str).collect();
        if let Some(existing) = get::<Value>(&doc, &keys)? {
            ensure!(
                existing == value,
                "TOML/JSON conflict in {}; resolve before migration",
                keys.join(".")
            );
        } else {
            put(&mut doc, &keys, &value)?;
        }
    }
    if doc.get("desktop").is_none() {
        doc["desktop"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    ensure!(doc["desktop"].is_table_like(), "desktop must be a table");
    doc["desktop"]["settings_version"] = toml_edit::value(1);
    let mut report = MigrationReport {
        already_migrated: false,
        applied: false,
        sources: sources.into_iter().map(|(path, _)| path).collect(),
        backup: None,
    };
    if !apply {
        return Ok(report);
    }

    ensure!(
        !base
            .join("backups")
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink()),
        "backup directory must not be a symlink"
    );
    let backup = base
        .join("backups")
        .join(format!("global-config-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&backup)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&backup, fs::Permissions::from_mode(0o700))?;
    }
    if config_path.is_file() {
        fs::copy(&config_path, backup.join("config.toml"))?;
    }
    for (source, bytes) in &snapshots {
        let dest = backup.join(source.strip_prefix(base)?);
        fs::create_dir_all(dest.parent().context("backup has no parent")?)?;
        fs::write(dest, bytes)?;
    }
    for (source, bytes) in &snapshots {
        ensure!(
            fs::read(source)? == *bytes,
            "migration source changed; stop Astro and retry"
        );
    }
    write_document(&config_path, &doc)?;
    report.applied = true;
    report.backup = Some(backup);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn migration_does_not_follow_inputs_outside_the_requested_home() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("providers.json"), r#"{"providers":[]}"#).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("models")).unwrap();
        assert!(migrate(dir.path(), false)
            .unwrap_err()
            .to_string()
            .contains("escapes Astro home"));
        assert!(!dir.path().join("backups").exists());
    }

    #[test]
    fn imports_domain_layout_and_never_reimports_old_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("models")).unwrap();
        fs::create_dir_all(dir.path().join("skills")).unwrap();
        fs::write(
            dir.path().join("models/providers.json"),
            r#"{"providers":[]}"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("skills/enabled.json"),
            r#"{"/skills/demo":false}"#,
        )
        .unwrap();
        migrate(dir.path(), true).unwrap();
        fs::write(
            dir.path().join("skills/enabled.json"),
            r#"{"/skills/demo":true}"#,
        )
        .unwrap();
        let current: Value = super::super::read(dir.path(), &["desktop", "skills"])
            .unwrap()
            .unwrap();
        assert_eq!(current["/skills/demo"], false);
    }

    #[test]
    fn dry_run_then_apply_is_lossless_and_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("models")).unwrap();
        fs::create_dir_all(dir.path().join("tools")).unwrap();
        fs::write(path(dir.path()), "# preserve\n[mcp_servers]\n").unwrap();
        fs::write(
            dir.path().join("models/providers.json"),
            r#"{"providers":[],"active_provider_id":null}"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("tools/enabled.json"),
            r#"{"exec_command":false}"#,
        )
        .unwrap();
        let preview = migrate(dir.path(), false).unwrap();
        assert!(!preview.applied && preview.sources.len() == 2);
        assert!(!dir.path().join("backups").exists());
        assert!(super::super::read::<Value>(dir.path(), &["desktop", "tools"]).is_err());
        let done = migrate(dir.path(), true).unwrap();
        assert!(done.backup.unwrap().join("tools/enabled.json").is_file());
        assert!(dir.path().join("tools/enabled.json").is_file());
        assert!(fs::read_to_string(path(dir.path()))
            .unwrap()
            .contains("# preserve"));
        assert_eq!(
            super::super::read::<Value>(dir.path(), &["desktop", "tools"])
                .unwrap()
                .unwrap()["exec_command"],
            false
        );
        assert!(migrate(dir.path(), true).unwrap().already_migrated);
    }

    #[test]
    fn refuses_conflicts_and_credentials_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("tools")).unwrap();
        let original = "[desktop.tools]\nexec_command = true\n";
        fs::write(path(dir.path()), original).unwrap();
        fs::write(
            dir.path().join("tools/enabled.json"),
            r#"{"exec_command":false}"#,
        )
        .unwrap();
        assert!(migrate(dir.path(), true).is_err());
        assert_eq!(fs::read_to_string(path(dir.path())).unwrap(), original);
        fs::write(
            dir.path().join("tools/enabled.json"),
            r#"{"api_key":"do-not-copy"}"#,
        )
        .unwrap();
        assert!(migrate(dir.path(), true)
            .unwrap_err()
            .to_string()
            .contains("credential"));
        assert_eq!(fs::read_to_string(path(dir.path())).unwrap(), original);
    }

    #[test]
    fn old_layout_can_be_previewed_but_cannot_be_applied_or_initialized() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("providers.json"), r#"{"providers":[]}"#).unwrap();
        let preview = migrate(dir.path(), false).unwrap();
        assert_eq!(preview.sources.len(), 1);
        assert!(migrate(dir.path(), true)
            .unwrap_err()
            .to_string()
            .contains("retired layout"));
        assert!(!path(dir.path()).exists());
        assert!(!dir.path().join("backups").exists());
        assert!(crate::ensure_workspace(dir.path()).is_err());
    }
}
