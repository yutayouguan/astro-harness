//! Explicit offline extension migration. Preview never creates files.
use agent_config::sources::{self, Domain};
use anyhow::{ensure, Context, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use toml::Value;

#[derive(Debug, serde::Serialize)]
pub struct Report {
    pub applied: bool,
    pub changed_files: Vec<PathBuf>,
    pub backup: Option<PathBuf>,
}

pub fn marker(base: &Path) -> PathBuf {
    home::extension_migration_marker(base)
}

/// Only global sources are migrated. Project-owned hashes cannot grant global
/// trust; project hook references must be added explicitly by their owner.
pub fn migrate(base: &Path, apply: bool) -> Result<Report> {
    let base = base.canonicalize()?;
    ensure!(
        base.parent().is_some() && Some(&base) != home::user_home_dir().as_ref(),
        "refusing broad migration root"
    );
    ensure!(
        !base.join(".git").exists(),
        "expected Astro home, not a repository root"
    );
    ensure!(
        !marker(&base).exists(),
        "unfinished extension migration; restore the backup referenced by the marker first"
    );
    let root = home::config_path(&base);
    ensure!(
        !root
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink()),
        "refusing symlink config root"
    );
    let mut value = sources::read_root(&root)?;
    let original = value.clone();
    // The older observational shell bus used string entries in [hooks].
    let mut shell = BTreeMap::new();
    if let Some(hooks) = value.get_mut("hooks").and_then(Value::as_table_mut) {
        let keys: Vec<_> = hooks
            .iter()
            .filter(|(_, v)| v.is_str())
            .map(|(k, _)| k.clone())
            .collect();
        for key in keys {
            shell.insert(key.clone(), hooks.remove(&key).unwrap());
        }
    }
    if !shell.is_empty() {
        let table = value.as_table_mut().context("config must be a table")?;
        let target = table
            .entry("shell_hooks")
            .or_insert_with(|| Value::Table(Default::default()))
            .as_table_mut()
            .context("shell_hooks must be a table")?;
        for (key, command) in shell {
            ensure!(
                target.get(&key).is_none_or(|old| old == &command),
                "conflicting shell hook definitions"
            );
            target.insert(key, command);
        }
    }
    let legacy = base.join("hooks.json");
    if legacy.exists() {
        let canonical_legacy = legacy.canonicalize()?;
        let current = sources::documents(&root, &value, Domain::Hooks, &base)?;
        if !current.iter().any(|doc| doc.path == canonical_legacy) {
            let sources = value
                .as_table_mut()
                .unwrap()
                .entry("config_sources")
                .or_insert_with(|| Value::Table(Default::default()))
                .as_table_mut()
                .context("config_sources must be a table")?;
            sources
                .entry("hooks")
                .or_insert_with(|| Value::Array(vec![]))
                .as_array_mut()
                .context("config_sources.hooks must be an array")?
                .push(Value::String("hooks.json".into()));
        }
    }
    let mut documents = BTreeMap::new();
    for domain in [Domain::Mcp, Domain::Hooks] {
        for doc in sources::documents(&root, &value, domain, &base)? {
            documents.insert(doc.path, doc.value);
        }
    }
    let trust_path = home::hook_trust_path(&base);
    let trust_before = if trust_path.exists() {
        Some(fs::read(&trust_path)?)
    } else {
        None
    };
    let mut hashes = crate::trust::load(&base)?;
    ensure!(
        trust_before
            == if trust_path.exists() {
                Some(fs::read(&trust_path)?)
            } else {
                None
            },
        "hook trust changed during migration planning"
    );
    let old_hashes = hashes.clone();
    let mut snapshots = BTreeMap::<PathBuf, Option<Vec<u8>>>::new();
    let mut changes = BTreeMap::<PathBuf, Vec<u8>>::new();
    for (path, mut doc) in documents {
        let bytes = if path.exists() {
            Some(fs::read(&path)?)
        } else {
            None
        };
        let before = if path == root {
            original.clone()
        } else {
            doc.clone()
        };
        let is_json = path.extension().and_then(|e| e.to_str()) == Some("json");
        let snapshot_value: Value = if is_json {
            Value::try_from(serde_json::from_slice::<serde_json::Value>(
                bytes.as_deref().unwrap_or(b"{}"),
            )?)?
        } else {
            std::str::from_utf8(bytes.as_deref().unwrap_or(b""))?.parse()?
        };
        ensure!(
            snapshot_value == before,
            "configuration changed during migration planning"
        );
        if let Some(states) = doc
            .get_mut("hooks")
            .and_then(|v| v.get_mut("state"))
            .and_then(Value::as_table_mut)
        {
            for (key, state) in states {
                if let Some(hash) = state.as_table_mut().and_then(|v| v.remove("trusted_hash")) {
                    let hash = hash.as_str().context("trusted_hash must be a string")?;
                    crate::trust::validate_hash(hash)?;
                    let key = crate::trust::canonical_key(key);
                    ensure!(
                        hashes.get(&key).is_none_or(|old| old == hash),
                        "conflicting hook approval hash"
                    );
                    hashes.insert(key, hash.into());
                }
            }
        }
        if let Some(servers) = doc.get_mut("mcp_servers").and_then(Value::as_table_mut) {
            for (_, server) in servers {
                if let Some(server) = server.as_table_mut() {
                    server.remove("discovered");
                }
            }
        }
        if doc != before {
            let rendered = if is_json {
                serde_json::to_vec_pretty(&doc)?
            } else {
                let mut text = std::str::from_utf8(bytes.as_deref().unwrap_or(b""))?.parse()?;
                home::settings::replace_changed_root(&mut text, &before, &doc)?;
                text.to_string().into_bytes()
            };
            changes.insert(path.clone(), rendered);
        }
        snapshots.insert(path, bytes);
    }
    if hashes != old_hashes {
        snapshots.insert(trust_path.clone(), trust_before);
        changes.insert(
            trust_path,
            serde_json::to_vec_pretty(&serde_json::json!({"version":1,"hashes":hashes}))?,
        );
    }
    let mut report = Report {
        applied: false,
        changed_files: changes.keys().cloned().collect(),
        backup: None,
    };
    if !apply || changes.is_empty() {
        return Ok(report);
    }
    home::require_current_layout(&base)?;
    let mut locks = Vec::new();
    let mut directories = BTreeSet::new();
    for path in snapshots.keys() {
        if directories.insert(path.parent()) {
            locks.push(home::config_file::lock_config_file(path)?);
        }
    }
    for (path, expected) in &snapshots {
        let actual = if path.exists() {
            Some(fs::read(path)?)
        } else {
            None
        };
        ensure!(
            &actual == expected,
            "configuration changed during migration; rerun preview"
        );
    }
    let backups = home::backups_dir(&base);
    fs::create_dir_all(&backups)?;
    let backup = tempfile::Builder::new()
        .prefix("extensions-")
        .tempdir_in(&backups)?
        .keep();
    let mut manifest = Vec::new();
    for (index, (path, bytes)) in snapshots.iter().enumerate() {
        if let Some(bytes) = bytes {
            fs::write(backup.join(format!("{index}.original")), bytes)?;
        }
        manifest.push(serde_json::json!({"path":path,"original":bytes.as_ref().map(|_| format!("{index}.original"))}));
    }
    fs::write(
        backup.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    home::config_file::write_config_file(
        &marker(&base),
        &serde_json::to_string(&serde_json::json!({"backup":backup}))?,
    )?;
    // Any interruption leaves the marker in place and runtime fails closed.
    for (path, bytes) in &changes {
        home::config_file::write_config_file(path, std::str::from_utf8(bytes)?)?;
    }
    crate::config::load_config(&base)?;
    crate::command::CommandHookRunner::validate_migrated_home(&base)?;
    fs::remove_file(marker(&base))?;
    report.applied = true;
    report.backup = Some(backup);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_preserves_existing_hash_without_granting_new_trust() {
        let dir = tempfile::tempdir().unwrap();
        let root = home::config_path(dir.path());
        let body =
            "[[hooks.PreToolUse]]\n[[hooks.PreToolUse.hooks]]\ntype='command'\ncommand='true'\n";
        fs::write(&root, body).unwrap();
        let initial = crate::command::CommandHookRunner::load(dir.path())
            .unwrap()
            .list()
            .remove(0);
        fs::write(
            &root,
            format!(
                "{body}\n[hooks.state.{:?}]\nenabled=false\ntrusted_hash={:?}\n",
                initial.key, initial.current_hash
            ),
        )
        .unwrap();
        assert!(crate::command::CommandHookRunner::load(dir.path()).is_err());
        migrate(dir.path(), true).unwrap();
        let current = crate::command::CommandHookRunner::load(dir.path())
            .unwrap()
            .list()
            .remove(0);
        assert_eq!(initial.current_hash, current.current_hash);
        assert!(!current.enabled);
        assert_eq!(
            crate::trust::load(dir.path()).unwrap()[&initial.key],
            initial.current_hash
        );
        assert!(!fs::read_to_string(root).unwrap().contains("trusted_hash"));
        fs::write(marker(dir.path()), "interrupted").unwrap();
        assert!(home::require_current_layout(dir.path()).is_err());
        assert!(crate::command::CommandHookRunner::load(dir.path()).is_err());
    }
    #[test]
    fn preview_is_read_only_and_apply_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let root = home::config_path(dir.path());
        fs::write(&root, "# keep me\n[hooks]\nPostToolUse='true'\n[mcp_servers.demo]\ncommand='echo'\ndiscovered=[]\n").unwrap();
        fs::write(
            dir.path().join("hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"true"}]}]}}"#,
        )
        .unwrap();
        let old = fs::read(&root).unwrap();
        assert!(!migrate(dir.path(), false).unwrap().applied);
        assert_eq!(fs::read(&root).unwrap(), old);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        let report = migrate(dir.path(), true).unwrap();
        assert!(report.backup.unwrap().join("manifest.json").exists());
        let text = fs::read_to_string(&root).unwrap();
        assert!(text.contains("# keep me") && text.contains("shell_hooks"));
        assert!(!text.contains("discovered"));
        assert_eq!(
            crate::command::CommandHookRunner::load(dir.path())
                .unwrap()
                .handler_count(),
            1
        );
        assert!(migrate(dir.path(), true).unwrap().changed_files.is_empty());
    }
}
