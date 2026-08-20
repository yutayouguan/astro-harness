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
    _roles: BTreeMap<String, AgentRoleSettings>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AgentRoleSettings {
    #[serde(rename = "description")]
    _description: Option<String>,
    #[serde(rename = "config_file")]
    _config_file: Option<PathBuf>,
    #[serde(rename = "nickname_candidates")]
    _nickname_candidates: Option<Vec<String>>,
}

impl AgentsSettings {
    fn apply(&mut self, partial: PartialAgentsSettings) {
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
            self.default_subagent_model = partial.default_subagent_model;
        }
        if partial.default_subagent_reasoning_effort.is_some() {
            self.default_subagent_reasoning_effort = partial.default_subagent_reasoning_effort;
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
    let codex_home = codex_home(memory_dir);
    let cwd = project_root.unwrap_or(memory_dir);
    let options = LocalConfigOptions::new(&codex_home, cwd);
    let loaded = load_local_config(&options).with_context(|| {
        format!(
            "failed to load Codex configuration for agent settings from {}",
            codex_home.display()
        )
    })?;
    let effective = loaded.resolve();
    let settings = agents_settings_from_effective(&effective)?;

    let mut catalog = AgentCatalog::default();
    for definition in builtin_agents() {
        catalog.agents.insert(definition.name.clone(), definition);
    }
    let mut agent_dirs = vec![codex_home.join("agents")];
    for layer in effective.layers_low_to_high() {
        let candidate = match &layer.source {
            ConfigLayerSource::User { file } | ConfigLayerSource::Profile { file, .. } => {
                file.parent().map(|parent| parent.join("agents"))
            }
            ConfigLayerSource::Project { dot_config_dir } => Some(dot_config_dir.join("agents")),
            _ => None,
        };
        if let Some(candidate) = candidate {
            if !agent_dirs.contains(&candidate) {
                agent_dirs.push(candidate);
            }
        }
    }
    if loaded.project_trust == ProjectTrust::Trusted {
        let standalone_project_dir = loaded.project_root.join(".codex/agents");
        if !agent_dirs.contains(&standalone_project_dir) {
            agent_dirs.push(standalone_project_dir);
        }
    }
    for directory in agent_dirs {
        load_agent_dir(&directory, &mut catalog);
    }

    Ok(AgentConfiguration {
        settings,
        catalog,
        effective_config_version: effective.version().to_string(),
    })
}

pub fn agents_settings_from_effective(
    effective: &EffectiveConfig,
) -> anyhow::Result<AgentsSettings> {
    let root = effective
        .decode::<RootConfig>()
        .context("invalid effective [agents] configuration")?;
    let mut settings = AgentsSettings::default();
    if let Some(partial) = root.agents {
        settings.apply(partial);
    }
    Ok(settings)
}

fn codex_home(memory_dir: &Path) -> PathBuf {
    if memory_dir.file_name().and_then(|name| name.to_str()) == Some(".astro") {
        return memory_dir.parent().unwrap_or(memory_dir).join(".codex");
    }
    memory_dir.join(".codex")
}

fn load_agent_dir(dir: &Path, catalog: &mut AgentCatalog) {
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
        let parsed = fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|text| toml::from_str::<AgentDefinition>(&text).map_err(Into::into));
        match parsed {
            Ok(mut agent)
                if !agent.name.trim().is_empty()
                    && !agent.description.trim().is_empty()
                    && !agent.developer_instructions.trim().is_empty() =>
            {
                if let Some(parent) = path.parent() {
                    for skill in &mut agent.skills.config {
                        if skill.path.is_relative() {
                            skill.path = parent.join(&skill.path);
                        }
                    }
                }
                catalog.agents.insert(agent.name.clone(), agent);
            }
            Ok(_) => catalog.diagnostics.push(AgentConfigDiagnostic {
                path,
                message: "name, description and developer_instructions are required".into(),
            }),
            Err(error) => catalog.diagnostics.push(AgentConfigDiagnostic {
                path,
                message: error.to_string(),
            }),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn toml_key(path: &Path) -> String {
        format!("{:?}", path.to_string_lossy())
    }

    #[test]
    fn astro_agent_directories_are_not_configuration_inputs() {
        let memory = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(memory.path().join("agents")).unwrap();
        fs::create_dir_all(project.path().join(".astro/agents")).unwrap();
        fs::write(
            memory.path().join("agents/legacy.toml"),
            "name = \"legacy\"\ndescription = \"legacy\"\ndeveloper_instructions = \"legacy\"\n",
        )
        .unwrap();
        fs::write(
            project.path().join(".astro/agents/project_legacy.toml"),
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
    fn project_astro_config_toml_is_not_a_configuration_input() {
        let memory = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(project.path().join(".astro")).unwrap();
        fs::write(
            project.path().join(".astro/config.toml"),
            "[agents]\nenabled = false\n",
        )
        .unwrap();

        assert!(
            load_agent_configuration(memory.path(), Some(project.path()))
                .unwrap()
                .settings
                .enabled
        );
    }

    #[test]
    fn codex_settings_load_from_home_and_project_overrides_personal() {
        let home = tempfile::tempdir().unwrap();
        let memory = home.path().join(".astro");
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(home.path().join(".codex")).unwrap();
        fs::create_dir_all(project.path().join(".codex/agents")).unwrap();
        fs::write(
            home.path().join(".codex/config.toml"),
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
            project.path().join(".codex/config.toml"),
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
        fs::create_dir_all(home.path().join(".codex")).unwrap();
        fs::create_dir_all(project.path().join(".codex/agents")).unwrap();
        fs::write(
            home.path().join(".codex/config.toml"),
            "[agents]\nenabled = true\nmax_concurrent_threads_per_session = 5\n",
        )
        .unwrap();
        fs::write(
            project.path().join(".codex/config.toml"),
            "[agents]\nenabled = false\nmax_concurrent_threads_per_session = 11\n",
        )
        .unwrap();
        fs::write(
            project.path().join(".codex/agents/untrusted.toml"),
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
        fs::create_dir_all(home.path().join(".codex")).unwrap();
        fs::write(
            home.path().join(".codex/config.toml"),
            "[agents]\nmax_concurrent_threads_per_session = 'many'\n",
        )
        .unwrap();

        let error = load_agent_configuration(&memory, None).unwrap_err();

        assert!(error.to_string().contains("invalid effective [agents]"));
    }

    #[test]
    fn custom_agent_overrides_builtin_and_model_precedence() {
        let root = tempfile::tempdir().unwrap();
        let agents = root.path().join(".codex/agents");
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
        let agents = root.path().join(".codex/agents");
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
        fs::create_dir_all(memory.path().join(".codex/agents")).unwrap();
        fs::create_dir_all(project.path().join(".codex/agents")).unwrap();
        fs::write(
            memory.path().join(".codex/config.toml"),
            format!(
                "[projects.{}]\ntrust_level = 'trusted'\n",
                toml_key(project.path())
            ),
        )
        .unwrap();
        fs::write(
            memory.path().join(".codex/agents/reviewer.toml"),
            r#"name = "reviewer"
description = "personal codex"
developer_instructions = "personal"
"#,
        )
        .unwrap();
        fs::write(
            project.path().join(".codex/agents/reviewer.toml"),
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
            project.path().join(".codex/config.toml"),
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
        fs::create_dir_all(home.path().join(".codex")).unwrap();
        fs::create_dir_all(project.path().join(".git")).unwrap();
        fs::create_dir_all(project.path().join(".codex/agents")).unwrap();
        fs::create_dir_all(nested.join(".codex/agents")).unwrap();
        fs::write(
            home.path().join(".codex/config.toml"),
            format!(
                "[projects.{}]\ntrust_level = 'trusted'\n",
                toml_key(project.path())
            ),
        )
        .unwrap();
        fs::write(
            project.path().join(".codex/config.toml"),
            "model = 'root'\n",
        )
        .unwrap();
        fs::write(nested.join(".codex/config.toml"), "model = 'nested'\n").unwrap();
        fs::write(
            project.path().join(".codex/agents/reviewer.toml"),
            "name = 'reviewer'\ndescription = 'root'\ndeveloper_instructions = 'root'\n",
        )
        .unwrap();
        fs::write(
            nested.join(".codex/agents/reviewer.toml"),
            "name = 'reviewer'\ndescription = 'nested'\ndeveloper_instructions = 'nested'\n",
        )
        .unwrap();

        let configuration = load_agent_configuration(&memory, Some(&nested)).unwrap();

        assert_eq!(
            configuration.catalog.agents["reviewer"].description,
            "nested"
        );
    }
}
