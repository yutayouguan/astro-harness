use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// Canonical persisted path for a Codex V2 agent thread.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AgentPath(String);

impl AgentPath {
    const ROOT: &str = "/root";

    pub fn root() -> Self {
        Self(Self::ROOT.to_string())
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        validate_absolute_path(value)?;
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn child(&self, task_name: &str) -> Result<Self, String> {
        validate_segment(task_name)?;
        Self::parse(&format!("{}/{task_name}", self.as_str()))
    }

    pub fn resolve(&self, target: &str) -> Result<Self, String> {
        if target.starts_with('/') {
            Self::parse(target)
        } else {
            self.child(target)
        }
    }

    pub fn parent(&self) -> Option<Self> {
        if self.as_str() == Self::ROOT {
            return None;
        }
        let (parent, _) = self.as_str().rsplit_once('/')?;
        Self::parse(parent).ok()
    }

    pub fn starts_with(&self, prefix: &Self) -> bool {
        self == prefix
            || self
                .as_str()
                .strip_prefix(prefix.as_str())
                .is_some_and(|remaining| remaining.starts_with('/'))
    }

    pub fn depth(&self) -> usize {
        self.as_str().split('/').count() - 2
    }

    pub fn name(&self) -> &str {
        self.as_str().rsplit('/').next().unwrap_or("root")
    }
}

impl AsRef<str> for AgentPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl<'de> Deserialize<'de> for AgentPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for AgentPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn validate_absolute_path(value: &str) -> Result<(), String> {
    if value == AgentPath::ROOT {
        return Ok(());
    }
    let Some(relative) = value.strip_prefix("/root/") else {
        return Err("agent path must be /root or begin with /root/".to_string());
    };
    if relative.is_empty() || relative.ends_with('/') {
        return Err("agent path must not contain empty segments".to_string());
    }
    for segment in relative.split('/') {
        validate_segment(segment)?;
    }
    Ok(())
}

fn validate_segment(value: &str) -> Result<(), String> {
    if value.is_empty() || value.contains('/') {
        return Err("agent task name must be exactly one non-empty segment".to_string());
    }
    if value == "." || value == ".." {
        return Err("agent task name must not be a dot segment".to_string());
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(
            "agent task name must use lowercase ASCII letters, digits, or underscores".to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::AgentPath;

    #[test]
    fn resolves_relative_and_absolute_agent_paths() {
        let parent = AgentPath::parse("/root/research").unwrap();
        assert_eq!(
            parent.child("citations").unwrap().as_str(),
            "/root/research/citations"
        );
        assert_eq!(
            parent.resolve("/root/review").unwrap().as_str(),
            "/root/review"
        );
        assert_eq!(
            parent.resolve("citations").unwrap().as_str(),
            "/root/research/citations"
        );
    }

    #[test]
    fn rejects_invalid_task_segments() {
        for value in ["", "UPPER", "has-dash", "../escape", "two/parts"] {
            assert!(AgentPath::root().child(value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn root_has_no_parent_and_prefixes_are_segment_aware() {
        let root = AgentPath::root();
        assert_eq!(root.parent(), None);
        assert!(AgentPath::parse("/root/a").unwrap().starts_with(&root));
        assert!(!AgentPath::parse("/root/ab")
            .unwrap()
            .starts_with(&AgentPath::parse("/root/a").unwrap()));
    }

    #[test]
    fn serde_roundtrip_is_stable() {
        let path = AgentPath::parse("/root/research/citations").unwrap();
        let serialized = serde_json::to_string(&path).unwrap();
        assert_eq!(serialized, "\"/root/research/citations\"");
        assert_eq!(
            serde_json::from_str::<AgentPath>(&serialized).unwrap(),
            path
        );
    }

    #[test]
    fn serde_rejects_invalid_paths() {
        for value in ["/other", "/root/a/", "/root/UPPER"] {
            assert!(
                serde_json::from_str::<AgentPath>(&format!("\"{value}\"")).is_err(),
                "accepted {value}"
            );
        }
    }
}
