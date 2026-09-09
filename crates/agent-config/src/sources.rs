//! Explicit, bounded domain-file references. No implicit scanning or writes.
use crate::{ConfigLayerEntry, ConfigLayerSource};
use anyhow::{ensure, Context, Result};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use toml::Value;

const MAX_FILES: usize = 64;
const MAX_DEPTH: usize = 8;
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Mcp,
    Hooks,
}
impl Domain {
    pub fn name(self) -> &'static str {
        match self {
            Self::Mcp => "mcp",
            Self::Hooks => "hooks",
        }
    }
    pub fn section(self) -> &'static str {
        match self {
            Self::Mcp => "mcp_servers",
            Self::Hooks => "hooks",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceDocument {
    pub path: PathBuf,
    pub value: Value,
    pub root: bool,
}

pub fn source_file(source: &ConfigLayerSource) -> Option<&Path> {
    match source {
        ConfigLayerSource::Included { file, .. }
        | ConfigLayerSource::User { file }
        | ConfigLayerSource::System { file }
        | ConfigLayerSource::Profile { file, .. } => Some(file),
        _ => None,
    }
}

pub fn read_root(path: &Path) -> Result<Value> {
    if !path.try_exists()? {
        return Ok(Value::Table(Default::default()));
    }
    read_file(path)
}

fn read_file(path: &Path) -> Result<Value> {
    ensure!(
        fs::metadata(path)?.is_file(),
        "configuration source is not a file: {}",
        path.display()
    );
    ensure!(
        fs::metadata(path)?.len() <= MAX_BYTES,
        "configuration source exceeds 1 MiB: {}",
        path.display()
    );
    let bytes = fs::read(path)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "configuration source exceeds 1 MiB"
    );
    if path.extension().and_then(|e| e.to_str()) == Some("json") {
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
            anyhow::anyhow!("invalid JSON configuration source: {}", path.display())
        })?;
        Value::try_from(value)
            .map_err(|_| anyhow::anyhow!("source is not representable as TOML: {}", path.display()))
    } else {
        let text = std::str::from_utf8(&bytes).context("configuration source is not UTF-8")?;
        text.parse()
            .map_err(|_| anyhow::anyhow!("invalid TOML configuration source: {}", path.display()))
    }
}

fn references(value: &Value, domain: Domain) -> Result<Vec<String>> {
    let Some(sources) = value.get("config_sources") else {
        return Ok(vec![]);
    };
    let table = sources
        .as_table()
        .context("config_sources must be a table")?;
    ensure!(
        table
            .keys()
            .all(|key| matches!(key.as_str(), "mcp" | "hooks")),
        "unknown config_sources domain"
    );
    let Some(files) = table.get(domain.name()) else {
        return Ok(vec![]);
    };
    files
        .as_array()
        .context("config_sources entries must be arrays")?
        .iter()
        .map(|file| {
            let file = file
                .as_str()
                .context("configuration reference must be a path string")?;
            ensure!(
                !file.trim().is_empty() && !file.contains("://"),
                "configuration reference must be a local file"
            );
            Ok(file.to_string())
        })
        .collect()
}

/// All referenced files stay inside their source's authority boundary. The root
/// document may contain unrelated settings; referenced files may not.
pub fn documents(
    root: &Path,
    root_value: &Value,
    domain: Domain,
    boundary: &Path,
) -> Result<Vec<SourceDocument>> {
    let boundary = canonical_missing_path(boundary)
        .with_context(|| format!("invalid source boundary {}", boundary.display()))?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    visit(
        root,
        root_value.clone(),
        domain,
        &boundary,
        0,
        true,
        &mut seen,
        &mut out,
    )?;
    validate_duplicates(&out, domain)?;
    Ok(out)
}

fn visit(
    path: &Path,
    value: Value,
    domain: Domain,
    boundary: &Path,
    depth: usize,
    root: bool,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<SourceDocument>,
) -> Result<()> {
    ensure!(
        depth <= MAX_DEPTH && seen.len() < MAX_FILES,
        "configuration reference limit exceeded"
    );
    let canonical = if path.try_exists()? {
        path.canonicalize()?
    } else {
        ensure!(
            root,
            "referenced configuration file is missing: {}",
            path.display()
        );
        canonical_missing_path(path)?
    };
    ensure!(
        canonical.starts_with(boundary),
        "configuration source escapes its authority boundary: {}",
        path.display()
    );
    ensure!(
        seen.insert(canonical.clone()),
        "duplicate or cyclic configuration reference: {}",
        path.display()
    );
    let table = value
        .as_table()
        .context("configuration source must be a table")?;
    if !root {
        ensure!(
            domain != Domain::Mcp || canonical.extension().and_then(|e| e.to_str()) == Some("toml"),
            "MCP references must be TOML files"
        );
        ensure!(
            table.keys().all(|key| key == domain.section()
                || key == "config_sources"
                || (domain == Domain::Hooks && key == "description")),
            "referenced {} file contains settings from another domain",
            domain.name()
        );
        if let Some(sources) = table.get("config_sources").and_then(Value::as_table) {
            ensure!(
                sources.keys().all(|key| key == domain.name()),
                "cross-domain configuration references are not allowed"
            );
        }
    }
    for reference in references(&value, domain)? {
        let file = PathBuf::from(reference);
        let file = if file.is_absolute() {
            file
        } else {
            canonical
                .parent()
                .context("source has no parent")?
                .join(file)
        };
        let resolved = file
            .canonicalize()
            .with_context(|| format!("referenced file is unavailable: {}", file.display()))?;
        ensure!(
            resolved.starts_with(boundary),
            "configuration reference escapes its authority boundary"
        );
        let child = read_file(&resolved)?;
        visit(
            &resolved,
            child,
            domain,
            boundary,
            depth + 1,
            false,
            seen,
            out,
        )?;
    }
    out.push(SourceDocument {
        path: canonical,
        value,
        root,
    });
    Ok(())
}

fn canonical_missing_path(path: &Path) -> Result<PathBuf> {
    if path.try_exists()? {
        return Ok(path.canonicalize()?);
    }
    let parent = path.parent().context("source has no existing ancestor")?;
    let name = path.file_name().context("invalid source path")?;
    Ok(canonical_missing_path(parent)?.join(name))
}

fn validate_duplicates(docs: &[SourceDocument], domain: Domain) -> Result<()> {
    let mut ids = HashSet::new();
    for doc in docs {
        let Some(section) = doc.value.get(domain.section()) else {
            continue;
        };
        let table = section
            .as_table()
            .context("domain definitions must be a table")?;
        if domain == Domain::Mcp {
            for id in table.keys() {
                ensure!(
                    ids.insert(id.clone()),
                    "duplicate MCP ID in one configuration layer: {id}"
                );
            }
        } else {
            for (event, groups) in table {
                if event == "state" {
                    continue;
                }
                for group in groups
                    .as_array()
                    .context("hook event must contain an array of rules")?
                {
                    if let Some(id) = group.get("id") {
                        let id = id.as_str().context("hook rule id must be a string")?;
                        ensure!(
                            !id.is_empty()
                                && id.len() <= 128
                                && id
                                    .chars()
                                    .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c)),
                            "invalid hook rule ID"
                        );
                        ensure!(
                            ids.insert(id.to_string()),
                            "duplicate Hook ID in one configuration layer: {id}"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn expand_layer(
    layer: ConfigLayerEntry,
    file: &Path,
    boundary: &Path,
) -> Result<Vec<ConfigLayerEntry>> {
    let mut expanded = Vec::new();
    for domain in [Domain::Mcp, Domain::Hooks] {
        for doc in documents(file, &layer.config, domain, boundary)? {
            if doc.root {
                continue;
            }
            let Some(value) = doc.value.get(domain.section()) else {
                continue;
            };
            let config = Value::Table(
                [(domain.section().into(), value.clone())]
                    .into_iter()
                    .collect(),
            );
            expanded.push(ConfigLayerEntry::from_value(
                ConfigLayerSource::Included {
                    file: doc.path,
                    owner: Box::new(layer.source.clone()),
                },
                config,
            ));
        }
    }
    expanded.push(layer);
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_escape_and_duplicate_hook_sources_fail() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path().join("config.toml");
        let value: Value = "[config_sources]\nhooks=['missing.toml']\n"
            .parse()
            .unwrap();
        assert!(documents(&root, &value, Domain::Hooks, dir.path()).is_err());
        let foreign = outside.path().join("hooks.toml");
        fs::write(&foreign, "[hooks]").unwrap();
        let value: Value = format!("[config_sources]\nhooks=[{:?}]", foreign.to_string_lossy())
            .parse()
            .unwrap();
        assert!(documents(&root, &value, Domain::Hooks, dir.path()).is_err());
        let value: Value =
            "[[hooks.PreToolUse]]\nid='same'\nhooks=[]\n[[hooks.PostToolUse]]\nid='same'\nhooks=[]"
                .parse()
                .unwrap();
        assert!(documents(&root, &value, Domain::Hooks, dir.path()).is_err());
    }

    #[test]
    fn absent_optional_root_does_not_create_directories() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("absent");
        let root = home.join("config.toml");
        let docs = documents(&root, &read_root(&root).unwrap(), Domain::Mcp, &home).unwrap();
        assert_eq!(docs.len(), 1);
        assert!(!home.exists());
    }
    #[test]
    fn references_are_explicit_bounded_and_domain_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("config.toml");
        fs::write(&root, "[config_sources]\nmcp=['servers.toml']\n").unwrap();
        fs::write(
            dir.path().join("servers.toml"),
            "[mcp_servers.example]\ncommand='example'\n",
        )
        .unwrap();
        let docs = documents(&root, &read_root(&root).unwrap(), Domain::Mcp, dir.path()).unwrap();
        assert_eq!(docs.len(), 2);
        assert!(!docs[0].root);
        fs::write(
            dir.path().join("servers.toml"),
            "[config_sources]\nmcp=['config.toml']\n",
        )
        .unwrap();
        assert!(documents(&root, &read_root(&root).unwrap(), Domain::Mcp, dir.path()).is_err());
        fs::write(
            dir.path().join("servers.toml"),
            "model_provider='not-allowed'\n",
        )
        .unwrap();
        assert!(documents(&root, &read_root(&root).unwrap(), Domain::Mcp, dir.path()).is_err());
    }
    #[test]
    fn duplicate_inline_and_referenced_mcp_ids_fail() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("config.toml");
        fs::write(
            &root,
            "[config_sources]\nmcp=['servers.toml']\n[mcp_servers.x]\ncommand='first'\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("servers.toml"),
            "[mcp_servers.x]\ncommand='second'\n",
        )
        .unwrap();
        assert!(documents(&root, &read_root(&root).unwrap(), Domain::Mcp, dir.path()).is_err());
    }
}
