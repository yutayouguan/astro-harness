use super::*;
use agent_config::sources::{self, Domain, SourceDocument};
use std::collections::BTreeSet;

fn documents(scope: &str, project: Option<&Path>) -> anyhow::Result<Vec<SourceDocument>> {
    anyhow::ensure!(
        matches!(scope, "global" | "project"),
        "invalid MCP configuration scope"
    );
    let home = default_memory_dir();
    let (file, boundary) = if scope == "project" {
        let project = project.ok_or_else(|| anyhow::anyhow!("project scope requires a root"))?;
        (mcp_config_path_for_project(project), project)
    } else {
        (mcp_config_path_global(), home.as_path())
    };
    let docs = sources::documents(&file, &sources::read_root(&file)?, Domain::Mcp, boundary)?;
    if scope == "project" {
        let global = documents("global", None)?;
        anyhow::ensure!(
            !docs
                .iter()
                .any(|doc| global.iter().any(|other| doc.path == other.path)),
            "project MCP sources must not alias global configuration files"
        );
    }
    Ok(docs)
}

pub fn source_paths(
    scope: &str,
    project: Option<&Path>,
) -> anyhow::Result<BTreeMap<String, String>> {
    if scope == "builtin" {
        return Ok(BTreeMap::new());
    }
    let mut paths = BTreeMap::new();
    for doc in documents(scope, project)? {
        if let Some(table) = doc.value.get("mcp_servers").and_then(toml::Value::as_table) {
            for id in table.keys() {
                anyhow::ensure!(
                    paths
                        .insert(
                            sanitize_server_id(id),
                            doc.path.to_string_lossy().into_owned()
                        )
                        .is_none(),
                    "duplicate normalized MCP ID: {id}"
                );
            }
        }
    }
    Ok(paths)
}

pub fn load(scope: &str, project: Option<&Path>) -> anyhow::Result<Vec<McpServerConfig>> {
    if scope == "builtin" {
        return Ok(vec![]);
    }
    let mut merged = BTreeMap::new();
    for doc in documents(scope, project)? {
        let root: McpTomlRoot = doc.value.try_into().context("decode MCP source")?;
        for (id, server) in root.mcp_servers {
            let config = server.into_config(id)?;
            anyhow::ensure!(
                merged.insert(config.id.clone(), config).is_none(),
                "duplicate normalized MCP ID"
            );
        }
    }
    let mut configs: Vec<_> = merged.into_values().collect();
    hydrate_discovery(
        &mut configs,
        if scope == "project" { project } else { None },
    );
    Ok(configs)
}

pub fn save(
    scope: &str,
    project: Option<&Path>,
    configs: &[McpServerConfig],
) -> anyhow::Result<()> {
    anyhow::ensure!(scope != "builtin", "builtin MCP servers are read-only");
    let docs = documents(scope, project)?;
    let root = docs
        .iter()
        .find(|doc| doc.root)
        .context("missing MCP root source")?
        .path
        .clone();
    let mut files: BTreeSet<_> = docs.iter().map(|doc| doc.path.clone()).collect();
    files.insert(root.clone());
    let mut guards = Vec::new();
    // Do not create an empty project configuration just to save an empty list.
    if configs.is_empty() && docs.iter().all(|doc| !doc.path.exists()) {
        return Ok(());
    }
    let mut locked_directories = BTreeSet::new();
    for file in &files {
        if locked_directories.insert(file.parent()) {
            guards.push(home::config_file::lock_config_file(file)?);
        }
    }
    let current = documents(scope, project)?;
    anyhow::ensure!(
        current
            .iter()
            .map(|doc| doc.path.clone())
            .collect::<BTreeSet<_>>()
            == files,
        "MCP source list changed; reload before saving"
    );
    let mut origins = BTreeMap::new();
    let mut old = BTreeMap::new();
    for doc in &current {
        let decoded: McpTomlRoot = doc.value.clone().try_into()?;
        let mut canonical = BTreeMap::new();
        for (id, value) in decoded.mcp_servers {
            let config = value.into_config(id)?;
            let id = sanitize_server_id(&config.id);
            anyhow::ensure!(
                origins.insert(id.clone(), doc.path.clone()).is_none(),
                "duplicate MCP ID"
            );
            canonical.insert(id, TomlMcpServer::from_config(&config));
        }
        old.insert(doc.path.clone(), toml::Value::try_from(canonical)?);
    }
    let mut desired = BTreeMap::<_, BTreeMap<String, TomlMcpServer>>::new();
    for file in &files {
        desired.insert(file.clone(), BTreeMap::new());
    }
    let mut ids = BTreeSet::new();
    for config in configs {
        let id = sanitize_server_id(&config.id);
        anyhow::ensure!(ids.insert(id.clone()), "duplicate MCP ID in save request");
        let destination = origins.get(&id).unwrap_or(&root);
        let server = TomlMcpServer::from_config(config);
        server.clone().into_config(id.clone())?;
        desired
            .get_mut(destination)
            .context("unknown MCP destination")?
            .insert(id, server);
    }
    let mut changed = Vec::new();
    for (file, servers) in desired {
        let value = toml::Value::try_from(servers)?;
        if old.get(&file) != Some(&value) {
            changed.push((file, value));
        }
    }
    anyhow::ensure!(
        changed.len() <= 1,
        "MCP edit spans multiple source files; save one source at a time"
    );
    for (file, value) in changed {
        let mut doc = home::settings::read_document(&file)?;
        let before: toml::Value = doc.to_string().parse()?;
        let mut after = before.clone();
        after
            .as_table_mut()
            .context("MCP source must be a table")?
            .insert("mcp_servers".into(), value);
        home::settings::replace_changed_root(&mut doc, &before, &after)?;
        home::settings::write_document(&file, &doc)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_edits_origin_only_and_rejects_multi_file_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        let root = home::config_path(dir.path());
        let external = dir.path().join("servers.toml");
        let original = "# untouched root\n[config_sources]\nmcp=['servers.toml']\n[mcp_servers.local]\ncommand='echo'\n";
        fs::write(&root, original).unwrap();
        fs::write(
            &external,
            "# keep source\n[mcp_servers.external]\ncommand='echo'\n",
        )
        .unwrap();
        let mut configs = load("global", None).unwrap();
        configs
            .iter_mut()
            .find(|c| c.id == "external")
            .unwrap()
            .enabled = false;
        save("global", None, &configs).unwrap();
        assert_eq!(fs::read_to_string(&root).unwrap(), original);
        assert!(fs::read_to_string(&external)
            .unwrap()
            .contains("# keep source"));
        assert!(source_paths("global", None).unwrap()["external"].ends_with("servers.toml"));
        let before = fs::read(&external).unwrap();
        for config in &mut configs {
            config.enabled = !config.enabled;
        }
        assert!(save("global", None, &configs).is_err());
        assert_eq!(fs::read(&external).unwrap(), before);
        assert_eq!(fs::read_to_string(&root).unwrap(), original);
    }
}
