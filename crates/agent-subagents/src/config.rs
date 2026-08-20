use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use agent_config::loader::{load_local_config, LocalConfigOptions, ProjectTrust};
use agent_config::{ConfigLayerSource, EffectiveConfig};
use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub developer_instructions: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub model_reasoning_effort: Option<String>,
    #[serde(default)]
    pub sandbox_mode: Option<String>,
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub skills: SkillsLayer,
    #[serde(default)]
    pub nickname_candidates: Vec<String>,
    /// Codex agent files are configuration layers. Preserve forward-compatible
    /// keys even when Astro does not consume them yet.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SkillsLayer {
    #[serde(default)]
    pub config: Vec<SkillConfigEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkillConfigEntry {
    pub path: PathBuf,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone)]
pub struct AgentConfigDiagnostic {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct AgentCatalog {
    pub agents: BTreeMap<String, AgentDefinition>,
    pub diagnostics: Vec<AgentConfigDiagnostic>,
}

#[derive(Debug, Clone)]
pub struct AgentConfiguration {
    pub settings: AgentsSettings,
    pub catalog: AgentCatalog,
    /// Version of the merged `config.toml` value. Standalone agent files are
    /// separate layers and are intentionally not represented by this hash.
    pub effective_config_version: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AgentsSettings {
    pub enabled: bool,
    pub max_concurrent_threads_per_session: usize,
    pub default_subagent_model: Option<String>,
    pub default_subagent_reasoning_effort: Option<String>,
    pub interrupt_message: bool,
}

impl Default for AgentsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent_threads_per_session: 4,
            default_subagent_model: None,
            default_subagent_reasoning_effort: None,
            interrupt_message: true,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct RootConfig {
    #[serde(default)]
    agents: Option<PartialAgentsSettings>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialAgentsSettings {
    enabled: Option<bool>,
    max_concurrent_threads_per_session: Option<usize>,
    #[serde(default)]
    max_threads: Option<usize>,
    default_subagent_model: Option<String>,
    default_subagent_reasoning_effort: Option<String>,
    interrupt_message: Option<bool>,
    #[serde(default, rename = "max_depth")]
    _max_depth: Option<i32>,
    #[serde(default, rename = "job_max_runtime_seconds")]
    _job_max_runtime_seconds: Option<u64>,
    #[serde(default, flatten)]
    roles: BTreeMap<String, AgentRoleSettings>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AgentRoleSettings {
    description: Option<String>,
    config_file: Option<PathBuf>,
    nickname_candidates: Option<Vec<String>>,
}

impl AgentsSettings {
    fn apply(&mut self, partial: &PartialAgentsSettings) {
        if let Some(value) = partial.enabled {
            self.enabled = value;
        }
        if let Some(value) = partial
            .max_concurrent_threads_per_session
            .or(partial.max_threads)
        {
            self.max_concurrent_threads_per_session = value.clamp(1, 32);
        }
        if partial.default_subagent_model.is_some() {
            self.default_subagent_model = partial.default_subagent_model.clone();
        }
        if partial.default_subagent_reasoning_effort.is_some() {
            self.default_subagent_reasoning_effort =
                partial.default_subagent_reasoning_effort.clone();
        }
        if let Some(value) = partial.interrupt_message {
            self.interrupt_message = value;
        }
    }
}

pub fn load_agent_configuration(
    memory_dir: &Path,
    project_root: Option<&Path>,
) -> anyhow::Result<AgentConfiguration> {
    let astro_home = astro_home(memory_dir);
    let cwd = project_root.unwrap_or(memory_dir);
    let options = LocalConfigOptions::new(&astro_home, cwd);
    let loaded = load_local_config(&options).with_context(|| {
        format!(
            "failed to load Codex configuration for agent settings from {}",
            astro_home.display()
        )
    })?;
    let effective = loaded.resolve();
    let root = decode_root_config(&effective)?;
    let settings = agents_settings_from_root(&root);

    let catalog = load_agent_catalog_layers(
        &loaded.layers,
        &astro_home,
        &loaded.project_root,
        loaded.project_trust,
    );

    Ok(AgentConfiguration {
        settings,
        catalog,
        effective_config_version: effective.version().to_string(),
    })
}

fn decode_root_config(effective: &EffectiveConfig) -> anyhow::Result<RootConfig> {
    effective
        .decode::<RootConfig>()
        .context("invalid effective [agents] configuration")
}

fn agents_settings_from_root(root: &RootConfig) -> AgentsSettings {
    let mut settings = AgentsSettings::default();
    if let Some(partial) = root.agents.as_ref() {
        settings.apply(partial);
    }
    settings
}

fn astro_home(memory_dir: &Path) -> PathBuf {
    if memory_dir.file_name().and_then(|name| name.to_str()) == Some(".astro") {
        return memory_dir.to_path_buf();
    }
    memory_dir.join(".astro")
}

fn load_agent_catalog_layers(
    layers: &agent_config::ConfigLayerStack,
    astro_home: &Path,
    project_root: &Path,
    project_trust: ProjectTrust,
) -> AgentCatalog {
    let mut catalog = AgentCatalog::default();
    for definition in builtin_agents() {
        catalog.agents.insert(definition.name.clone(), definition);
    }

    let mut loaded_dirs = Vec::new();
    let user_agents_dir = astro_home.join("agents");
    let project_agents_dir = project_root.join(".astro/agents");

    for layer in layers
        .layers_low_to_high()
        .filter(|layer| layer.is_enabled())
    {
        let layer_root = match layer.config.clone().try_into::<RootConfig>() {
            Ok(root) => Some(root),
            Err(error) => {
                catalog.diagnostics.push(AgentConfigDiagnostic {
                    path: source_label(&layer.source),
                    message: format!("invalid declared agent roles: {error}"),
                });
                None
            }
        };
        let config_base_dir = config_base_dir(&layer.source);
        let declared_role_files = layer_root
            .as_ref()
            .and_then(|root| root.agents.as_ref())
            .map(|agents| declared_role_files(config_base_dir.as_deref(), &agents.roles))
            .unwrap_or_default();

        // Standalone agent files participate at the same precedence boundary
        // as their owning config layer. Load them immediately before that
        // layer's declarations so declarations win within a layer, while a
        // higher project layer still overrides lower personal declarations.
        if matches!(
            layer.source,
            ConfigLayerSource::User { .. }
                | ConfigLayerSource::Profile { .. }
                | ConfigLayerSource::Project { .. }
                | ConfigLayerSource::Agent { .. }
                | ConfigLayerSource::SessionOverrides
                | ConfigLayerSource::RequestOverrides
        ) {
            load_agent_dir_once(
                &user_agents_dir,
                &declared_role_files,
                &mut loaded_dirs,
                &mut catalog,
            );
        }
        if project_trust == ProjectTrust::Trusted
            && matches!(
                layer.source,
                ConfigLayerSource::Project { .. }
                    | ConfigLayerSource::Agent { .. }
                    | ConfigLayerSource::SessionOverrides
                    | ConfigLayerSource::RequestOverrides
            )
        {
            load_agent_dir_once(
                &project_agents_dir,
                &declared_role_files,
                &mut loaded_dirs,
                &mut catalog,
            );
        }
        if let Some(directory) = agent_dir_for_source(&layer.source) {
            load_agent_dir_once(
                &directory,
                &declared_role_files,
                &mut loaded_dirs,
                &mut catalog,
            );
        }
        if let Some(agents) = layer_root.and_then(|root| root.agents) {
            load_declared_agent_roles(config_base_dir.as_deref(), &agents.roles, &mut catalog);
        }
    }

    // The directories are configuration inputs even when their sibling
    // config.toml does not exist, so ensure the terminal personal/project
    // layers are still represented when the layer stack did not cross them.
    load_agent_dir_once(&user_agents_dir, &[], &mut loaded_dirs, &mut catalog);
    if project_trust == ProjectTrust::Trusted {
        load_agent_dir_once(&project_agents_dir, &[], &mut loaded_dirs, &mut catalog);
    }
    catalog
}

fn load_agent_dir_once(
    directory: &Path,
    declared_role_files: &[PathBuf],
    loaded_dirs: &mut Vec<PathBuf>,
    catalog: &mut AgentCatalog,
) {
    if loaded_dirs.iter().any(|loaded| loaded == directory) {
        return;
    }
    loaded_dirs.push(directory.to_path_buf());
    load_agent_dir(directory, declared_role_files, catalog);
}

fn declared_role_files(
    config_base_dir: Option<&Path>,
    roles: &BTreeMap<String, AgentRoleSettings>,
) -> Vec<PathBuf> {
    roles
        .values()
        .filter_map(|role| role.config_file.as_deref())
        .filter_map(|path| {
            if path.is_absolute() {
                Some(path.to_path_buf())
            } else {
                config_base_dir.map(|base_dir| base_dir.join(path))
            }
        })
        .collect()
}

fn agent_dir_for_source(source: &ConfigLayerSource) -> Option<PathBuf> {
    match source {
        ConfigLayerSource::User { file } | ConfigLayerSource::Profile { file, .. } => {
            file.parent().map(|parent| parent.join("agents"))
        }
        ConfigLayerSource::Project { dot_config_dir } => Some(dot_config_dir.join("agents")),
        _ => None,
    }
}

fn config_base_dir(source: &ConfigLayerSource) -> Option<PathBuf> {
    match source {
        ConfigLayerSource::PackagedDefaults { file }
        | ConfigLayerSource::System { file }
        | ConfigLayerSource::User { file }
        | ConfigLayerSource::Profile { file, .. }
        | ConfigLayerSource::Agent { file, .. } => file.parent().map(Path::to_path_buf),
        ConfigLayerSource::Project { dot_config_dir } => Some(dot_config_dir.clone()),
        ConfigLayerSource::ManagedPreferences { .. }
        | ConfigLayerSource::EnterpriseManaged { .. }
        | ConfigLayerSource::SessionOverrides
        | ConfigLayerSource::RequestOverrides => None,
    }
}

fn source_label(source: &ConfigLayerSource) -> PathBuf {
    config_base_dir(source).unwrap_or_else(|| PathBuf::from(format!("{source:?}")))
}

#[derive(Debug, Default, Deserialize)]
struct AgentDefinitionFile {
    name: Option<String>,
    description: Option<String>,
    developer_instructions: Option<String>,
    model: Option<String>,
    model_reasoning_effort: Option<String>,
    sandbox_mode: Option<String>,
    #[serde(default)]
    mcp_servers: BTreeMap<String, toml::Value>,
    #[serde(default)]
    skills: SkillsLayer,
    nickname_candidates: Option<Vec<String>>,
    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
}

fn load_declared_agent_roles(
    config_base_dir: Option<&Path>,
    roles: &BTreeMap<String, AgentRoleSettings>,
    catalog: &mut AgentCatalog,
) {
    for (declared_name, role) in roles {
        let description = match normalize_optional_description(
            &format!("agents.{declared_name}.description"),
            role.description.as_deref(),
        ) {
            Ok(description) => description,
            Err(error) => {
                catalog.diagnostics.push(AgentConfigDiagnostic {
                    path: PathBuf::from(format!("agents.{declared_name}")),
                    message: error.to_string(),
                });
                continue;
            }
        };
        let nickname_candidates = match normalize_nickname_candidates(
            &format!("agents.{declared_name}.nickname_candidates"),
            role.nickname_candidates.as_deref(),
        ) {
            Ok(candidates) => candidates,
            Err(error) => {
                catalog.diagnostics.push(AgentConfigDiagnostic {
                    path: PathBuf::from(format!("agents.{declared_name}")),
                    message: error.to_string(),
                });
                continue;
            }
        };
        let Some(config_file) = role.config_file.as_deref() else {
            if let Some(existing) = catalog.agents.get_mut(declared_name) {
                if let Some(description) = description {
                    existing.description = description;
                }
                if let Some(candidates) = nickname_candidates {
                    existing.nickname_candidates = candidates;
                }
            } else {
                catalog.diagnostics.push(AgentConfigDiagnostic {
                    path: PathBuf::from(format!("agents.{declared_name}")),
                    message: "config_file is required for a new declared agent role".into(),
                });
            }
            continue;
        };

        let path = if config_file.is_absolute() {
            config_file.to_path_buf()
        } else {
            let Some(base_dir) = config_base_dir else {
                catalog.diagnostics.push(AgentConfigDiagnostic {
                    path: config_file.to_path_buf(),
                    message: "relative config_file has no filesystem-backed config layer".into(),
                });
                continue;
            };
            base_dir.join(config_file)
        };

        match read_agent_definition_file(
            &path,
            Some(declared_name),
            description.as_deref(),
            nickname_candidates.as_deref(),
            Some(&catalog.agents),
        ) {
            Ok(agent) => {
                catalog.agents.insert(agent.name.clone(), agent);
            }
            Err(error) => catalog.diagnostics.push(AgentConfigDiagnostic {
                path,
                message: error.to_string(),
            }),
        }
    }
}

fn read_agent_definition_file(
    path: &Path,
    name_hint: Option<&str>,
    description_hint: Option<&str>,
    nickname_candidates_hint: Option<&[String]>,
    fallback_agents: Option<&BTreeMap<String, AgentDefinition>>,
) -> anyhow::Result<AgentDefinition> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read agent role file {}", path.display()))?;
    let parsed = toml::from_str::<AgentDefinitionFile>(&text)
        .with_context(|| format!("failed to parse agent role file {}", path.display()))?;
    let name = non_empty(parsed.name.as_deref())
        .or_else(|| non_empty(name_hint))
        .context("name is required")?;
    let fallback = fallback_agents.and_then(|agents| agents.get(&name));
    let file_description = normalize_optional_description(
        &format!("agent role file {}.description", path.display()),
        parsed.description.as_deref(),
    )?;
    let description = file_description
        .or_else(|| description_hint.map(ToOwned::to_owned))
        .or_else(|| fallback.map(|agent| agent.description.clone()))
        .context("description is required")?;
    let developer_instructions = non_empty(parsed.developer_instructions.as_deref())
        .context("developer_instructions is required")?;
    let mut skills = parsed.skills;
    if let Some(parent) = path.parent() {
        for skill in &mut skills.config {
            if skill.path.is_relative() {
                skill.path = parent.join(&skill.path);
            }
        }
    }

    Ok(AgentDefinition {
        name,
        description,
        developer_instructions,
        model: parsed.model,
        model_reasoning_effort: parsed.model_reasoning_effort,
        sandbox_mode: parsed.sandbox_mode,
        mcp_servers: parsed.mcp_servers,
        skills,
        nickname_candidates: normalize_nickname_candidates(
            &format!("agent role file {}.nickname_candidates", path.display()),
            parsed.nickname_candidates.as_deref(),
        )?
        .or_else(|| nickname_candidates_hint.map(|candidates| candidates.to_vec()))
        .or_else(|| fallback.map(|agent| agent.nickname_candidates.clone()))
        .unwrap_or_default(),
        extra: parsed.extra,
    })
}

fn load_agent_dir(dir: &Path, declared_role_files: &[PathBuf], catalog: &mut AgentCatalog) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("toml"))
        .collect();
    paths.sort();
    for path in paths {
        if declared_role_files
            .iter()
            .any(|declared| paths_refer_to_same_file(&path, declared))
        {
            continue;
        }
        match read_agent_definition_file(&path, None, None, None, None) {
            Ok(agent) => {
                catalog.agents.insert(agent.name.clone(), agent);
            }
            Err(error) => catalog.diagnostics.push(AgentConfigDiagnostic {
                path,
                message: error.to_string(),
            }),
        }
    }
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn builtin_agents() -> Vec<AgentDefinition> {
    vec![
        AgentDefinition {
            name: "default".into(),
            description: "General-purpose fallback agent.".into(),
            developer_instructions: "Complete the delegated task. Keep the parent informed with a concise, evidence-based result.".into(),
            model: None,
            model_reasoning_effort: None,
            sandbox_mode: None,
            mcp_servers: BTreeMap::new(),
            skills: SkillsLayer::default(),
            nickname_candidates: Vec::new(),
            extra: BTreeMap::new(),
        },
        AgentDefinition {
            name: "worker".into(),
            description: "Execution-focused agent for implementation and fixes.".into(),
            developer_instructions: "Own the implementation task end to end. Make focused changes, validate them, and report files changed plus verification results.".into(),
            model: None,
            model_reasoning_effort: None,
            sandbox_mode: None,
            mcp_servers: BTreeMap::new(),
            skills: SkillsLayer::default(),
            nickname_candidates: Vec::new(),
            extra: BTreeMap::new(),
        },
        AgentDefinition {
            name: "explorer".into(),
            description: "Read-heavy codebase exploration agent.".into(),
            developer_instructions: "Stay read-only. Trace real code paths, gather evidence, and return concise findings with file and symbol references.".into(),
            model: None,
            model_reasoning_effort: None,
            sandbox_mode: Some("read-only".into()),
            mcp_servers: BTreeMap::new(),
            skills: SkillsLayer::default(),
            nickname_candidates: Vec::new(),
            extra: BTreeMap::new(),
        },
    ]
}

#[derive(Debug, Clone)]
pub struct ResolvedAgent {
    pub definition: AgentDefinition,
    pub model: Option<String>,
    pub model_reasoning_effort: Option<String>,
    pub sandbox_mode: Option<String>,
}

pub fn resolve_agent(
    catalog: &AgentCatalog,
    settings: &AgentsSettings,
    name: &str,
    explicit_model: Option<&str>,
    explicit_effort: Option<&str>,
    parent_model: Option<&str>,
    parent_sandbox_mode: Option<&str>,
) -> anyhow::Result<ResolvedAgent> {
    let definition = catalog
        .agents
        .get(name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("unknown agent: {name}"))?;

    let model = definition
        .model
        .clone()
        .or_else(|| non_empty(explicit_model))
        .or_else(|| settings.default_subagent_model.clone())
        .or_else(|| non_empty(parent_model));
    let model_reasoning_effort = definition
        .model_reasoning_effort
        .clone()
        .or_else(|| non_empty(explicit_effort))
        .or_else(|| settings.default_subagent_reasoning_effort.clone());
    let sandbox_mode =
        resolve_sandbox_mode(parent_sandbox_mode, definition.sandbox_mode.as_deref());

    Ok(ResolvedAgent {
        definition,
        model,
        model_reasoning_effort,
        sandbox_mode,
    })
}

fn resolve_sandbox_mode(parent: Option<&str>, requested: Option<&str>) -> Option<String> {
    let parent = non_empty(parent)?;
    let Some(requested) = non_empty(requested) else {
        return Some(parent);
    };
    let rank = |mode: &str| match mode.trim().to_ascii_lowercase().as_str() {
        "read-only" | "read_only" => Some(0),
        "workspace-write" | "workspace_write" => Some(1),
        "danger-full-access" | "danger_full_access" => Some(2),
        _ => None,
    };
    match (rank(&parent), rank(&requested)) {
        (Some(parent_rank), Some(requested_rank)) if requested_rank <= parent_rank => {
            Some(requested)
        }
        // Unknown/custom profiles have no comparable privilege ordering.  A
        // child may never replace one, and an unknown child request may never
        // replace a known parent, so both cases conservatively inherit.
        _ => Some(parent),
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_optional_description(
    field: &str,
    value: Option<&str>,
) -> anyhow::Result<Option<String>> {
    match value.map(str::trim) {
        Some("") => anyhow::bail!("{field} cannot be blank"),
        Some(value) => Ok(Some(value.to_string())),
        None => Ok(None),
    }
}

fn normalize_nickname_candidates(
    field: &str,
    value: Option<&[String]>,
) -> anyhow::Result<Option<Vec<String>>> {
    let Some(value) = value else {
        return Ok(None);
    };
    anyhow::ensure!(!value.is_empty(), "{field} must contain at least one name");

    let mut normalized = Vec::with_capacity(value.len());
    for candidate in value {
        let candidate = candidate.trim();
        anyhow::ensure!(!candidate.is_empty(), "{field} cannot contain blank names");
        anyhow::ensure!(
            candidate
                .chars()
                .all(|character| character.is_ascii_alphanumeric()
                    || matches!(character, ' ' | '-' | '_')),
            "{field} may only contain ASCII letters, digits, spaces, hyphens, and underscores"
        );
        anyhow::ensure!(
            !normalized.iter().any(|existing| existing == candidate),
            "{field} cannot contain duplicates"
        );
        normalized.push(candidate.to_string());
    }
    Ok(Some(normalized))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toml_key(path: &Path) -> String {
        format!("{:?}", path.to_string_lossy())
    }

    #[test]
    fn codex_agent_directories_are_not_configuration_inputs() {
        let memory = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(memory.path().join(".codex/agents")).unwrap();
        fs::create_dir_all(project.path().join(".codex/agents")).unwrap();
        fs::write(
            memory.path().join(".codex/agents/legacy.toml"),
            "name = \"legacy\"\ndescription = \"legacy\"\ndeveloper_instructions = \"legacy\"\n",
        )
        .unwrap();
        fs::write(
            project.path().join(".codex/agents/project_legacy.toml"),
            "name = \"project_legacy\"\ndescription = \"legacy\"\ndeveloper_instructions = \"legacy\"\n",
        )
        .unwrap();

        let catalog = load_agent_configuration(memory.path(), Some(project.path()))
            .unwrap()
            .catalog;

        assert!(!catalog.agents.contains_key("legacy"));
        assert!(!catalog.agents.contains_key("project_legacy"));
    }

    #[test]
    fn codex_config_toml_is_not_a_configuration_input() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".codex")).unwrap();
        fs::write(
            root.path().join(".codex/config.toml"),
            "[agents]\nenabled = false\n",
        )
        .unwrap();

        assert!(
            load_agent_configuration(root.path(), None)
                .unwrap()
                .settings
                .enabled
        );
    }

    #[test]
    fn memory_config_toml_is_not_a_configuration_input() {
        let memory = tempfile::tempdir().unwrap();
        fs::write(
            memory.path().join("config.toml"),
            "[agents]\nenabled = false\n",
        )
        .unwrap();

        assert!(
            load_agent_configuration(memory.path(), None)
                .unwrap()
                .settings
                .enabled
        );
    }

    #[test]
    fn trusted_project_astro_config_toml_is_a_configuration_input() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(project.path().join(".astro")).unwrap();
        fs::write(
            memory.join("config.toml"),
            format!(
                "[projects.{}]\ntrust_level = 'trusted'\n",
                toml_key(project.path())
            ),
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/config.toml"),
            "[agents]\nenabled = false\n",
        )
        .unwrap();

        assert!(
            !load_agent_configuration(&memory, Some(project.path()))
                .unwrap()
                .settings
                .enabled
        );
    }

    #[test]
    fn astro_settings_load_from_home_and_project_overrides_personal() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(home.path().join(".astro")).unwrap();
        fs::create_dir_all(project.path().join(".astro/agents")).unwrap();
        fs::write(
            home.path().join(".astro/config.toml"),
            format!(
                "[agents]\nenabled = false\nmax_threads = 7\n\n[projects.{}]\ntrust_level = 'trusted'\n",
                toml_key(project.path())
            ),
        )
        .unwrap();

        let personal = load_agent_configuration(&memory, None).unwrap().settings;
        assert!(!personal.enabled);
        assert_eq!(personal.max_concurrent_threads_per_session, 7);

        fs::write(
            project.path().join(".astro/config.toml"),
            "[agents]\nenabled = true\nmax_threads = 9\n",
        )
        .unwrap();
        let combined = load_agent_configuration(&memory, Some(project.path()))
            .unwrap()
            .settings;
        assert!(combined.enabled);
        assert_eq!(combined.max_concurrent_threads_per_session, 9);
    }

    #[test]
    fn untrusted_project_agent_settings_are_not_applied() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(home.path().join(".astro")).unwrap();
        fs::create_dir_all(project.path().join(".astro/agents")).unwrap();
        fs::write(
            home.path().join(".astro/config.toml"),
            "[agents]\nenabled = true\nmax_concurrent_threads_per_session = 5\n",
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/config.toml"),
            r#"[agents]
enabled = false
max_concurrent_threads_per_session = 11

[agents.untrusted]
description = "ignored declaration"
config_file = "agents/untrusted.toml"
"#,
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/agents/untrusted.toml"),
            "name = 'untrusted'\ndescription = 'ignored'\ndeveloper_instructions = 'ignored'\n",
        )
        .unwrap();

        let configuration = load_agent_configuration(&memory, Some(project.path())).unwrap();
        let settings = configuration.settings;

        assert!(settings.enabled);
        assert_eq!(settings.max_concurrent_threads_per_session, 5);
        assert!(!configuration.catalog.agents.contains_key("untrusted"));
    }

    #[test]
    fn invalid_agent_settings_fail_instead_of_silently_using_defaults() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(home.path().join(".astro")).unwrap();
        fs::write(
            home.path().join(".astro/config.toml"),
            "[agents]\nmax_concurrent_threads_per_session = 'many'\n",
        )
        .unwrap();

        let error = load_agent_configuration(&memory, None).unwrap_err();

        assert!(error.to_string().contains("invalid effective [agents]"));
    }

    #[test]
    fn custom_agent_overrides_builtin_and_model_precedence() {
        let root = tempfile::tempdir().unwrap();
        let agents = root.path().join(".astro/agents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("explorer.toml"),
            r#"name = "explorer"
description = "custom"
developer_instructions = "custom instructions"
model = "provider:custom"
"#,
        )
        .unwrap();
        let catalog = load_agent_configuration(root.path(), None).unwrap().catalog;
        let resolved = resolve_agent(
            &catalog,
            &AgentsSettings::default(),
            "explorer",
            Some("provider:explicit"),
            None,
            Some("provider:parent"),
            Some("workspace-write"),
        )
        .unwrap();
        assert_eq!(resolved.model.as_deref(), Some("provider:custom"));
        assert_eq!(resolved.definition.description, "custom");
        assert_eq!(resolved.sandbox_mode.as_deref(), Some("workspace-write"));
    }

    #[test]
    fn custom_agent_can_narrow_but_not_expand_parent_permissions() {
        let root = tempfile::tempdir().unwrap();
        let agents = root.path().join(".astro/agents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("unsafe.toml"),
            r#"name = "unsafe"
description = "invalid permission override"
developer_instructions = "work"
sandbox_mode = "danger-full-access"
"#,
        )
        .unwrap();
        let catalog = load_agent_configuration(root.path(), None).unwrap().catalog;
        let resolved = resolve_agent(
            &catalog,
            &AgentsSettings::default(),
            "unsafe",
            None,
            None,
            None,
            Some("workspace-write"),
        )
        .unwrap();
        assert_eq!(resolved.sandbox_mode.as_deref(), Some("workspace-write"));

        let mut narrowed = catalog.clone();
        narrowed.agents.get_mut("unsafe").unwrap().sandbox_mode = Some("read-only".into());
        let resolved = resolve_agent(
            &narrowed,
            &AgentsSettings::default(),
            "unsafe",
            None,
            None,
            None,
            Some("workspace-write"),
        )
        .unwrap();
        assert_eq!(resolved.sandbox_mode.as_deref(), Some("read-only"));
    }

    #[test]
    fn sandbox_resolution_is_conservative_for_unknown_profiles() {
        assert_eq!(
            resolve_sandbox_mode(Some("locked"), Some("danger-full-access")).as_deref(),
            Some("locked")
        );
        assert_eq!(
            resolve_sandbox_mode(Some("workspace-write"), Some("custom-unconfined")).as_deref(),
            Some("workspace-write")
        );
        assert_eq!(
            resolve_sandbox_mode(Some("locked"), Some("locked")).as_deref(),
            Some("locked")
        );
    }

    #[test]
    fn sandbox_resolution_known_matrix_and_aliases_only_narrows() {
        let cases = [
            ("read-only", "read-only", "read-only"),
            ("read-only", "workspace-write", "read-only"),
            ("read-only", "danger-full-access", "read-only"),
            ("workspace-write", "read-only", "read-only"),
            ("workspace-write", "workspace-write", "workspace-write"),
            ("workspace-write", "danger-full-access", "workspace-write"),
            ("danger-full-access", "read-only", "read-only"),
            ("danger-full-access", "workspace-write", "workspace-write"),
            (
                "danger-full-access",
                "danger-full-access",
                "danger-full-access",
            ),
            ("workspace_write", "read_only", "read_only"),
            ("workspace_write", "danger_full_access", "workspace_write"),
        ];

        for (parent, requested, expected) in cases {
            assert_eq!(
                resolve_sandbox_mode(Some(parent), Some(requested)).as_deref(),
                Some(expected),
                "parent={parent}, requested={requested}"
            );
        }
    }

    #[test]
    fn codex_paths_preserve_precedence_and_decode_layers() {
        let memory = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(memory.path().join(".astro/agents")).unwrap();
        fs::create_dir_all(project.path().join(".astro/agents")).unwrap();
        fs::write(
            memory.path().join(".astro/config.toml"),
            format!(
                "[projects.{}]\ntrust_level = 'trusted'\n",
                toml_key(project.path())
            ),
        )
        .unwrap();
        fs::write(
            memory.path().join(".astro/agents/reviewer.toml"),
            r#"name = "reviewer"
description = "personal codex"
developer_instructions = "personal"
"#,
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/agents/reviewer.toml"),
            r#"name = "reviewer"
description = "project codex"
developer_instructions = "project"
sandbox_mode = "read-only"
future_setting = "preserved"

[mcp_servers.docs]
url = "https://example.invalid/mcp"

[[skills.config]]
path = "/tmp/reviewer/SKILL.md"
enabled = false
"#,
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/config.toml"),
            "[agents]\nenabled = false\n",
        )
        .unwrap();

        let configuration = load_agent_configuration(memory.path(), Some(project.path())).unwrap();
        assert!(!configuration.settings.enabled);
        assert!(configuration
            .effective_config_version
            .starts_with("sha256:"));
        let catalog = configuration.catalog;
        let reviewer = catalog.agents.get("reviewer").unwrap();
        assert_eq!(reviewer.description, "project codex");
        assert_eq!(reviewer.sandbox_mode.as_deref(), Some("read-only"));
        assert!(reviewer.mcp_servers.contains_key("docs"));
        assert_eq!(reviewer.skills.config.len(), 1);
        assert!(reviewer.extra.contains_key("future_setting"));
    }

    #[test]
    fn trusted_project_agent_directories_follow_root_to_cwd_layer_order() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        let project = tempfile::tempdir().unwrap();
        let nested = project.path().join("nested");
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(home.path().join(".astro")).unwrap();
        fs::create_dir_all(project.path().join(".git")).unwrap();
        fs::create_dir_all(project.path().join(".astro/agents")).unwrap();
        fs::create_dir_all(nested.join(".astro/agents")).unwrap();
        fs::write(
            home.path().join(".astro/config.toml"),
            format!(
                "[projects.{}]\ntrust_level = 'trusted'\n",
                toml_key(project.path())
            ),
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/config.toml"),
            "model = 'root'\n",
        )
        .unwrap();
        fs::write(nested.join(".astro/config.toml"), "model = 'nested'\n").unwrap();
        fs::write(
            project.path().join(".astro/agents/reviewer.toml"),
            "name = 'reviewer'\ndescription = 'root'\ndeveloper_instructions = 'root'\n",
        )
        .unwrap();
        fs::write(
            nested.join(".astro/agents/reviewer.toml"),
            "name = 'reviewer'\ndescription = 'nested'\ndeveloper_instructions = 'nested'\n",
        )
        .unwrap();

        let configuration = load_agent_configuration(&memory, Some(&nested)).unwrap();

        assert_eq!(
            configuration.catalog.agents["reviewer"].description,
            "nested"
        );
    }

    #[test]
    fn declared_role_resolves_relative_config_file_from_declaring_layer() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(home.path().join(".astro")).unwrap();
        fs::create_dir_all(project.path().join(".git")).unwrap();
        fs::create_dir_all(project.path().join(".astro/roles")).unwrap();
        fs::write(
            home.path().join(".astro/config.toml"),
            format!(
                "[projects.{}]\ntrust_level = 'trusted'\n",
                toml_key(project.path())
            ),
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/config.toml"),
            r#"[agents.reviewer]
description = "inline review"
config_file = "roles/reviewer.toml"
nickname_candidates = ["Ada", "Grace"]
"#,
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/roles/reviewer.toml"),
            r#"developer_instructions = "Review the patch."
model = "openai:gpt-5.6"

[[skills.config]]
path = "skills/review/SKILL.md"
"#,
        )
        .unwrap();

        let configuration = load_agent_configuration(&memory, Some(project.path())).unwrap();
        let reviewer = &configuration.catalog.agents["reviewer"];

        assert_eq!(reviewer.description, "inline review");
        assert_eq!(reviewer.nickname_candidates, ["Ada", "Grace"]);
        assert_eq!(reviewer.model.as_deref(), Some("openai:gpt-5.6"));
        assert_eq!(
            reviewer.skills.config[0].path,
            project
                .path()
                .canonicalize()
                .unwrap()
                .join(".astro/roles/skills/review/SKILL.md")
        );
    }

    #[test]
    fn declared_role_file_identity_fields_override_inline_fallbacks() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(home.path().join(".astro/roles")).unwrap();
        fs::write(
            home.path().join(".astro/config.toml"),
            r#"[agents.reviewer]
description = "inline"
config_file = "roles/reviewer.toml"
nickname_candidates = ["Inline"]
"#,
        )
        .unwrap();
        fs::write(
            home.path().join(".astro/roles/reviewer.toml"),
            r#"name = "critic"
description = "file description"
nickname_candidates = ["File"]
developer_instructions = "Critique carefully."
"#,
        )
        .unwrap();

        let configuration = load_agent_configuration(&memory, None).unwrap();
        let critic = &configuration.catalog.agents["critic"];

        assert!(!configuration.catalog.agents.contains_key("reviewer"));
        assert_eq!(critic.description, "file description");
        assert_eq!(critic.nickname_candidates, ["File"]);
    }

    #[test]
    fn higher_project_standalone_overrides_lower_user_declaration() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(home.path().join(".astro/roles")).unwrap();
        fs::create_dir_all(project.path().join(".git")).unwrap();
        fs::create_dir_all(project.path().join(".astro/agents")).unwrap();
        fs::write(
            home.path().join(".astro/config.toml"),
            format!(
                r#"[projects.{}]
trust_level = "trusted"

[agents.reviewer]
description = "user declaration"
config_file = "roles/reviewer.toml"
"#,
                toml_key(project.path())
            ),
        )
        .unwrap();
        fs::write(
            home.path().join(".astro/roles/reviewer.toml"),
            "developer_instructions = 'user declaration'\n",
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/agents/reviewer.toml"),
            "name = 'reviewer'\ndescription = 'project standalone'\ndeveloper_instructions = 'project standalone'\n",
        )
        .unwrap();

        let configuration = load_agent_configuration(&memory, Some(project.path())).unwrap();

        assert_eq!(
            configuration.catalog.agents["reviewer"].description,
            "project standalone"
        );
    }

    #[test]
    fn same_layer_declaration_overrides_standalone_agent() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(home.path().join(".astro")).unwrap();
        fs::create_dir_all(project.path().join(".git")).unwrap();
        fs::create_dir_all(project.path().join(".astro/agents")).unwrap();
        fs::create_dir_all(project.path().join(".astro/roles")).unwrap();
        fs::write(
            home.path().join(".astro/config.toml"),
            format!(
                "[projects.{}]\ntrust_level = 'trusted'\n",
                toml_key(project.path())
            ),
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/config.toml"),
            r#"[agents.reviewer]
description = "declared"
config_file = "roles/reviewer.toml"
"#,
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/agents/reviewer.toml"),
            "name = 'reviewer'\ndescription = 'standalone'\ndeveloper_instructions = 'standalone'\n",
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/roles/reviewer.toml"),
            "developer_instructions = 'declared'\n",
        )
        .unwrap();

        let configuration = load_agent_configuration(&memory, Some(project.path())).unwrap();

        assert_eq!(
            configuration.catalog.agents["reviewer"].description,
            "declared"
        );
        assert_eq!(
            configuration.catalog.agents["reviewer"].developer_instructions,
            "declared"
        );
    }

    #[test]
    fn higher_role_layer_inherits_missing_metadata_from_lower_role() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(home.path().join(".astro/agents")).unwrap();
        fs::create_dir_all(project.path().join(".git")).unwrap();
        fs::create_dir_all(project.path().join(".astro/roles")).unwrap();
        fs::write(
            home.path().join(".astro/config.toml"),
            format!(
                "[projects.{}]\ntrust_level = 'trusted'\n",
                toml_key(project.path())
            ),
        )
        .unwrap();
        fs::write(
            home.path().join(".astro/agents/reviewer.toml"),
            r#"name = "reviewer"
description = "personal description"
nickname_candidates = ["Ada"]
developer_instructions = "personal instructions"
"#,
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/config.toml"),
            r#"[agents.reviewer]
config_file = "roles/reviewer.toml"
"#,
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/roles/reviewer.toml"),
            "developer_instructions = 'project instructions'\n",
        )
        .unwrap();

        let configuration = load_agent_configuration(&memory, Some(project.path())).unwrap();
        let reviewer = &configuration.catalog.agents["reviewer"];

        assert_eq!(reviewer.description, "personal description");
        assert_eq!(reviewer.nickname_candidates, ["Ada"]);
        assert_eq!(reviewer.developer_instructions, "project instructions");
    }

    #[test]
    fn declared_file_inside_agents_directory_is_not_parsed_as_standalone() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(home.path().join(".astro/agents")).unwrap();
        fs::write(
            home.path().join(".astro/config.toml"),
            r#"[agents.reviewer]
description = "declared metadata"
config_file = "agents/reviewer.toml"
"#,
        )
        .unwrap();
        fs::write(
            home.path().join(".astro/agents/reviewer.toml"),
            "developer_instructions = 'review carefully'\n",
        )
        .unwrap();

        let configuration = load_agent_configuration(&memory, None).unwrap();

        assert!(configuration.catalog.diagnostics.is_empty());
        assert_eq!(
            configuration.catalog.agents["reviewer"].description,
            "declared metadata"
        );
    }

    #[test]
    fn malformed_role_metadata_is_diagnosed_without_overriding_lower_role() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(home.path().join(".astro/agents")).unwrap();
        fs::create_dir_all(project.path().join(".git")).unwrap();
        fs::create_dir_all(project.path().join(".astro/roles")).unwrap();
        fs::write(
            home.path().join(".astro/config.toml"),
            format!(
                "[projects.{}]\ntrust_level = 'trusted'\n",
                toml_key(project.path())
            ),
        )
        .unwrap();
        fs::write(
            home.path().join(".astro/agents/reviewer.toml"),
            "name = 'reviewer'\ndescription = 'personal'\ndeveloper_instructions = 'personal'\n",
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/config.toml"),
            r#"[agents.reviewer]
description = "project"
config_file = "roles/reviewer.toml"
nickname_candidates = ["Ada", " Ada "]
"#,
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/roles/reviewer.toml"),
            "developer_instructions = 'project'\n",
        )
        .unwrap();

        let configuration = load_agent_configuration(&memory, Some(project.path())).unwrap();

        assert_eq!(
            configuration.catalog.agents["reviewer"].description,
            "personal"
        );
        assert!(configuration
            .catalog
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("cannot contain duplicates")));
    }
}
