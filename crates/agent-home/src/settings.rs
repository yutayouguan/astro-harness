//! Global editable settings. Runtime state and credentials do not belong here.
//!
//! Every writer owns a section, shares an OS file lock, and preserves other TOML
//! sections/comments. Legacy JSON is imported only by the explicit migration tool.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use toml_edit::{DocumentMut, Item, Table};

pub mod migration;

pub use crate::config_file::{lock_config_file as lock_file, ConfigWriteGuard as WriteGuard};

pub fn path(base: &Path) -> PathBuf {
    crate::config_path(base)
}

pub fn read_document(path: &Path) -> Result<DocumentMut> {
    match fs::read_to_string(path) {
        Ok(text) => text.parse().map_err(|_| {
            anyhow::anyhow!(
                "invalid TOML in {} (configuration contents omitted)",
                path.display()
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

/// Call under lock_file. Atomic replacement works on Unix and Windows.
pub fn write_document(path: &Path, doc: &DocumentMut) -> Result<()> {
    crate::config_file::write_config_file(path, &doc.to_string())
}

/// Compare the editor's own before/after representation, not its projection
/// against TOML. This preserves untouched TOML datetimes and comments even when
/// the editor uses a JSON/YAML value tree without TOML's native date type.
pub fn replace_changed_root<T: Serialize>(
    doc: &mut DocumentMut,
    before: &T,
    after: &T,
) -> Result<()> {
    let previous = serde_json::to_value(before)?;
    let updated = serde_json::to_value(after)?;
    ensure!(
        previous.is_object() && updated.is_object(),
        "configuration root must be a table"
    );
    let rendered = toml_edit::ser::to_document(&toml::Value::try_from(after)?)?;
    patch_changed_items(doc.as_item_mut(), &previous, &updated, rendered.as_item())
}

fn patch_changed_items(
    target: &mut Item,
    before: &serde_json::Value,
    after: &serde_json::Value,
    rendered: &Item,
) -> Result<()> {
    if before == after {
        return Ok(());
    }
    if let (Some(previous), Some(updated), Some(table)) = (
        before.as_object(),
        after.as_object(),
        target.as_table_like_mut(),
    ) {
        let removed: Vec<_> = table
            .iter()
            .filter(|(key, _)| !updated.contains_key(*key))
            .map(|(key, _)| key.to_string())
            .collect();
        for key in removed {
            table.remove(&key);
        }
        for (key, value) in updated {
            let encoded = rendered
                .get(key)
                .context("missing rendered configuration key")?;
            if let (Some(old), Some(item)) = (previous.get(key), table.get_mut(key)) {
                patch_changed_items(item, old, value, encoded)?;
            } else {
                table.insert(key, encoded.clone());
            }
        }
    } else {
        *target = rendered.clone();
    }
    Ok(())
}

fn section<'a>(doc: &'a DocumentMut, keys: &[&str]) -> Result<Option<&'a Item>> {
    let mut item = doc.as_item();
    for key in keys {
        ensure!(
            item.is_table_like(),
            "config section parent is not a table: {key}"
        );
        let Some(next) = item.get(key) else {
            return Ok(None);
        };
        item = next;
    }
    Ok(Some(item))
}

pub fn get<T: DeserializeOwned>(doc: &DocumentMut, keys: &[&str]) -> Result<Option<T>> {
    let Some(item) = section(doc, keys)? else {
        return Ok(None);
    };
    let mut wrapper = DocumentMut::new();
    wrapper["value"] = item.clone();
    #[derive(serde::Deserialize)]
    struct Wrapped<T> {
        value: T,
    }
    let decoded: Wrapped<T> = toml::from_str(&wrapper.to_string()).map_err(|_| {
        anyhow::anyhow!(
            "invalid config section {} (check field types; contents omitted)",
            keys.join(".")
        )
    })?;
    Ok(Some(decoded.value))
}

pub fn put<T: Serialize>(doc: &mut DocumentMut, keys: &[&str], value: &T) -> Result<()> {
    ensure!(!keys.is_empty(), "section path must not be empty");
    let item = Item::Table(toml_edit::ser::to_document(value)?.into_table());
    let mut parent = doc.as_item_mut();
    for key in &keys[..keys.len() - 1] {
        if parent.get(key).is_none() {
            parent[key] = Item::Table(Table::new());
        }
        ensure!(
            parent[key].is_table_like(),
            "config section is not a table: {key}"
        );
        parent = &mut parent[key];
    }
    ensure!(
        parent.is_table_like(),
        "config section parent is not a table"
    );
    parent[keys[keys.len() - 1]] = item;
    Ok(())
}

pub fn read<T: DeserializeOwned>(base: &Path, keys: &[&str]) -> Result<Option<T>> {
    crate::require_current_layout(base)?;
    let doc = read_document(&path(base))?;
    migration::require_migrated(base, &doc)?;
    get(&doc, keys)
}

pub fn update<R>(base: &Path, edit: impl FnOnce(&mut DocumentMut) -> Result<R>) -> Result<R> {
    crate::require_current_layout(base)?;
    let config = path(base);
    let _guard = lock_file(&config)?;
    let mut doc = read_document(&config)?;
    migration::require_migrated(base, &doc)?;
    let result = edit(&mut doc)?;
    if doc.get("desktop").is_none() {
        doc["desktop"] = Item::Table(Table::new());
    }
    ensure!(doc["desktop"].is_table_like(), "desktop must be a table");
    doc["desktop"]["settings_version"] = toml_edit::value(1);
    write_document(&config, &doc)?;
    Ok(result)
}

pub fn write<T: Serialize>(base: &Path, keys: &[&str], value: &T) -> Result<()> {
    update(base, |doc| put(doc, keys, value))
}

/// Disk wire format keeps arbitrary vendor JSON losslessly, including nulls.
pub(crate) fn agent_to_wire(config: &crate::AgentRuntimeConfig) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(config)?;
    let object = value
        .as_object_mut()
        .context("agent config must be an object")?;
    object.remove("additional_params");
    if let Some(params) = config.additional_params.as_ref() {
        object.insert(
            "additional_params_json".into(),
            serde_json::Value::String(serde_json::to_string(&params)?),
        );
    }
    omit_null_fields(&mut value);
    Ok(value)
}

pub(crate) fn agent_from_wire(mut value: serde_json::Value) -> Result<crate::AgentRuntimeConfig> {
    let object = value
        .as_object_mut()
        .context("agent config must be an object")?;
    let encoded_params = if let Some(params) = object.remove("additional_params_json") {
        ensure!(
            !object.contains_key("additional_params"),
            "use either additional_params or additional_params_json, not both"
        );
        Some(serde_json::from_str(
            params
                .as_str()
                .context("additional_params_json must be a string")?,
        )?)
    } else {
        None
    };
    let mut config: crate::AgentRuntimeConfig = serde_json::from_value(value)
        .map_err(|_| anyhow::anyhow!("invalid Agent settings fields (contents omitted)"))?;
    if config.id == "default" {
        config.id = crate::DEFAULT_AGENT_ID.into();
    }
    if let Some(params) = encoded_params {
        config.additional_params = Some(params);
    }
    Ok(config)
}

pub(crate) fn omit_null_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            object.retain(|_, value| !value.is_null());
            for value in object.values_mut() {
                omit_null_fields(value);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                omit_null_fields(value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn reading_virtual_tool_defaults_does_not_write_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let _env = crate::test_env::AstroMemoryDirGuard::set(dir.path());
        let tools = crate::sync_tools_enabled_defaults().unwrap();
        assert_eq!(tools.get("exec_command"), Some(&true));
        assert!(!path(dir.path()).exists());
    }

    #[test]
    fn agent_options_and_vendor_json_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut agent = crate::AgentRuntimeConfig {
            id: crate::DEFAULT_AGENT_ID.into(),
            name: "Astro".into(),
            inherit_from: None,
            provider_id: None,
            model: None,
            temperature: Some(0.7),
            max_turns: Some(90),
            additional_params: Some(serde_json::json!({"vendor":null,"values":[null,1]})),
            tools_enabled: None,
            created_at: "test".into(),
        };
        agent.save(dir.path()).unwrap();
        let restored =
            crate::AgentRuntimeConfig::load(dir.path(), crate::DEFAULT_AGENT_ID).unwrap();
        assert_eq!(agent.additional_params, restored.additional_params);
        assert_eq!(restored.model, None);
        agent.additional_params = Some(serde_json::Value::Null);
        agent.save(dir.path()).unwrap();
        assert_eq!(
            crate::AgentRuntimeConfig::load(dir.path(), crate::DEFAULT_AGENT_ID)
                .unwrap()
                .additional_params,
            Some(serde_json::Value::Null)
        );
        assert!(!dir.path().join("agents").exists());
    }

    #[test]
    fn section_updates_preserve_foreign_config_and_comments() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            path(dir.path()),
            "# keep this\n[mcp_servers.demo]\ncommand = 'demo' # keep this too\n",
        )
        .unwrap();
        write(
            dir.path(),
            &["desktop", "tools"],
            &HashMap::from([("terminal", false)]),
        )
        .unwrap();
        let text = fs::read_to_string(path(dir.path())).unwrap();
        assert!(text.contains("# keep this too"));
        assert!(text.contains("[mcp_servers.demo]"));
        let state: HashMap<String, bool> =
            read(dir.path(), &["desktop", "tools"]).unwrap().unwrap();
        assert_eq!(state.get("terminal"), Some(&false));
    }

    #[test]
    fn concurrent_writers_keep_independent_sections() {
        let dir = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            for key in ["tools", "skills"] {
                let base = dir.path();
                scope.spawn(move || {
                    write(
                        base,
                        &["desktop", key],
                        &HashMap::from([("enabled", false)]),
                    )
                    .unwrap()
                });
            }
        });
        let doc = read_document(&path(dir.path())).unwrap();
        for key in ["tools", "skills"] {
            assert!(get::<HashMap<String, bool>>(&doc, &["desktop", key])
                .unwrap()
                .is_some());
        }
    }

    #[test]
    fn invalid_toml_is_not_overwritten_and_reads_do_not_create_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            read::<HashMap<String, bool>>(dir.path(), &["desktop", "tools"])
                .unwrap()
                .is_none()
        );
        assert!(!path(dir.path()).exists());
        fs::write(path(dir.path()), "broken = [").unwrap();
        assert!(write(
            dir.path(),
            &["desktop", "tools"],
            &HashMap::from([("x", true)])
        )
        .is_err());
        assert_eq!(fs::read_to_string(path(dir.path())).unwrap(), "broken = [");
    }

    #[test]
    fn parse_errors_never_print_configuration_values() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(path(dir.path()), "private = 'never-echo-this").unwrap();
        let error = read_document(&path(dir.path())).unwrap_err();
        assert!(!format!("{error:?}").contains("never-echo-this"));
        fs::write(
            path(dir.path()),
            "[desktop.tools]\nterminal = 'never-echo-this'\n",
        )
        .unwrap();
        let error = read::<HashMap<String, bool>>(dir.path(), &["desktop", "tools"]).unwrap_err();
        assert!(!format!("{error:?}").contains("never-echo-this"));
    }
}
