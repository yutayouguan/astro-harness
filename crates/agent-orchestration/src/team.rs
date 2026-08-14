//! Team definitions inspired by Agno Team.
//!
//! This module intentionally stores only the durable definition. Execution lives
//! in the tools/agent layer so it can reuse the existing delegate runtime.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum TeamMode {
    #[default]
    Coordinate,
    Route,
    Broadcast,
    Tasks,
}

impl TeamMode {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "coordinate" => Ok(Self::Coordinate),
            "route" => Ok(Self::Route),
            "broadcast" => Ok(Self::Broadcast),
            "tasks" => Ok(Self::Tasks),
            other => anyhow::bail!("unsupported team mode: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub id: String,
    pub role: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub toolsets: Option<Vec<String>>,
    /// 可选成员模型简写：`provider:model_id`（见 [`common::ModelSpec`]）。
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mode: TeamMode,
    #[serde(default)]
    pub leader_agent_id: Option<String>,
    pub members: Vec<TeamMember>,
}

pub fn teams_dir(base: &Path) -> PathBuf {
    base.join("teams")
}

pub fn ensure_teams_dir(base: &Path) -> Result<PathBuf> {
    let dir = teams_dir(base);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn team_path(base: &Path, id: &str) -> PathBuf {
    teams_dir(base).join(format!("{}.json", sanitize_id(id)))
}

pub fn save_team(base: &Path, team: &TeamDefinition) -> Result<()> {
    validate_team(team)?;
    ensure_teams_dir(base)?;
    let json = serde_json::to_string_pretty(team)?;
    fs::write(team_path(base, &team.id), json).context("write team definition")
}

pub fn load_team(base: &Path, id: &str) -> Result<TeamDefinition> {
    let path = team_path(base, id);
    let text = fs::read_to_string(&path)
        .with_context(|| format!("read team definition: {}", path.display()))?;
    let team: TeamDefinition = serde_json::from_str(&text).context("parse team definition")?;
    validate_team(&team)?;
    Ok(team)
}

pub fn list_teams(base: &Path) -> Result<Vec<TeamDefinition>> {
    let dir = ensure_teams_dir(base)?;
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read team definition: {}", path.display()))?;
        let team: TeamDefinition = serde_json::from_str(&text)
            .with_context(|| format!("parse team definition: {}", path.display()))?;
        validate_team(&team)?;
        out.push(team);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

pub fn validate_team(team: &TeamDefinition) -> Result<()> {
    if team.id.trim().is_empty() {
        anyhow::bail!("team id cannot be empty");
    }
    if team.name.trim().is_empty() {
        anyhow::bail!("team name cannot be empty");
    }
    if team.members.is_empty() {
        anyhow::bail!("team members cannot be empty");
    }
    if team.members.len() > 8 {
        anyhow::bail!("team members cannot exceed 8");
    }
    for member in &team.members {
        if member.id.trim().is_empty() {
            anyhow::bail!("team member id cannot be empty");
        }
        if member.role.trim().is_empty() {
            anyhow::bail!("team member role cannot be empty");
        }
    }
    Ok(())
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_team_definition() {
        let dir = tempfile::tempdir().unwrap();
        let team = TeamDefinition {
            id: "research".into(),
            name: "Research Team".into(),
            mode: TeamMode::Broadcast,
            leader_agent_id: None,
            members: vec![TeamMember {
                id: "web".into(),
                role: "Web researcher".into(),
                description: Some("Searches current docs".into()),
                agent_id: None,
                toolsets: Some(vec!["web".into()]),
                model: Some("openai:gpt-5.6".into()),
            }],
        };
        save_team(dir.path(), &team).unwrap();
        let loaded = load_team(dir.path(), "research").unwrap();
        assert_eq!(loaded.mode, TeamMode::Broadcast);
        assert_eq!(loaded.members[0].model.as_deref(), Some("openai:gpt-5.6"));
        assert_eq!(list_teams(dir.path()).unwrap().len(), 1);
    }
}
