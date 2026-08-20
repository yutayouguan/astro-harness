//! Filesystem discovery for Codex-compatible local configuration layers.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml::Value as TomlValue;

use crate::{
    ConfigLayerEntry, ConfigLayerError, ConfigLayerSource, ConfigLayerStack, EffectiveConfig,
};

pub const CONFIG_TOML_FILE: &str = "config.toml";
pub const DOT_ASTRO_DIR: &str = ".astro";

/// Machine-local keys that project configuration must not override.
///
/// This list follows the public Codex project-config contract. These values
/// redirect credentials, provider traffic, host metadata, notifications, or
/// telemetry and therefore belong to user/system layers only.
pub const PROJECT_PROTECTED_KEYS: &[&str] = &[
    "openai_base_url",
    "chatgpt_base_url",
    "apps_mcp_product_sku",
    "responses_api_metadata",
    "model_provider",
    "model_providers",
    "notify",
    "profile",
    "profiles",
    "experimental_realtime_webrtc_call_base_url",
    "experimental_realtime_ws_base_url",
    "otel",
];

#[derive(Debug, Clone, PartialEq)]
pub struct LocalConfigOptions {
    pub astro_home: PathBuf,
    pub cwd: PathBuf,
    pub system_config: Option<PathBuf>,
    pub profile: Option<String>,
    pub packaged_defaults: Option<(PathBuf, TomlValue)>,
    pub session_overrides: Option<TomlValue>,
    pub request_overrides: Option<TomlValue>,
}

impl LocalConfigOptions {
    pub fn new(astro_home: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            astro_home: astro_home.into(),
            cwd: cwd.into(),
            system_config: default_system_config_path(),
            profile: None,
            packaged_defaults: None,
            session_overrides: None,
            request_overrides: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTrust {
    Trusted,
    Untrusted,
    Unknown,
}

impl ProjectTrust {
    fn disabled_reason(self) -> Option<&'static str> {
        match self {
            Self::Trusted => None,
            Self::Untrusted => Some("project is untrusted"),
            Self::Unknown => Some("project trust has not been granted"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigDiagnosticCode {
    ProjectLayerDisabled,
    ProjectKeyIgnored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigDiagnostic {
    pub code: ConfigDiagnosticCode,
    pub source: ConfigLayerSource,
    pub key: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalConfigLoad {
    pub layers: ConfigLayerStack,
    pub diagnostics: Vec<ConfigDiagnostic>,
    pub project_root: PathBuf,
    pub project_trust: ProjectTrust,
}

impl LocalConfigLoad {
    /// Freeze the loaded layers into the immutable snapshot consumed by one
    /// request, turn, or long-lived subsystem configuration generation.
    pub fn resolve(&self) -> EffectiveConfig {
        self.layers.resolve()
    }
}

#[derive(Debug, Error)]
pub enum LocalConfigError {
    #[error("configuration working directory is not a directory: {0}")]
    InvalidWorkingDirectory(PathBuf),
    #[error("failed to canonicalize {path}: {source}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to read configuration {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    InvalidLayer(#[from] Box<ConfigLayerError>),
    #[error("invalid profile name {0:?}; use only letters, numbers, '-' and '_'")]
    InvalidProfileName(String),
    #[error("selected profile does not exist: {0}")]
    MissingProfile(PathBuf),
    #[error(
        "selected profile {profile:?} conflicts with legacy profile configuration in {user_config}"
    )]
    LegacyProfileConflict {
        profile: String,
        user_config: PathBuf,
    },
    #[error("project_root_markers must be an array of strings")]
    InvalidProjectRootMarkers,
}

/// Loads local Codex-compatible configuration without interpreting
/// product-specific fields.
///
/// Layer order is defaults < system < user < profile < project(root-to-cwd)
/// < session < request. Project files are discovered but never read unless the
/// project root is trusted.
pub fn load_local_config(
    options: &LocalConfigOptions,
) -> Result<LocalConfigLoad, LocalConfigError> {
    if !options.cwd.is_dir() {
        return Err(LocalConfigError::InvalidWorkingDirectory(
            options.cwd.clone(),
        ));
    }
    let cwd = canonicalize(&options.cwd)?;
    let mut layers = Vec::new();
    let mut discovery_layers = Vec::new();

    if let Some((path, config)) = &options.packaged_defaults {
        layers.push(ConfigLayerEntry::from_value(
            ConfigLayerSource::PackagedDefaults { file: path.clone() },
            config.clone(),
        ));
    }

    if let Some(path) = options.system_config.as_deref() {
        if let Some(layer) = load_optional_file(
            path,
            ConfigLayerSource::System {
                file: path.to_path_buf(),
            },
        )? {
            discovery_layers.push(layer.clone());
            layers.push(layer);
        }
    }

    let user_path = options.astro_home.join(CONFIG_TOML_FILE);
    if let Some(layer) = load_optional_file(
        &user_path,
        ConfigLayerSource::User {
            file: user_path.clone(),
        },
    )? {
        discovery_layers.push(layer.clone());
        layers.push(layer);
    }

    // Project discovery and trust are machine/user decisions. A selected
    // profile cannot redirect project-root discovery or grant repository trust.
    let discovery_config = ConfigLayerStack::new(discovery_layers).effective_config();
    let root_markers = project_root_markers(&discovery_config)?;
    let project_root = discover_project_root(&cwd, &root_markers);
    let project_trust = resolve_project_trust(&discovery_config, &project_root);

    if let Some(profile) = options.profile.as_deref() {
        validate_profile_name(profile)?;
        reject_legacy_profile_conflict(&discovery_config, profile, &user_path)?;
        let path = options.astro_home.join(format!("{profile}.config.toml"));
        if !path.is_file() {
            return Err(LocalConfigError::MissingProfile(path));
        }
        layers.push(load_file(
            &path,
            ConfigLayerSource::Profile {
                name: profile.to_string(),
                file: path.clone(),
            },
        )?);
    }

    let mut diagnostics = Vec::new();

    for dot_codex_dir in project_config_dirs(&project_root, &cwd) {
        let path = dot_codex_dir.join(CONFIG_TOML_FILE);
        if !path.is_file() {
            continue;
        }
        let source = ConfigLayerSource::Project {
            dot_config_dir: dot_codex_dir,
        };
        if let Some(reason) = project_trust.disabled_reason() {
            // Do not parse or otherwise consume untrusted repository content.
            layers.push(
                ConfigLayerEntry::from_value(source.clone(), TomlValue::Table(Default::default()))
                    .disabled(reason),
            );
            diagnostics.push(ConfigDiagnostic {
                code: ConfigDiagnosticCode::ProjectLayerDisabled,
                source,
                key: None,
                message: reason.to_string(),
            });
            continue;
        }

        let mut layer = load_file(&path, source.clone())?;
        strip_project_protected_keys(&mut layer, &mut diagnostics);
        layers.push(layer);
    }

    if let Some(config) = &options.session_overrides {
        layers.push(ConfigLayerEntry::from_value(
            ConfigLayerSource::SessionOverrides,
            config.clone(),
        ));
    }
    if let Some(config) = &options.request_overrides {
        layers.push(ConfigLayerEntry::from_value(
            ConfigLayerSource::RequestOverrides,
            config.clone(),
        ));
    }

    Ok(LocalConfigLoad {
        layers: ConfigLayerStack::new(layers),
        diagnostics,
        project_root,
        project_trust,
    })
}

pub fn validate_profile_name(profile: &str) -> Result<(), LocalConfigError> {
    if profile.is_empty()
        || !profile
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(LocalConfigError::InvalidProfileName(profile.to_string()));
    }
    Ok(())
}

fn default_system_config_path() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        Some(PathBuf::from("/etc/astro/config.toml"))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn canonicalize(path: &Path) -> Result<PathBuf, LocalConfigError> {
    path.canonicalize()
        .map_err(|source| LocalConfigError::Canonicalize {
            path: path.to_path_buf(),
            source,
        })
}

fn load_optional_file(
    path: &Path,
    source: ConfigLayerSource,
) -> Result<Option<ConfigLayerEntry>, LocalConfigError> {
    if path.is_file() {
        return load_file(path, source).map(Some);
    }
    Ok(None)
}

fn load_file(path: &Path, source: ConfigLayerSource) -> Result<ConfigLayerEntry, LocalConfigError> {
    let raw_toml = fs::read_to_string(path).map_err(|source| LocalConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    ConfigLayerEntry::parse(source, &raw_toml).map_err(|error| Box::new(error).into())
}

fn project_root_markers(config: &TomlValue) -> Result<Vec<String>, LocalConfigError> {
    let Some(value) = config.get("project_root_markers") else {
        return Ok(vec![".git".to_string()]);
    };
    let Some(values) = value.as_array() else {
        return Err(LocalConfigError::InvalidProjectRootMarkers);
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or(LocalConfigError::InvalidProjectRootMarkers)
        })
        .collect()
}

fn discover_project_root(cwd: &Path, root_markers: &[String]) -> PathBuf {
    if root_markers.is_empty() {
        return cwd.to_path_buf();
    }
    cwd.ancestors()
        .find(|directory| {
            root_markers
                .iter()
                .any(|marker| directory.join(marker).exists())
        })
        .unwrap_or(cwd)
        .to_path_buf()
}

fn project_config_dirs(project_root: &Path, cwd: &Path) -> Vec<PathBuf> {
    let mut directories = cwd
        .ancestors()
        .take_while(|directory| directory.starts_with(project_root))
        .map(|directory| directory.join(DOT_ASTRO_DIR))
        .collect::<Vec<_>>();
    directories.reverse();
    directories
}

fn resolve_project_trust(config: &TomlValue, project_root: &Path) -> ProjectTrust {
    let Some(projects) = config.get("projects").and_then(TomlValue::as_table) else {
        return ProjectTrust::Unknown;
    };
    let mut saw_trusted = false;
    let mut saw_untrusted = false;
    for (configured_path, project) in projects {
        let path = Path::new(configured_path);
        if !path.is_absolute()
            || path
                .canonicalize()
                .map_or(true, |configured| configured != project_root)
        {
            continue;
        }
        match project
            .get("trust_level")
            .and_then(TomlValue::as_str)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("trusted") => saw_trusted = true,
            Some("untrusted") => saw_untrusted = true,
            _ => {}
        }
    }
    if saw_untrusted {
        ProjectTrust::Untrusted
    } else if saw_trusted {
        ProjectTrust::Trusted
    } else {
        ProjectTrust::Unknown
    }
}

fn strip_project_protected_keys(
    layer: &mut ConfigLayerEntry,
    diagnostics: &mut Vec<ConfigDiagnostic>,
) {
    let Some(table) = layer.config.as_table_mut() else {
        return;
    };
    for key in PROJECT_PROTECTED_KEYS {
        if table.remove(*key).is_none() {
            continue;
        }
        diagnostics.push(ConfigDiagnostic {
            code: ConfigDiagnosticCode::ProjectKeyIgnored,
            source: layer.source.clone(),
            key: Some((*key).to_string()),
            message: format!("project configuration cannot override `{key}`"),
        });
    }
    if let Some(features) = table.get_mut("features").and_then(TomlValue::as_table_mut) {
        if features.remove("respect_system_proxy").is_some() {
            diagnostics.push(ConfigDiagnostic {
                code: ConfigDiagnosticCode::ProjectKeyIgnored,
                source: layer.source.clone(),
                key: Some("features.respect_system_proxy".to_string()),
                message: "project configuration cannot override `features.respect_system_proxy`"
                    .to_string(),
            });
        }
    }
}

fn reject_legacy_profile_conflict(
    user_config: &TomlValue,
    profile: &str,
    user_path: &Path,
) -> Result<(), LocalConfigError> {
    let legacy_selector_matches = user_config
        .get("profile")
        .and_then(TomlValue::as_str)
        .is_some_and(|selected| selected == profile);
    let legacy_table_matches = user_config
        .get("profiles")
        .and_then(TomlValue::as_table)
        .is_some_and(|profiles| profiles.contains_key(profile));
    if legacy_selector_matches || legacy_table_matches {
        return Err(LocalConfigError::LegacyProfileConflict {
            profile: profile.to_string(),
            user_config: user_path.to_path_buf(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::ConfigKeyPath;

    struct Fixture {
        _root: TempDir,
        home: PathBuf,
        project: PathBuf,
        nested: PathBuf,
    }

    impl Fixture {
        fn new(trust_level: Option<&str>) -> Self {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("home/.astro");
            let project = root.path().join("repo");
            let nested = project.join("packages/app");
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(project.join(".git")).unwrap();
            fs::create_dir_all(&nested).unwrap();
            let mut user = "model = 'user'\nmodel_provider = 'user-provider'\n".to_string();
            if let Some(trust_level) = trust_level {
                user.push_str(&format!(
                    "[projects.{}]\ntrust_level = {:?}\n",
                    toml_key(&project),
                    trust_level
                ));
            }
            fs::write(home.join(CONFIG_TOML_FILE), user).unwrap();
            Self {
                _root: root,
                home,
                project,
                nested,
            }
        }

        fn options(&self) -> LocalConfigOptions {
            let mut options = LocalConfigOptions::new(&self.home, &self.nested);
            options.system_config = None;
            options
        }
    }

    fn toml_key(path: &Path) -> String {
        format!("{:?}", path.to_string_lossy())
    }

    fn write_project_config(directory: &Path, raw: &str) {
        let dot_astro = directory.join(DOT_ASTRO_DIR);
        fs::create_dir_all(&dot_astro).unwrap();
        fs::write(dot_astro.join(CONFIG_TOML_FILE), raw).unwrap();
    }

    #[test]
    fn loads_profile_and_project_chain_in_official_precedence_order() {
        let fixture = Fixture::new(Some("trusted"));
        fs::write(
            fixture.home.join("review.config.toml"),
            "model = 'profile'\n",
        )
        .unwrap();
        write_project_config(&fixture.project, "model = 'root-project'\n");
        write_project_config(&fixture.nested, "model = 'closest-project'\n");
        let mut options = fixture.options();
        options.profile = Some("review".to_string());

        let loaded = load_local_config(&options).unwrap();
        let effective = loaded.resolve();
        assert_eq!(loaded.project_trust, ProjectTrust::Trusted);
        assert_eq!(effective.raw()["model"].as_str(), Some("closest-project"));
        assert!(matches!(
            effective.origin_at(["model"].iter()).unwrap().source,
            ConfigLayerSource::Project { ref dot_config_dir }
                if dot_config_dir
                    == &fixture.nested.canonicalize().unwrap().join(DOT_ASTRO_DIR)
        ));
    }

    #[test]
    fn session_and_request_overrides_beat_project_layers() {
        let fixture = Fixture::new(Some("trusted"));
        write_project_config(&fixture.project, "model = 'project'\n");
        let mut options = fixture.options();
        options.session_overrides = Some("model = 'session'".parse().unwrap());
        options.request_overrides = Some("model = 'request'".parse().unwrap());

        let loaded = load_local_config(&options).unwrap();
        assert_eq!(
            loaded.layers.effective_config()["model"].as_str(),
            Some("request")
        );
        assert!(matches!(
            loaded.layers.origin_at(["model"].iter()).unwrap().source,
            ConfigLayerSource::RequestOverrides
        ));
    }

    #[test]
    fn untrusted_project_content_is_not_parsed_or_applied() {
        let fixture = Fixture::new(None);
        write_project_config(&fixture.project, "this is not valid TOML = [");

        let loaded = load_local_config(&fixture.options()).unwrap();
        assert_eq!(loaded.project_trust, ProjectTrust::Unknown);
        assert_eq!(
            loaded.layers.effective_config()["model"].as_str(),
            Some("user")
        );
        assert_eq!(
            loaded
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.code == ConfigDiagnosticCode::ProjectLayerDisabled
                })
                .count(),
            1
        );
        assert!(loaded
            .layers
            .layers_high_to_low()
            .any(|layer| layer.disabled_reason.is_some()));
    }

    #[test]
    fn trusted_project_cannot_override_machine_local_keys() {
        let fixture = Fixture::new(Some("trusted"));
        write_project_config(
            &fixture.project,
            concat!(
                "model = 'project'\n",
                "model_provider = 'attacker'\n",
                "responses_api_metadata = { leaked = 'yes' }\n",
                "notify = ['bad']\n",
                "[features]\nrespect_system_proxy = true\n",
            ),
        );

        let loaded = load_local_config(&fixture.options()).unwrap();
        let effective = loaded.layers.effective_config();
        assert_eq!(effective["model"].as_str(), Some("project"));
        assert_eq!(effective["model_provider"].as_str(), Some("user-provider"));
        assert!(effective.get("notify").is_none());
        assert!(effective.get("responses_api_metadata").is_none());
        assert!(effective
            .get("features")
            .and_then(TomlValue::as_table)
            .is_none_or(|features| !features.contains_key("respect_system_proxy")));
        assert_eq!(
            loaded
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == ConfigDiagnosticCode::ProjectKeyIgnored)
                .count(),
            4
        );
        assert!(matches!(
            loaded
                .layers
                .origins()
                .get(&ConfigKeyPath::from_segments(["model_provider"]))
                .unwrap()
                .source,
            ConfigLayerSource::User { .. }
        ));
    }

    #[test]
    fn empty_root_markers_treat_current_directory_as_project_root() {
        let fixture = Fixture::new(None);
        let raw = format!(
            "project_root_markers = []\n[projects.{}]\ntrust_level = 'trusted'\n",
            toml_key(&fixture.nested)
        );
        fs::write(fixture.home.join(CONFIG_TOML_FILE), raw).unwrap();
        write_project_config(&fixture.nested, "model = 'nested'\n");

        let loaded = load_local_config(&fixture.options()).unwrap();
        assert_eq!(loaded.project_root, fixture.nested.canonicalize().unwrap());
        assert_eq!(loaded.project_trust, ProjectTrust::Trusted);
        assert_eq!(
            loaded.layers.effective_config()["model"].as_str(),
            Some("nested")
        );
    }

    #[test]
    fn profile_cannot_grant_project_trust_or_change_root_discovery() {
        let fixture = Fixture::new(None);
        let profile = format!(
            "project_root_markers = []\n[projects.{}]\ntrust_level = 'trusted'\n",
            toml_key(&fixture.nested)
        );
        fs::write(fixture.home.join("review.config.toml"), profile).unwrap();
        write_project_config(&fixture.project, "model = 'must-not-load'\n");
        let mut options = fixture.options();
        options.profile = Some("review".to_string());

        let loaded = load_local_config(&options).unwrap();
        assert_eq!(loaded.project_root, fixture.project.canonicalize().unwrap());
        assert_eq!(loaded.project_trust, ProjectTrust::Unknown);
        assert_eq!(
            loaded.layers.effective_config()["model"].as_str(),
            Some("user")
        );
    }

    #[test]
    fn empty_string_root_marker_matches_the_current_directory() {
        let fixture = Fixture::new(None);
        let raw = format!(
            "project_root_markers = ['']\n[projects.{}]\ntrust_level = 'trusted'\n",
            toml_key(&fixture.nested)
        );
        fs::write(fixture.home.join(CONFIG_TOML_FILE), raw).unwrap();
        write_project_config(&fixture.nested, "model = 'nested'\n");

        let loaded = load_local_config(&fixture.options()).unwrap();
        assert_eq!(loaded.project_root, fixture.nested.canonicalize().unwrap());
        assert_eq!(loaded.project_trust, ProjectTrust::Trusted);
    }

    #[test]
    fn invalid_or_missing_profile_fails_closed() {
        let fixture = Fixture::new(Some("trusted"));
        for profile in ["../escape", "contains space", ""] {
            let mut options = fixture.options();
            options.profile = Some(profile.to_string());
            assert!(matches!(
                load_local_config(&options),
                Err(LocalConfigError::InvalidProfileName(_))
            ));
        }

        let mut options = fixture.options();
        options.profile = Some("missing".to_string());
        assert!(matches!(
            load_local_config(&options),
            Err(LocalConfigError::MissingProfile(_))
        ));
    }

    #[test]
    fn selected_v2_profile_rejects_matching_legacy_profile_config() {
        let fixture = Fixture::new(Some("trusted"));
        fs::write(
            fixture.home.join(CONFIG_TOML_FILE),
            "profile = 'review'\n[profiles.review]\nmodel = 'legacy'\n",
        )
        .unwrap();
        fs::write(
            fixture.home.join("review.config.toml"),
            "model = 'current'\n",
        )
        .unwrap();
        let mut options = fixture.options();
        options.profile = Some("review".to_string());

        assert!(matches!(
            load_local_config(&options),
            Err(LocalConfigError::LegacyProfileConflict { .. })
        ));
    }

    #[test]
    fn invalid_root_marker_shape_is_rejected_before_project_discovery() {
        let fixture = Fixture::new(None);
        fs::write(
            fixture.home.join(CONFIG_TOML_FILE),
            "project_root_markers = '.git'\n",
        )
        .unwrap();

        assert!(matches!(
            load_local_config(&fixture.options()),
            Err(LocalConfigError::InvalidProjectRootMarkers)
        ));
    }
}
