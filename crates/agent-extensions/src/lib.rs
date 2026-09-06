//! Astro 统一扩展包发现、校验与 turn 级不可变快照。
//!
//! 扩展包位于 `~/.astro/extensions/<id>/extension.toml`，可信项目还可在
//! `<project>/.astro/extensions/<id>/extension.toml` 提供同 id 覆盖。Manifest 只声明
//! 可安全热加载的贡献：MCP Server、Skill、配置 schema/defaults 与已编译 toolset。
//! 新的可执行代码必须通过 MCP 或编译进 Astro 的 toolset 提供。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const EXTENSION_MANIFEST_FILE: &str = "extension.toml";
pub const EXTENSION_MANIFEST_VERSION: u32 = 1;

fn default_true() -> bool {
    true
}

/// 一个扩展包的声明文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionSkillContribution {
    pub path: PathBuf,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// 扩展配置契约。`schema` 指向 JSON Schema，`defaults` 是默认 TOML 值。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionConfigContribution {
    #[serde(default)]
    pub schema: Option<PathBuf>,
    #[serde(default)]
    pub defaults: Option<toml::Value>,
}

/// 已编译工具集合贡献。Manifest 不加载任意动态库或本地原生代码。
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionChangeKind {
    Added,
    Updated,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtensionChange {
    pub extension_id: String,
    pub kind: ExtensionChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtensionReconcileReport {
    pub previous_version: String,
    pub next_version: String,
    pub changed_extensions: Vec<ExtensionChange>,
    pub refresh_mcp: bool,
    pub refresh_skills: bool,
    pub refresh_hooks: bool,
    pub refresh_toolsets: bool,
}

/// 单个 turn 使用的完整扩展投影。
///
/// 该值包含基础配置、会话覆盖和扩展贡献合并后的 MCP/Skill/toolset 视图；创建后
/// 不再读盘。同一 turn 的所有 step 必须共享同一个 `Arc<ExtensionSnapshot>`。
#[derive(Debug, Clone)]
pub struct ExtensionSnapshot {
    version: String,
    config_version: String,
    effective_config: toml::Value,
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

    /// 查询 `[features]` 下的布尔开关；缺失或类型错误都视为关闭。
    pub fn feature_enabled(&self, name: &str) -> bool {
        self.effective_config
            .get("features")
            .and_then(toml::Value::as_table)
            .and_then(|features| features.get(name))
            .and_then(toml::Value::as_bool)
            .unwrap_or(false)
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

/// Compares two fully validated immutable snapshots without mutating the active turn.
pub fn reconcile_extension_snapshots(
    previous: &ExtensionSnapshot,
    next: &ExtensionSnapshot,
) -> Result<ExtensionReconcileReport> {
    let previous_by_id = previous
        .extensions
        .iter()
        .map(|extension| (extension.manifest.id.as_str(), extension))
        .collect::<BTreeMap<_, _>>();
    let next_by_id = next
        .extensions
        .iter()
        .map(|extension| (extension.manifest.id.as_str(), extension))
        .collect::<BTreeMap<_, _>>();
    let mut ids = previous_by_id
        .keys()
        .chain(next_by_id.keys())
        .copied()
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();

    let mut changed_extensions = Vec::new();
    let mut refresh_mcp = false;
    let mut refresh_skills = false;
    let mut refresh_toolsets = false;
    for id in ids {
        let previous_extension = previous_by_id.get(id).copied();
        let next_extension = next_by_id.get(id).copied();
        let kind = match (previous_extension, next_extension) {
            (None, Some(_)) => Some(ExtensionChangeKind::Added),
            (Some(_), None) => Some(ExtensionChangeKind::Removed),
            (Some(before), Some(after))
                if resolved_extension_fingerprint(before)?
                    != resolved_extension_fingerprint(after)? =>
            {
                Some(ExtensionChangeKind::Updated)
            }
            _ => None,
        };
        let Some(kind) = kind else { continue };
        refresh_mcp |= contribution_changed(
            previous_extension.map(|extension| &extension.mcp_servers),
            next_extension.map(|extension| &extension.mcp_servers),
        )?;
        refresh_skills |= contribution_changed(
            previous_extension.map(|extension| &extension.skill_configs),
            next_extension.map(|extension| &extension.skill_configs),
        )?;
        refresh_toolsets |= contribution_changed(
            previous_extension.map(|extension| &extension.toolsets),
            next_extension.map(|extension| &extension.toolsets),
        )?;
        changed_extensions.push(ExtensionChange {
            extension_id: id.to_string(),
            kind,
        });
    }

    Ok(ExtensionReconcileReport {
        previous_version: previous.version.clone(),
        next_version: next.version.clone(),
        refresh_hooks: !changed_extensions.is_empty(),
        changed_extensions,
        refresh_mcp,
        refresh_skills,
        refresh_toolsets,
    })
}

fn contribution_changed<T: Serialize>(before: Option<&T>, after: Option<&T>) -> Result<bool> {
    Ok(serde_json::to_value(before)? != serde_json::to_value(after)?)
}

fn resolved_extension_fingerprint(extension: &ResolvedExtension) -> Result<String> {
    let mut value = resolved_extension_value(extension);
    canonicalize_json(&mut value);
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&value)?)
    ))
}

/// 发现一次扩展快照所需的 session 输入。
#[derive(Debug, Clone)]
pub struct ExtensionDiscoveryOptions {
    pub astro_home: PathBuf,
    pub working_dir: PathBuf,
    /// 仅当上层已经确认项目可信时设置；`None` 表示禁止读取项目扩展。
    pub trusted_project_root: Option<PathBuf>,
    /// 当前 turn 已解析的完整分层配置。
    pub effective_config: toml::Value,
    /// 与 `effective_config` 对应的稳定版本。
    pub config_version: String,
    pub mcp_overrides: Vec<mcp::McpServerConfig>,
    pub skill_overrides: Vec<(PathBuf, bool)>,
}

impl ExtensionDiscoveryOptions {
    pub fn new(
        astro_home: impl Into<PathBuf>,
        working_dir: impl Into<PathBuf>,
        effective_config: toml::Value,
        config_version: impl Into<String>,
    ) -> Self {
        Self {
            astro_home: astro_home.into(),
            working_dir: working_dir.into(),
            trusted_project_root: None,
            effective_config,
            config_version: config_version.into(),
            mcp_overrides: Vec::new(),
            skill_overrides: Vec::new(),
        }
    }

    /// 启用来自已受信任项目根的扩展包。
    pub fn with_trusted_project_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.trusted_project_root = Some(root.into());
        self
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
    let mut diagnostics = Vec::new();
    let mut selected = BTreeMap::<String, ManifestCandidate>::new();

    collect_candidates(
        &options.astro_home.join("extensions"),
        ExtensionScope::User,
        &mut selected,
        &mut diagnostics,
    );
    if let Some(project_root) = options.trusted_project_root.as_deref() {
        validate_trusted_project_root(project_root, &options.working_dir)?;
        collect_candidates(
            &project_root.join(".astro/extensions"),
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
        match resolve_candidate(candidate, &options.effective_config) {
            Ok(extension) => resolved_candidates.push(extension),
            Err(error) => diagnostics.push(ExtensionDiagnostic {
                manifest_path: error.0,
                extension_id: error.1,
                message: error.2,
            }),
        }
    }

    let mut mcp_by_id = BTreeMap::new();
    for config in mcp::decode_mcp_servers_from_value(&options.effective_config)? {
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

    let skill_index = skills::list_enabled_for_prompt_with_config_in_workspace(
        &options.working_dir,
        &skill_configs,
    );
    let mcp_servers = mcp_by_id.into_values().collect::<Vec<_>>();
    let version = snapshot_fingerprint(
        &options.config_version,
        &options.working_dir,
        &resolved,
        &mcp_servers,
        &skill_configs,
        &skill_index,
        &toolsets,
    )?;

    Ok(ExtensionSnapshot {
        version,
        config_version: options.config_version.clone(),
        effective_config: options.effective_config.clone(),
        working_dir: options.working_dir.clone(),
        extensions: resolved,
        mcp_servers,
        skill_configs,
        skill_index,
        toolsets,
        diagnostics,
    })
}

fn validate_trusted_project_root(project_root: &Path, working_dir: &Path) -> Result<()> {
    let project_root = project_root.canonicalize().with_context(|| {
        format!(
            "canonicalize trusted project root {}",
            project_root.display()
        )
    })?;
    let working_dir = working_dir.canonicalize().with_context(|| {
        format!(
            "canonicalize extension working directory {}",
            working_dir.display()
        )
    })?;
    anyhow::ensure!(
        working_dir.starts_with(&project_root),
        "trusted project root {} is not an ancestor of working directory {}",
        project_root.display(),
        working_dir.display()
    );
    Ok(())
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
    if let Some(schema) = config_schema.as_ref() {
        validate_extension_config(extension_id, schema, &config)?;
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

fn validate_extension_config(
    extension_id: &str,
    schema: &serde_json::Value,
    config: &toml::Value,
) -> Result<()> {
    jsonschema::meta::validate(schema)
        .map_err(|error| anyhow::anyhow!("invalid config JSON Schema: {error}"))?;
    let validator = jsonschema::validator_for(schema)
        .map_err(|error| anyhow::anyhow!("compile config JSON Schema: {error}"))?;
    let instance = serde_json::to_value(config)
        .with_context(|| format!("serialize configuration for extension {extension_id}"))?;
    let errors = validator
        .iter_errors(&instance)
        .take(8)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    anyhow::ensure!(
        errors.is_empty(),
        "extension configuration does not match JSON Schema: {}",
        errors.join("; ")
    );
    Ok(())
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
        .map(resolved_extension_value)
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

fn resolved_extension_value(extension: &ResolvedExtension) -> serde_json::Value {
    serde_json::json!({
        "manifest": &extension.manifest,
        "root": &extension.root,
        "scope": extension.scope,
        "config_schema": &extension.config_schema,
        "config": &extension.config,
        "mcp_servers": &extension.mcp_servers,
        "skill_configs": &extension.skill_configs,
        "toolsets": &extension.toolsets,
    })
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

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home/.astro");
        let project = root.path().join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(project.join(".git")).unwrap();
        (root, home, project)
    }

    fn discovery_options(home: &Path, project: &Path, trusted: bool) -> ExtensionDiscoveryOptions {
        let config = toml::from_str("[extensions.demo]\nlabel = 'configured'\n").unwrap();
        discovery_options_with_config(home, project, trusted, config)
    }

    fn discovery_options_with_config(
        home: &Path,
        project: &Path,
        trusted: bool,
        config: toml::Value,
    ) -> ExtensionDiscoveryOptions {
        let options = ExtensionDiscoveryOptions::new(home, project, config, "test-config-v1");
        if trusted {
            options.with_trusted_project_root(project)
        } else {
            options
        }
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
            discover_extension_snapshot(&discovery_options(&home, &project, true)).unwrap();

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
            discover_extension_snapshot(&discovery_options(&home, &project, true)).unwrap();

        assert_eq!(snapshot.extensions().len(), 1);
        assert_eq!(snapshot.extensions()[0].scope, ExtensionScope::Project);
        assert_eq!(snapshot.extensions()[0].manifest.description, "project");
    }

    #[test]
    fn session_skill_override_wins_over_manifest_contribution() {
        let (_temp, home, project) = fixture();
        write_demo_extension(&home.join("extensions"), "global");
        let skill_path = home.join("extensions/demo/skills/demo-skill");
        let mut options = discovery_options(&home, &project, true);
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
        write_demo_extension(&project.join(".astro/extensions"), "project");

        let snapshot =
            discover_extension_snapshot(&discovery_options(&home, &project, false)).unwrap();

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
            discover_extension_snapshot(&discovery_options(&home, &project, true)).unwrap();

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
        let config = toml::from_str(
            "[extensions.demo]\nlabel = 'configured'\n\
             [mcp_servers.ext-demo-echo]\nurl = 'https://example.invalid/mcp'\n",
        )
        .unwrap();
        let options = discovery_options_with_config(&home, &project, true, config);
        let snapshot = discover_extension_snapshot(&options).unwrap();

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
        let first = discover_extension_snapshot(&discovery_options(&home, &project, true)).unwrap();
        write_demo_extension(&root, "after");

        assert_eq!(first.extensions()[0].manifest.description, "before");
        let second =
            discover_extension_snapshot(&discovery_options(&home, &project, true)).unwrap();
        assert_eq!(second.extensions()[0].manifest.description, "after");
        assert_ne!(first.version(), second.version());
    }

    #[test]
    fn reconcile_reports_updated_contributions_without_mutating_previous() {
        let (_temp, home, project) = fixture();
        let root = home.join("extensions");
        write_demo_extension(&root, "before");
        let previous =
            discover_extension_snapshot(&discovery_options(&home, &project, true)).unwrap();
        write_demo_extension(&root, "after");
        let next = discover_extension_snapshot(&discovery_options(&home, &project, true)).unwrap();

        let report = reconcile_extension_snapshots(&previous, &next).unwrap();

        assert_eq!(previous.extensions()[0].manifest.description, "before");
        assert_eq!(report.changed_extensions.len(), 1);
        assert_eq!(report.changed_extensions[0].extension_id, "demo");
        assert_eq!(
            report.changed_extensions[0].kind,
            ExtensionChangeKind::Updated
        );
        assert!(report.refresh_hooks);
        assert!(!report.refresh_mcp);
        assert!(!report.refresh_skills);
        assert!(!report.refresh_toolsets);
    }

    #[test]
    fn reconcile_reports_added_and_removed_extension_contributions() {
        let (_temp, home, project) = fixture();
        let empty = discover_extension_snapshot(&discovery_options(&home, &project, true)).unwrap();
        write_demo_extension(&home.join("extensions"), "added");
        let added = discover_extension_snapshot(&discovery_options(&home, &project, true)).unwrap();

        let add_report = reconcile_extension_snapshots(&empty, &added).unwrap();
        assert_eq!(
            add_report.changed_extensions[0].kind,
            ExtensionChangeKind::Added
        );
        assert!(add_report.refresh_mcp);
        assert!(add_report.refresh_skills);
        assert!(add_report.refresh_toolsets);

        fs::remove_dir_all(home.join("extensions/demo")).unwrap();
        let removed =
            discover_extension_snapshot(&discovery_options(&home, &project, true)).unwrap();
        let remove_report = reconcile_extension_snapshots(&added, &removed).unwrap();
        assert_eq!(
            remove_report.changed_extensions[0].kind,
            ExtensionChangeKind::Removed
        );
        assert!(remove_report.refresh_mcp);
        assert!(remove_report.refresh_skills);
        assert!(remove_report.refresh_toolsets);
    }

    #[test]
    fn config_must_match_declared_json_schema() {
        let (_temp, home, project) = fixture();
        write_demo_extension(&home.join("extensions"), "global");
        let config = toml::from_str("[extensions.demo]\nlabel = 42\n").unwrap();

        let snapshot = discover_extension_snapshot(&discovery_options_with_config(
            &home, &project, true, config,
        ))
        .unwrap();

        assert!(snapshot.extensions().is_empty());
        assert_eq!(snapshot.diagnostics().len(), 1);
        assert!(snapshot.diagnostics()[0]
            .message
            .contains("does not match JSON Schema"));
    }

    #[test]
    fn malformed_json_schema_skips_the_whole_extension() {
        let (_temp, home, project) = fixture();
        write_demo_extension(&home.join("extensions"), "global");
        fs::write(
            home.join("extensions/demo/schema.json"),
            r#"{"type":"not-a-json-schema-type"}"#,
        )
        .unwrap();

        let snapshot =
            discover_extension_snapshot(&discovery_options(&home, &project, true)).unwrap();

        assert!(snapshot.extensions().is_empty());
        assert_eq!(snapshot.diagnostics().len(), 1);
        assert!(snapshot.diagnostics()[0]
            .message
            .contains("invalid config JSON Schema"));
    }

    #[test]
    fn trusted_project_root_must_contain_the_working_directory() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home/.astro");
        let project = root.path().join("project");
        let unrelated = root.path().join("unrelated");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&unrelated).unwrap();
        let options =
            ExtensionDiscoveryOptions::new(&home, &project, empty_table(), "test-config-v1")
                .with_trusted_project_root(&unrelated);

        let error = discover_extension_snapshot(&options).unwrap_err();

        assert!(error.to_string().contains("is not an ancestor"));
    }
}
