//! Canonical layered configuration primitives for Astro.
//!
//! This crate deliberately contains no product-specific configuration fields
//! and performs no filesystem discovery. Loaders contribute ordered layers;
//! consumers receive one effective TOML value plus exact per-key provenance.

pub mod loader;

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use toml::Value as TomlValue;

/// Provenance for one configuration layer.
///
/// Higher precedence values override lower ones. Equal-precedence layers keep
/// insertion order, which lets project loaders append directories from the
/// repository root down to the current working directory (closest wins).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigLayerSource {
    PackagedDefaults { file: PathBuf },
    ManagedPreferences { domain: String, key: String },
    System { file: PathBuf },
    EnterpriseManaged { id: String, name: String },
    User { file: PathBuf },
    Profile { name: String, file: PathBuf },
    Project { dot_config_dir: PathBuf },
    Agent { agent_id: String, file: PathBuf },
    SessionOverrides,
    RequestOverrides,
}

impl ConfigLayerSource {
    pub const fn precedence(&self) -> i16 {
        match self {
            Self::PackagedDefaults { .. } => -10,
            Self::ManagedPreferences { .. } => 0,
            Self::System { .. } => 10,
            Self::EnterpriseManaged { .. } => 15,
            Self::User { .. } => 20,
            Self::Profile { .. } => 21,
            Self::Project { .. } => 25,
            Self::Agent { .. } => 27,
            Self::SessionOverrides => 30,
            Self::RequestOverrides => 35,
        }
    }
}

/// A lossless configuration key path. Segments are not joined internally, so
/// a literal key containing `.` cannot collide with a nested table path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ConfigKeyPath(Vec<String>);

impl ConfigKeyPath {
    pub fn root() -> Self {
        Self(Vec::new())
    }

    pub fn from_segments(segments: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self(segments.into_iter().map(Into::into).collect())
    }

    pub fn segments(&self) -> &[String] {
        &self.0
    }

    fn pushed(&self, segment: impl Into<String>) -> Self {
        let mut segments = self.0.clone();
        segments.push(segment.into());
        Self(segments)
    }

    fn starts_with(&self, parent: &Self) -> bool {
        self.0.starts_with(&parent.0)
    }
}

impl fmt::Display for ConfigKeyPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, segment) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(".")?;
            }
            if segment.contains(['.', '"']) {
                write!(formatter, "{:?}", segment)?;
            } else {
                formatter.write_str(segment)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigOrigin {
    pub source: ConfigLayerSource,
    pub version: String,
}

/// One enabled layer captured in an immutable effective-config snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveConfigLayer {
    pub source: ConfigLayerSource,
    pub version: String,
}

/// Immutable result of resolving a configuration layer stack.
///
/// Consumers should keep this snapshot for the lifetime of one request or
/// turn. That prevents configuration files from being re-read midway through
/// an operation and makes the exact effective version observable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectiveConfig {
    config: TomlValue,
    origins: BTreeMap<ConfigKeyPath, ConfigOrigin>,
    layers_low_to_high: Vec<EffectiveConfigLayer>,
    version: String,
}

impl EffectiveConfig {
    pub fn raw(&self) -> &TomlValue {
        &self.config
    }

    pub fn into_raw(self) -> TomlValue {
        self.config
    }

    pub fn origins(&self) -> &BTreeMap<ConfigKeyPath, ConfigOrigin> {
        &self.origins
    }

    pub fn origin_at<'a, I, S>(&self, segments: I) -> Option<&ConfigOrigin>
    where
        I: IntoIterator<Item = &'a S>,
        S: AsRef<str> + 'a + ?Sized,
    {
        let path = ConfigKeyPath::from_segments(
            segments
                .into_iter()
                .map(|segment| segment.as_ref().to_string()),
        );
        self.origins.get(&path)
    }

    pub fn layers_low_to_high(&self) -> &[EffectiveConfigLayer] {
        &self.layers_low_to_high
    }

    /// Stable fingerprint of the canonical effective value.
    ///
    /// Formatting changes and overridden lower-layer values do not change the
    /// version; a behaviorally different effective value does.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Decode this snapshot into a consumer-owned strong schema.
    ///
    /// Consumers can opt into strict field validation with
    /// `#[serde(deny_unknown_fields)]` without coupling this crate to every
    /// domain-specific configuration type.
    pub fn decode<T>(&self) -> Result<T, EffectiveConfigError>
    where
        T: DeserializeOwned,
    {
        self.config
            .clone()
            .try_into()
            .map_err(|source| EffectiveConfigError::Decode {
                target: std::any::type_name::<T>(),
                source: Box::new(source),
            })
    }
}

#[derive(Debug, Error)]
pub enum EffectiveConfigError {
    #[error("effective configuration is invalid for {target}: {source}")]
    Decode {
        target: &'static str,
        #[source]
        source: Box<toml::de::Error>,
    },
}

/// One parsed layer plus its stable raw-content fingerprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigLayerEntry {
    pub source: ConfigLayerSource,
    pub config: TomlValue,
    pub version: String,
    pub disabled_reason: Option<String>,
}

impl ConfigLayerEntry {
    pub fn parse(source: ConfigLayerSource, raw_toml: &str) -> Result<Self, ConfigLayerError> {
        let config = raw_toml.parse::<TomlValue>().map_err(|source_error| {
            ConfigLayerError::InvalidToml {
                layer: source.clone(),
                source: Box::new(source_error),
            }
        })?;
        Ok(Self {
            source,
            config,
            version: fingerprint(raw_toml.as_bytes()),
            disabled_reason: None,
        })
    }

    pub fn from_value(source: ConfigLayerSource, config: TomlValue) -> Self {
        let canonical = canonical_toml_bytes(&config);
        Self {
            source,
            config,
            version: fingerprint(&canonical),
            disabled_reason: None,
        }
    }

    pub fn disabled(mut self, reason: impl Into<String>) -> Self {
        self.disabled_reason = Some(reason.into());
        self
    }

    pub fn is_enabled(&self) -> bool {
        self.disabled_reason.is_none()
    }
}

#[derive(Debug, Error)]
pub enum ConfigLayerError {
    #[error("invalid TOML in {layer:?}: {source}")]
    InvalidToml {
        layer: ConfigLayerSource,
        #[source]
        source: Box<toml::de::Error>,
    },
}

/// Materialized layers ordered from lowest to highest precedence.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigLayerStack {
    layers: Vec<ConfigLayerEntry>,
}

impl ConfigLayerStack {
    pub fn new(mut layers: Vec<ConfigLayerEntry>) -> Self {
        // `sort_by_key` is stable: equal-precedence project layers retain the
        // root-to-cwd ordering supplied by discovery.
        layers.sort_by_key(|layer| layer.source.precedence());
        Self { layers }
    }

    pub fn push(&mut self, layer: ConfigLayerEntry) {
        self.layers.push(layer);
        self.layers.sort_by_key(|entry| entry.source.precedence());
    }

    pub fn layers_low_to_high(&self) -> impl DoubleEndedIterator<Item = &ConfigLayerEntry> {
        self.layers.iter()
    }

    pub fn layers_high_to_low(&self) -> impl DoubleEndedIterator<Item = &ConfigLayerEntry> {
        self.layers.iter().rev()
    }

    pub fn effective_config(&self) -> TomlValue {
        self.resolve().into_raw()
    }

    pub fn origins(&self) -> BTreeMap<ConfigKeyPath, ConfigOrigin> {
        self.resolve().origins
    }

    /// Resolve all enabled layers once into an immutable snapshot.
    pub fn resolve(&self) -> EffectiveConfig {
        let (config, origins) = self.materialize();
        let version = fingerprint(&canonical_toml_bytes(&config));
        let layers_low_to_high = self
            .layers
            .iter()
            .filter(|layer| layer.is_enabled())
            .map(|layer| EffectiveConfigLayer {
                source: layer.source.clone(),
                version: layer.version.clone(),
            })
            .collect();
        EffectiveConfig {
            config,
            origins,
            layers_low_to_high,
            version,
        }
    }

    pub fn origin_at<'a, I, S>(&self, segments: I) -> Option<ConfigOrigin>
    where
        I: IntoIterator<Item = &'a S>,
        S: AsRef<str> + 'a + ?Sized,
    {
        let path = ConfigKeyPath::from_segments(
            segments
                .into_iter()
                .map(|segment| segment.as_ref().to_string()),
        );
        self.origins().remove(&path)
    }

    fn materialize(&self) -> (TomlValue, BTreeMap<ConfigKeyPath, ConfigOrigin>) {
        let mut effective = TomlValue::Table(Default::default());
        let mut origins = BTreeMap::new();
        for layer in self.layers.iter().filter(|layer| layer.is_enabled()) {
            let origin = ConfigOrigin {
                source: layer.source.clone(),
                version: layer.version.clone(),
            };
            merge_with_origins(
                &mut effective,
                &layer.config,
                &ConfigKeyPath::root(),
                &origin,
                &mut origins,
            );
        }
        (effective, origins)
    }
}

/// Recursively merges `overlay` into `base`. Tables merge by key; scalars and
/// arrays replace the lower-precedence value.
pub fn merge_toml_values(base: &mut TomlValue, overlay: &TomlValue) {
    if let (Some(base), Some(overlay)) = (base.as_table_mut(), overlay.as_table()) {
        for (key, value) in overlay {
            if let Some(existing) = base.get_mut(key) {
                merge_toml_values(existing, value);
            } else {
                base.insert(key.clone(), value.clone());
            }
        }
    } else {
        *base = overlay.clone();
    }
}

fn merge_with_origins(
    base: &mut TomlValue,
    overlay: &TomlValue,
    path: &ConfigKeyPath,
    origin: &ConfigOrigin,
    origins: &mut BTreeMap<ConfigKeyPath, ConfigOrigin>,
) {
    if let (Some(base_table), Some(overlay_table)) = (base.as_table_mut(), overlay.as_table()) {
        for (key, value) in overlay_table {
            let child_path = path.pushed(key);
            if let Some(existing) = base_table.get_mut(key) {
                merge_with_origins(existing, value, &child_path, origin, origins);
            } else {
                base_table.insert(key.clone(), value.clone());
                assign_origin(value, &child_path, origin, origins);
            }
        }
        return;
    }

    *base = overlay.clone();
    origins.retain(|candidate, _| !candidate.starts_with(path));
    assign_origin(overlay, path, origin, origins);
}

fn assign_origin(
    value: &TomlValue,
    path: &ConfigKeyPath,
    origin: &ConfigOrigin,
    origins: &mut BTreeMap<ConfigKeyPath, ConfigOrigin>,
) {
    match value {
        TomlValue::Table(table) if !table.is_empty() => {
            for (key, value) in table {
                assign_origin(value, &path.pushed(key), origin, origins);
            }
        }
        _ => {
            origins.insert(path.clone(), origin.clone());
        }
    }
}

fn fingerprint(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(7 + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn canonical_toml_bytes(value: &TomlValue) -> Vec<u8> {
    fn write_value(value: &TomlValue, output: &mut Vec<u8>) {
        match value {
            TomlValue::String(value) => write_atom(b's', value.as_bytes(), output),
            TomlValue::Integer(value) => write_atom(b'i', value.to_string().as_bytes(), output),
            TomlValue::Float(value) => {
                write_atom(b'f', value.to_bits().to_string().as_bytes(), output)
            }
            TomlValue::Boolean(value) => write_atom(b'b', &[u8::from(*value)], output),
            TomlValue::Datetime(value) => write_atom(b'd', value.to_string().as_bytes(), output),
            TomlValue::Array(values) => {
                output.push(b'a');
                output.extend_from_slice(&values.len().to_be_bytes());
                for value in values {
                    write_value(value, output);
                }
            }
            TomlValue::Table(table) => {
                output.push(b't');
                output.extend_from_slice(&table.len().to_be_bytes());
                let mut entries = table.iter().collect::<Vec<_>>();
                entries.sort_by_key(|(key, _)| *key);
                for (key, value) in entries {
                    write_atom(b'k', key.as_bytes(), output);
                    write_value(value, output);
                }
            }
        }
    }

    fn write_atom(tag: u8, bytes: &[u8], output: &mut Vec<u8>) {
        output.push(tag);
        output.extend_from_slice(&bytes.len().to_be_bytes());
        output.extend_from_slice(bytes);
    }

    let mut output = Vec::new();
    write_value(value, &mut output);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_user() -> ConfigLayerSource {
        ConfigLayerSource::User {
            file: PathBuf::from("/home/test/.codex/config.toml"),
        }
    }

    fn source_project(name: &str) -> ConfigLayerSource {
        ConfigLayerSource::Project {
            dot_config_dir: PathBuf::from(name),
        }
    }

    fn layer(source: ConfigLayerSource, raw: &str) -> ConfigLayerEntry {
        ConfigLayerEntry::parse(source, raw).unwrap()
    }

    #[test]
    fn precedence_and_project_proximity_are_deterministic() {
        let stack = ConfigLayerStack::new(vec![
            layer(source_project("/repo/.codex"), "model = 'root'"),
            layer(source_user(), "model = 'user'"),
            layer(source_project("/repo/sub/.codex"), "model = 'closest'"),
            layer(
                ConfigLayerSource::SessionOverrides,
                "approval_policy = 'never'",
            ),
        ]);

        let effective = stack.effective_config();
        assert_eq!(effective["model"].as_str(), Some("closest"));
        assert_eq!(effective["approval_policy"].as_str(), Some("never"));
        assert!(matches!(
            stack.origin_at(["model"].iter()).unwrap().source,
            ConfigLayerSource::Project { ref dot_config_dir }
                if dot_config_dir == &PathBuf::from("/repo/sub/.codex")
        ));
    }

    #[test]
    fn tables_merge_recursively_while_arrays_and_scalars_replace() {
        let stack = ConfigLayerStack::new(vec![
            layer(
                source_user(),
                "[features]\na = true\nb = true\nitems = [1, 2]\n",
            ),
            layer(
                source_project("/repo/.codex"),
                "[features]\nb = false\nitems = [3]\n",
            ),
        ]);

        let effective = stack.effective_config();
        assert_eq!(effective["features"]["a"].as_bool(), Some(true));
        assert_eq!(effective["features"]["b"].as_bool(), Some(false));
        assert_eq!(effective["features"]["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn replacing_a_table_removes_descendant_origins() {
        let stack = ConfigLayerStack::new(vec![
            layer(source_user(), "[tools]\na = true\nb = true\n"),
            layer(source_project("/repo/.codex"), "tools = false\n"),
        ]);

        let origins = stack.origins();
        assert_eq!(origins.len(), 1);
        assert!(origins.contains_key(&ConfigKeyPath::from_segments(["tools"])));
        assert!(!origins.contains_key(&ConfigKeyPath::from_segments(["tools", "a"])));
    }

    #[test]
    fn literal_dotted_keys_do_not_collide_with_nested_paths() {
        let stack = ConfigLayerStack::new(vec![layer(
            source_user(),
            "[domains]\n'api.example.com' = 'allow'\n[domains.api]\nexample = 'nested'\n",
        )]);
        let origins = stack.origins();

        assert!(origins.contains_key(&ConfigKeyPath::from_segments([
            "domains",
            "api.example.com",
        ])));
        assert!(origins.contains_key(&ConfigKeyPath::from_segments(
            ["domains", "api", "example",]
        )));
    }

    #[test]
    fn disabled_layers_are_visible_but_do_not_affect_effective_config() {
        let stack = ConfigLayerStack::new(vec![
            layer(source_user(), "model = 'safe'"),
            layer(source_project("/repo/.codex"), "model = 'ignored'")
                .disabled("project is untrusted"),
        ]);

        assert_eq!(stack.effective_config()["model"].as_str(), Some("safe"));
        assert_eq!(
            stack
                .layers_high_to_low()
                .next()
                .unwrap()
                .disabled_reason
                .as_deref(),
            Some("project is untrusted")
        );
    }

    #[test]
    fn fingerprints_are_stable_and_track_raw_content() {
        let first = layer(source_user(), "model = 'a'\n");
        let same = layer(source_user(), "model = 'a'\n");
        let reformatted = layer(source_user(), "model='a'\n");

        assert_eq!(first.version, same.version);
        assert_ne!(first.version, reformatted.version);
        assert!(first.version.starts_with("sha256:"));
    }

    #[test]
    fn value_fingerprints_ignore_table_insertion_order() {
        let left: TomlValue = "a = 1\nb = 2".parse().unwrap();
        let right: TomlValue = "b = 2\na = 1".parse().unwrap();
        assert_eq!(
            ConfigLayerEntry::from_value(source_user(), left).version,
            ConfigLayerEntry::from_value(source_user(), right).version
        );
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct StrictRuntimeConfig {
        model: String,
        features: StrictFeatures,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct StrictFeatures {
        shell: bool,
    }

    #[test]
    fn effective_snapshot_decodes_consumer_owned_strict_schema() {
        let stack = ConfigLayerStack::new(vec![
            layer(source_user(), "model = 'base'\n[features]\nshell = false\n"),
            layer(
                source_project("/repo/.codex"),
                "model = 'project'\n[features]\nshell = true\n",
            ),
        ]);

        let snapshot = stack.resolve();
        let decoded = snapshot.decode::<StrictRuntimeConfig>().unwrap();

        assert_eq!(
            decoded,
            StrictRuntimeConfig {
                model: "project".to_string(),
                features: StrictFeatures { shell: true },
            }
        );
        assert!(matches!(
            snapshot.origin_at(["model"].iter()).unwrap().source,
            ConfigLayerSource::Project { .. }
        ));
        assert_eq!(snapshot.layers_low_to_high().len(), 2);
    }

    #[test]
    fn strict_schema_rejects_unknown_effective_keys() {
        let stack = ConfigLayerStack::new(vec![layer(
            source_user(),
            "model = 'base'\nunknown = true\n[features]\nshell = true\n",
        )]);

        let error = stack.resolve().decode::<StrictRuntimeConfig>().unwrap_err();

        assert!(error.to_string().contains("unknown field"));
        assert!(error.to_string().contains("unknown"));
    }

    #[test]
    fn effective_version_tracks_behavior_not_layer_formatting() {
        let formatted = ConfigLayerStack::new(vec![layer(source_user(), "model = 'same'\n")]);
        let compact = ConfigLayerStack::new(vec![layer(source_user(), "model='same'\n")]);
        let overridden = ConfigLayerStack::new(vec![
            layer(source_user(), "model = 'ignored'\n"),
            layer(source_project("/repo/.codex"), "model = 'same'\n"),
        ]);
        let changed = ConfigLayerStack::new(vec![layer(source_user(), "model = 'changed'\n")]);

        assert_eq!(formatted.resolve().version(), compact.resolve().version());
        assert_eq!(
            formatted.resolve().version(),
            overridden.resolve().version()
        );
        assert_ne!(formatted.resolve().version(), changed.resolve().version());
    }

    #[test]
    fn resolved_snapshot_does_not_change_when_stack_changes() {
        let mut stack = ConfigLayerStack::new(vec![layer(source_user(), "model = 'before'\n")]);
        let snapshot = stack.resolve();

        stack.push(layer(
            ConfigLayerSource::RequestOverrides,
            "model = 'after'\n",
        ));

        assert_eq!(snapshot.raw()["model"].as_str(), Some("before"));
        assert_eq!(stack.resolve().raw()["model"].as_str(), Some("after"));
    }
}
