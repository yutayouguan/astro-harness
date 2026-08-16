use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub developer_instructions: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub model_reasoning_effort: Option<String>,
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

pub fn load_agents_settings(memory_dir: &Path, project_root: Option<&Path>) -> AgentsSettings {
    let mut settings = AgentsSettings::default();
    apply_settings_file(&mut settings, &memory_dir.join("config.toml"));
    if let Some(root) = project_root {
        apply_settings_file(&mut settings, &root.join(".astro/config.toml"));
    }
    settings
}

fn apply_settings_file(settings: &mut AgentsSettings, path: &Path) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    match toml::from_str::<RootConfig>(&text) {
        Ok(root) => {
            if let Some(partial) = root.agents {
                settings.apply(partial);
            }
        }
        Err(error) => tracing::warn!(path = %path.display(), %error, "invalid agents config"),
    }
}

pub fn load_agent_catalog(memory_dir: &Path, project_root: Option<&Path>) -> AgentCatalog {
    let mut catalog = AgentCatalog::default();
    for definition in builtin_agents() {
        catalog.agents.insert(definition.name.clone(), definition);
    }
    load_agent_dir(&memory_dir.join("agents"), &mut catalog);
    if let Some(root) = project_root {
        load_agent_dir(&root.join(".astro/agents"), &mut catalog);
    }
    catalog
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
            Ok(agent)
                if !agent.name.trim().is_empty()
                    && !agent.description.trim().is_empty()
                    && !agent.developer_instructions.trim().is_empty() =>
            {
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
        },
        AgentDefinition {
            name: "worker".into(),
            description: "Execution-focused agent for implementation and fixes.".into(),
            developer_instructions: "Own the implementation task end to end. Make focused changes, validate them, and report files changed plus verification results.".into(),
            model: None,
            model_reasoning_effort: None,
        },
        AgentDefinition {
            name: "explorer".into(),
            description: "Read-heavy codebase exploration agent.".into(),
            developer_instructions: "Stay read-only. Trace real code paths, gather evidence, and return concise findings with file and symbol references.".into(),
            model: None,
            model_reasoning_effort: None,
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
    // Codex subagents inherit the parent task's permission profile. Agent
    // definitions may specialize behavior and model selection, but cannot
    // elevate or replace sandbox permissions.
    let sandbox_mode = non_empty(parent_sandbox_mode);

    Ok(ResolvedAgent {
        definition,
        model,
        model_reasoning_effort,
        sandbox_mode,
    })
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

    #[test]
    fn custom_agent_overrides_builtin_and_model_precedence() {
        let root = tempfile::tempdir().unwrap();
        let agents = root.path().join("agents");
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
        let catalog = load_agent_catalog(root.path(), None);
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
    fn custom_agent_cannot_override_parent_permissions() {
        let root = tempfile::tempdir().unwrap();
        let agents = root.path().join("agents");
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
        let catalog = load_agent_catalog(root.path(), None);
        assert!(!catalog.agents.contains_key("unsafe"));
        assert_eq!(catalog.diagnostics.len(), 1);
    }
}
