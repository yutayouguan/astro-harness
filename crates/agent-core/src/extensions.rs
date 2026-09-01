//! 统一扩展包发现与 turn 级不可变快照。
//!
//! 扩展包位于 `~/.astro/extensions/<id>/extension.toml`，可信项目还可在
//! `<project>/.astro/extensions/<id>/extension.toml` 提供同 id 覆盖。Manifest 只声明
//! 可安全热加载的贡献：MCP Server、Skill、配置 schema/defaults 与已编译 toolset。
//! 新的可执行代码必须通过 MCP 或编译进 Astro 的 toolset 提供。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use agent_config::loader::{load_local_config, LocalConfigOptions, ProjectTrust};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const EXTENSION_MANIFEST_FILE: &str = "extension.toml";
pub const EXTENSION_MANIFEST_VERSION: u32 = 1;

fn default_true() -> bool {
    true
}

/// 一个扩展包的声明文件。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionManifest {
    #[serde(alias = "manifest_version")]
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub skills: Vec<ExtensionSkillContribution>,
    #[serde(default)]
    pub config: Option<ExtensionConfigContribution>,
    #[serde(default)]
    pub tools: Vec<ExtensionToolContribution>,
}

/// 扩展包内的 Skill；相对路径以 manifest 所在目录为根。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionSkillContribution {
    pub path: PathBuf,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// 扩展配置契约。`schema` 指向 JSON Schema，`defaults` 是默认 TOML 值。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionConfigContribution {
    #[serde(default)]
    pub schema: Option<PathBuf>,
    #[serde(default)]
    pub defaults: Option<toml::Value>,
}

/// 已编译工具集合贡献。Manifest 不加载任意动态库或本地原生代码。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionToolContribution {
    pub toolset: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionScope {
    User,
    Project,
}

/// 已解析且全部贡献校验通过的单个扩展包。
#[derive(Debug, Clone)]
pub struct ResolvedExtension {
    pub manifest: ExtensionManifest,
    pub root: PathBuf,
    pub scope: ExtensionScope,
    pub config_schema: Option<serde_json::Value>,
    pub config: toml::Value,
    pub mcp_servers: Vec<mcp::McpServerConfig>,
    pub skill_configs: Vec<(PathBuf, bool)>,
    pub toolsets: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtensionDiagnostic {
    pub manifest_path: PathBuf,
    pub extension_id: Option<String>,
    pub message: String,
}

/// 单个 turn 使用的完整扩展投影。
///
/// 该值包含基础配置、会话覆盖和扩展贡献合并后的 MCP/Skill/toolset 视图；创建后
/// 不再读盘。同一 turn 的所有 step 必须共享同一个 `Arc<ExtensionSnapshot>`。
#[derive(Debug, Clone)]
pub struct ExtensionSnapshot {
    version: String,
    config_version: String,
    working_dir: PathBuf,
    extensions: Vec<ResolvedExtension>,
    mcp_servers: Vec<mcp::McpServerConfig>,
    skill_configs: Vec<(PathBuf, bool)>,
    skill_index: Vec<(String, String)>,
    toolsets: Vec<String>,
    diagnostics: Vec<ExtensionDiagnostic>,
}

impl ExtensionSnapshot {
    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn config_version(&self) -> &str {
        &self.config_version
    }

    pub fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    pub fn extensions(&self) -> &[ResolvedExtension] {
        &self.extensions
    }

    pub fn extension_roots(&self) -> Vec<PathBuf> {
        self.extensions
            .iter()
            .map(|extension| extension.root.clone())
            .collect()
    }

    pub fn extension(&self, id: &str) -> Option<&ResolvedExtension> {
        self.extensions
            .iter()
            .find(|extension| extension.manifest.id == id)
    }

    pub fn config(&self, id: &str) -> Option<&toml::Value> {
        self.extension(id).map(|extension| &extension.config)
    }

    pub fn config_schema(&self, id: &str) -> Option<&serde_json::Value> {
        self.extension(id)
            .and_then(|extension| extension.config_schema.as_ref())
    }

    pub fn mcp_servers(&self) -> &[mcp::McpServerConfig] {
        &self.mcp_servers
    }

    pub fn skill_configs(&self) -> &[(PathBuf, bool)] {
        &self.skill_configs
    }

    pub fn skill_index(&self) -> &[(String, String)] {
        &self.skill_index
    }

    pub fn toolsets(&self) -> &[String] {
        &self.toolsets
    }

    pub fn diagnostics(&self) -> &[ExtensionDiagnostic] {
        &self.diagnostics
    }
}

/// 发现一次扩展快照所需的 session 输入。
#[derive(Debug, Clone)]
pub struct ExtensionDiscoveryOptions {
    pub astro_home: PathBuf,
    pub working_dir: PathBuf,
    pub mcp_overrides: Vec<mcp::McpServerConfig>,
    pub skill_overrides: Vec<(PathBuf, bool)>,
}

impl ExtensionDiscoveryOptions {
    pub fn new(astro_home: impl Into<PathBuf>, working_dir: impl Into<PathBuf>) -> Self {
        Self {
            astro_home: astro_home.into(),
            working_dir: working_dir.into(),
            mcp_overrides: Vec::new(),
            skill_overrides: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct ManifestCandidate {
    manifest: ExtensionManifest,
    root: PathBuf,
    path: PathBuf,
    scope: ExtensionScope,
}

/// 从磁盘发现并冻结一个不可变扩展快照。
pub fn discover_extension_snapshot(
    options: &ExtensionDiscoveryOptions,
) -> Result<ExtensionSnapshot> {
    let loaded = load_local_config(&LocalConfigOptions::new(
        &options.astro_home,
        &options.working_dir,
    ))?;
    let effective = loaded.resolve();
    let mut diagnostics = Vec::new();
    let mut selected = BTreeMap::<String, ManifestCandidate>::new();

    collect_candidates(
        &options.astro_home.join("extensions"),
        ExtensionScope::User,
        &mut selected,
        &mut diagnostics,
    );
    if loaded.project_trust == ProjectTrust::Trusted {
        collect_candidates(
            &loaded.project_root.join(".astro/extensions"),
            ExtensionScope::Project,
            &mut selected,
            &mut diagnostics,
        );
    }

    let mut resolved_candidates = Vec::new();
    for candidate in selected.into_values() {
        if !candidate.manifest.enabled {
            continue;
        }
        match resolve_candidate(candidate, effective.raw()) {
            Ok(extension) => resolved_candidates.push(extension),
            Err(error) => diagnostics.push(ExtensionDiagnostic {
                manifest_path: error.0,
                extension_id: error.1,
                message: error.2,
            }),
        }
    }

    let mut mcp_by_id = BTreeMap::new();
    for config in mcp::decode_mcp_servers_from_value(effective.raw())? {
        mcp_by_id.insert(mcp::sanitize_server_id(&config.id), config);
    }
    let mut skill_configs = Vec::new();
    let mut toolsets = Vec::new();
    let mut resolved = Vec::new();
    for extension in resolved_candidates {
        let collisions = extension
            .mcp_servers
            .iter()
            .filter(|config| mcp_by_id.contains_key(&mcp::sanitize_server_id(&config.id)))
            .map(|config| config.id.clone())
            .collect::<Vec<_>>();
        if !collisions.is_empty() {
            diagnostics.push(ExtensionDiagnostic {
                manifest_path: extension.root.join(EXTENSION_MANIFEST_FILE),
                extension_id: Some(extension.manifest.id.clone()),
                message: format!(
                    "extension MCP server id collision: {}",
                    collisions.join(", ")
                ),
            });
            continue;
        }
        for config in &extension.mcp_servers {
            mcp_by_id.insert(mcp::sanitize_server_id(&config.id), config.clone());
        }
        skill_configs.extend(extension.skill_configs.iter().cloned());
        toolsets.extend(extension.toolsets.iter().cloned());
        resolved.push(extension);
    }
    for config in &options.mcp_overrides {
        mcp_by_id.insert(mcp::sanitize_server_id(&config.id), config.clone());
    }
    // Session/custom-agent overlays are the highest skill layer; the skills loader uses
    // the last matching path as the winner.
    skill_configs.extend(options.skill_overrides.iter().cloned());
    toolsets.sort();
    toolsets.dedup();

    skills::set_workspace_override(&options.working_dir);
    let skill_index = skills::list_enabled_for_prompt_with_config(&skill_configs);
    let mcp_servers = mcp_by_id.into_values().collect::<Vec<_>>();
    let version = snapshot_fingerprint(
        effective.version(),
        &options.working_dir,
        &resolved,
        &mcp_servers,
        &skill_configs,
        &skill_index,
        &toolsets,
    )?;

    Ok(ExtensionSnapshot {
        version,
        config_version: effective.version().to_string(),
        working_dir: options.working_dir.clone(),
        extensions: resolved,
        mcp_servers,
        skill_configs,
        skill_index,
        toolsets,
        diagnostics,
    })
}

fn collect_candidates(
    root: &Path,
    scope: ExtensionScope,
    selected: &mut BTreeMap<String, ManifestCandidate>,
    diagnostics: &mut Vec<ExtensionDiagnostic>,
) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path().join(EXTENSION_MANIFEST_FILE))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        match parse_candidate(&path, scope) {
            Ok(candidate) => {
                selected.insert(candidate.manifest.id.clone(), candidate);
            }
            Err(error) => diagnostics.push(ExtensionDiagnostic {
                manifest_path: path,
                extension_id: None,
                message: error.to_string(),
            }),
        }
    }
}

fn parse_candidate(path: &Path, scope: ExtensionScope) -> Result<ManifestCandidate> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read extension manifest {}", path.display()))?;
    let mut manifest: ExtensionManifest = toml::from_str(&raw)
        .with_context(|| format!("parse extension manifest {}", path.display()))?;
    manifest.id = manifest.id.trim().to_string();
    if manifest.id.is_empty()
        || !manifest
            .id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
    {
        anyhow::bail!("extension id must use lowercase letters, digits, '-' or '_'");
    }
    if manifest.schema_version != EXTENSION_MANIFEST_VERSION {
        anyhow::bail!(
            "unsupported extension schema_version {}; expected {}",
            manifest.schema_version,
            EXTENSION_MANIFEST_VERSION
        );
    }
    if manifest.name.trim().is_empty() {
        manifest.name = manifest.id.clone();
    }
    let root = path
        .parent()
        .context("extension manifest has no parent directory")?
        .canonicalize()
        .with_context(|| format!("canonicalize extension root for {}", path.display()))?;
    Ok(ManifestCandidate {
        manifest,
        root,
        path: path.to_path_buf(),
        scope,
    })
}

fn resolve_candidate(
    candidate: ManifestCandidate,
    effective_config: &toml::Value,
) -> std::result::Result<ResolvedExtension, (PathBuf, Option<String>, String)> {
    let result = resolve_candidate_inner(&candidate, effective_config);
    result.map_err(|error| {
        (
            candidate.path.clone(),
            Some(candidate.manifest.id.clone()),
            error.to_string(),
        )
    })
}

fn resolve_candidate_inner(
    candidate: &ManifestCandidate,
    effective_config: &toml::Value,
) -> Result<ResolvedExtension> {
    let extension_id = &candidate.manifest.id;
    let config_schema = candidate
        .manifest
        .config
        .as_ref()
        .and_then(|config| config.schema.as_deref())
        .map(|path| {
            let path = resolve_inside_root(&candidate.root, path, true)?;
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("read extension config schema {}", path.display()))?;
            let schema: serde_json::Value = serde_json::from_str(&raw)
                .with_context(|| format!("parse extension config schema {}", path.display()))?;
            anyhow::ensure!(
                schema.is_object(),
                "extension config schema must be a JSON object"
            );
            Ok(schema)
        })
        .transpose()?;

    let mut config = candidate
        .manifest
        .config
        .as_ref()
        .and_then(|contribution| contribution.defaults.clone())
        .unwrap_or_else(empty_table);
    if let Some(overrides) = extension_config_value(effective_config, extension_id) {
        merge_toml(&mut config, overrides);
    }

    let mut skill_configs = Vec::new();
    for skill in &candidate.manifest.skills {
        let path = resolve_inside_root(&candidate.root, &skill.path, false)?;
        let skill_md = if path.is_dir() {
            path.join("SKILL.md")
        } else {
            path.clone()
        };
        anyhow::ensure!(
            skill_md.is_file(),
            "extension skill is missing SKILL.md: {}",
            skill_md.display()
        );
        skill_configs.push((path, skill.enabled));
    }

    let mut mcp_servers = mcp::decode_inline_mcp_servers(&candidate.manifest.mcp_servers)?;
    for server in &mut mcp_servers {
        let local_id = server.id.clone();
        server.id = mcp::sanitize_server_id(&format!("ext-{extension_id}-{local_id}"));
        if server.name.trim().is_empty() {
            server.name = format!("{} / {local_id}", candidate.manifest.name);
        }
        if server.r#type == mcp::McpTransportType::Stdio {
            if let Some(cwd) = server.cwd.as_deref() {
                let resolved = resolve_inside_root(&candidate.root, Path::new(cwd), false)?;
                server.cwd = Some(resolved.to_string_lossy().into_owned());
            }
        }
    }
    let mut toolsets = candidate
        .manifest
        .tools
        .iter()
        .filter(|tool| tool.enabled)
        .map(|tool| tool.toolset.trim().to_string())
        .filter(|toolset| !toolset.is_empty())
        .collect::<Vec<_>>();
    toolsets.sort();
    toolsets.dedup();

    Ok(ResolvedExtension {
        manifest: candidate.manifest.clone(),
        root: candidate.root.clone(),
        scope: candidate.scope,
        config_schema,
        config,
        mcp_servers,
        skill_configs,
        toolsets,
    })
}

fn resolve_inside_root(root: &Path, path: &Path, require_file: bool) -> Result<PathBuf> {
    anyhow::ensure!(!path.is_absolute(), "extension paths must be relative");
    let resolved = root
        .join(path)
        .canonicalize()
        .with_context(|| format!("resolve extension path {}", path.display()))?;
    anyhow::ensure!(
        resolved.starts_with(root),
        "extension path escapes package root: {}",
        path.display()
    );
    if require_file {
        anyhow::ensure!(resolved.is_file(), "extension path is not a file");
    }
    Ok(resolved)
}

fn extension_config_value<'a>(config: &'a toml::Value, id: &str) -> Option<&'a toml::Value> {
    config.get("extensions")?.as_table()?.get(id)
}

fn empty_table() -> toml::Value {
    toml::Value::Table(Default::default())
}

fn merge_toml(base: &mut toml::Value, overlay: &toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base), toml::Value::Table(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(key) {
                    Some(existing) => merge_toml(existing, value),
                    None => {
                        base.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

fn snapshot_fingerprint(
    config_version: &str,
    working_dir: &Path,
    extensions: &[ResolvedExtension],
    mcp_servers: &[mcp::McpServerConfig],
    skill_configs: &[(PathBuf, bool)],
    skill_index: &[(String, String)],
    toolsets: &[String],
) -> Result<String> {
    let extension_values = extensions
        .iter()
        .map(|extension| {
            serde_json::json!({
                "id": &extension.manifest.id,
                "name": &extension.manifest.name,
                "version": &extension.manifest.version,
                "description": &extension.manifest.description,
                "root": &extension.root,
                "scope": extension.scope,
                "config_schema": &extension.config_schema,
                "config": &extension.config,
                "mcp_servers": &extension.mcp_servers,
                "skill_configs": &extension.skill_configs,
                "toolsets": &extension.toolsets,
            })
        })
        .collect::<Vec<_>>();
    let mut value = serde_json::json!({
        "config_version": config_version,
        "working_dir": working_dir,
        "extensions": extension_values,
        "mcp_servers": mcp_servers,
        "skill_configs": skill_configs,
        "skill_index": skill_index,
        "toolsets": toolsets,
    });
    canonicalize_json(&mut value);
    let bytes = serde_json::to_vec(&value)?;
    let digest = Sha256::digest(bytes);
    Ok(format!("sha256:{digest:x}"))
}

fn canonicalize_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            let old = std::mem::take(map);
            let mut sorted = BTreeMap::new();
            for (key, mut value) in old {
                canonicalize_json(&mut value);
                sorted.insert(key, value);
            }
            map.extend(sorted);
        }
        serde_json::Value::Array(values) => {
            for value in values {
                canonicalize_json(value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quoted(path: &Path) -> String {
        format!("{:?}", path.to_string_lossy())
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home/.astro");
        let project = root.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(project.join(".git")).unwrap();
        fs::write(
            home.join("config.toml"),
            format!(
                "[projects.{}]\ntrust_level = 'trusted'\n[extensions.demo]\nlabel = 'configured'\n",
                quoted(&project)
            ),
        )
        .unwrap();
        (root, home, project)
    }

    fn write_demo_extension(root: &Path, description: &str) {
        let package = root.join("demo");
        fs::create_dir_all(package.join("skills/demo-skill")).unwrap();
        fs::write(
            package.join("skills/demo-skill/SKILL.md"),
            "---\nname: demo-skill\ndescription: demo\nastro_tools: [image_gen]\n---\nbody\n",
        )
        .unwrap();
        fs::write(
            package.join("schema.json"),
            r#"{"type":"object","properties":{"label":{"type":"string"}}}"#,
        )
        .unwrap();
        fs::write(
            package.join(EXTENSION_MANIFEST_FILE),
            format!(
                r#"schema_version = 1
id = "demo"
name = "Demo"
description = {description:?}

[config]
schema = "schema.json"
[config.defaults]
label = "default"

[[skills]]
path = "skills/demo-skill"

[[tools]]
toolset = "image_gen"

[mcp_servers.echo]
command = "echo"
args = ["ready"]
"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn discovers_and_freezes_all_contribution_kinds() {
        let (_temp, home, project) = fixture();
        write_demo_extension(&home.join("extensions"), "global");

        let snapshot =
            discover_extension_snapshot(&ExtensionDiscoveryOptions::new(&home, &project)).unwrap();

        assert_eq!(snapshot.extensions().len(), 1);
        assert_eq!(
            snapshot.extensions()[0].config["label"].as_str(),
            Some("configured")
        );
        assert!(snapshot.extensions()[0].config_schema.is_some());
        assert!(snapshot
            .skill_index()
            .iter()
            .any(|(name, _)| name == "demo-skill"));
        assert_eq!(snapshot.toolsets(), &["image_gen"]);
        assert_eq!(snapshot.mcp_servers().len(), 1);
        assert_eq!(snapshot.mcp_servers()[0].id, "ext-demo-echo");
        assert!(snapshot.version().starts_with("sha256:"));
    }

    #[test]
    fn trusted_project_manifest_shadows_user_manifest_atomically() {
        let (_temp, home, project) = fixture();
        write_demo_extension(&home.join("extensions"), "global");
        write_demo_extension(&project.join(".astro/extensions"), "project");

        let snapshot =
            discover_extension_snapshot(&ExtensionDiscoveryOptions::new(&home, &project)).unwrap();

        assert_eq!(snapshot.extensions().len(), 1);
        assert_eq!(snapshot.extensions()[0].scope, ExtensionScope::Project);
        assert_eq!(snapshot.extensions()[0].manifest.description, "project");
    }

    #[test]
    fn session_skill_override_wins_over_manifest_contribution() {
        let (_temp, home, project) = fixture();
        write_demo_extension(&home.join("extensions"), "global");
        let skill_path = home.join("extensions/demo/skills/demo-skill");
        let mut options = ExtensionDiscoveryOptions::new(&home, &project);
        options.skill_overrides.push((skill_path, false));

        let snapshot = discover_extension_snapshot(&options).unwrap();

        assert!(snapshot
            .skill_index()
            .iter()
            .all(|(name, _)| name != "demo-skill"));
    }

    #[test]
    fn untrusted_project_extensions_are_not_read() {
        let (_temp, home, project) = fixture();
        fs::write(home.join("config.toml"), "model = 'test'\n").unwrap();
        write_demo_extension(&project.join(".astro/extensions"), "project");

        let snapshot =
            discover_extension_snapshot(&ExtensionDiscoveryOptions::new(&home, &project)).unwrap();

        assert!(snapshot.extensions().is_empty());
        assert!(snapshot.diagnostics().is_empty());
    }

    #[test]
    fn invalid_package_is_skipped_as_one_diagnostic() {
        let (_temp, home, project) = fixture();
        let package = home.join("extensions/broken");
        fs::create_dir_all(&package).unwrap();
        fs::create_dir_all(home.join("extensions/escape")).unwrap();
        fs::write(
            home.join("extensions/escape/SKILL.md"),
            "---\nname: escape\ndescription: escape\n---\n",
        )
        .unwrap();
        fs::write(
            package.join(EXTENSION_MANIFEST_FILE),
            "schema_version = 1\nid = 'broken'\n[[skills]]\npath = '../escape'\n",
        )
        .unwrap();

        let snapshot =
            discover_extension_snapshot(&ExtensionDiscoveryOptions::new(&home, &project)).unwrap();

        assert!(snapshot.extensions().is_empty());
        assert_eq!(snapshot.diagnostics().len(), 1);
        assert_eq!(
            snapshot.diagnostics()[0].extension_id.as_deref(),
            Some("broken")
        );
    }

    #[test]
    fn mcp_id_collision_skips_the_whole_extension() {
        let (_temp, home, project) = fixture();
        write_demo_extension(&home.join("extensions"), "global");
        let config_path = home.join("config.toml");
        let mut config = fs::read_to_string(&config_path).unwrap();
        config.push_str("\n[mcp_servers.ext-demo-echo]\nurl = 'https://example.invalid/mcp'\n");
        fs::write(config_path, config).unwrap();

        let snapshot =
            discover_extension_snapshot(&ExtensionDiscoveryOptions::new(&home, &project)).unwrap();

        assert!(snapshot.extensions().is_empty());
        assert!(snapshot
            .skill_index()
            .iter()
            .all(|(name, _)| name != "demo-skill"));
        assert!(snapshot.toolsets().is_empty());
        assert_eq!(snapshot.mcp_servers().len(), 1);
        assert!(snapshot.diagnostics()[0].message.contains("collision"));
    }

    #[test]
    fn snapshot_is_unchanged_after_manifest_changes() {
        let (_temp, home, project) = fixture();
        let root = home.join("extensions");
        write_demo_extension(&root, "before");
        let first =
            discover_extension_snapshot(&ExtensionDiscoveryOptions::new(&home, &project)).unwrap();
        write_demo_extension(&root, "after");

        assert_eq!(first.extensions()[0].manifest.description, "before");
        let second =
            discover_extension_snapshot(&ExtensionDiscoveryOptions::new(&home, &project)).unwrap();
        assert_eq!(second.extensions()[0].manifest.description, "after");
        assert_ne!(first.version(), second.version());
    }

    #[test]
    fn turn_context_publishes_only_the_first_snapshot() {
        let (_temp, home, project) = fixture();
        let root = home.join("extensions");
        write_demo_extension(&root, "first");
        let first = std::sync::Arc::new(
            discover_extension_snapshot(&ExtensionDiscoveryOptions::new(&home, &project)).unwrap(),
        );
        let first_version = first.version().to_string();
        write_demo_extension(&root, "second");
        let second = std::sync::Arc::new(
            discover_extension_snapshot(&ExtensionDiscoveryOptions::new(&home, &project)).unwrap(),
        );
        assert_ne!(first.version(), second.version());

        let turn = crate::runtime::TurnContext::new(
            "turn-1".into(),
            1,
            types::InteractionMode::Agent,
            None,
            Some(project),
        );
        let published = turn.publish_extension_snapshot(first);
        let repeated = turn.publish_extension_snapshot(second);

        assert_eq!(published.version(), first_version);
        assert_eq!(repeated.version(), first_version);
        assert!(std::sync::Arc::ptr_eq(&published, &repeated));
    }
}
