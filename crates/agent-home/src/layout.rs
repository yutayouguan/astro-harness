//! Domain-owned persistence paths. Runtime code never falls back to the retired layout.
use std::path::{Path, PathBuf};

macro_rules! paths {
    ($($name:ident => $relative:literal),* $(,)?) => {
        $(pub fn $name(base: &Path) -> PathBuf { base.join($relative) })*
    };
}

paths! {
    config_path => "config.toml",
    sessions_dir => "sessions",
    models_dir => "models",
    models_cache_dir => "models/cache",
    mcp_cache_dir => "mcp/cache",
    hook_trust_path => "security/hooks/trust.json",
    hook_runs_dir => "logs/hooks",
    extension_migration_marker => "backups/extensions-in-progress.json",
    layout_migration_marker => "backups/layout-in-progress.json",
    backups_dir => "backups",
    skills_dir => "skills",
    skill_origins_path => "skills/origins.json",
    skill_lock_path => "skills/lock.json",
    skill_backups_dir => "skills/backups",
    active_agent_path => "agents/active.json",
    cron_dir => "automation/cron",
    workflows_dir => "automation/workflows",
    evolution_dir => "evolution",
    learning_dir => "evolution/learning",
    dspy_dir => "evolution/dspy",
    security_audit_dir => "security/audit",
    artifacts_dir => "artifacts",
    uploads_dir => "artifacts/uploads",
    usage_dir => "usage",
    ui_dir => "ui",
    desktop_pet_dir => "ui/desktop-pet",
    app_icon_path => "ui/app-icon.json",
    onboarding_path => "ui/onboarding.json",
    pending_agent_icons_dir => "agents/pending-icons",
    tool_spills_dir => "sessions/tool_spills",
    workflow_db_path => "automation/workflows/workflow.db",
    dreaming_state_path => "memory/dreaming.json",
}

pub(crate) const STORAGE_CLEANUP_RECOVERY_SUBDIR: &str = "backups/storage-cleanup";
pub fn storage_cleanup_recovery_dir(base: &Path) -> PathBuf {
    base.join(STORAGE_CLEANUP_RECOVERY_SUBDIR)
}

/// Only durable domain roots are ensured here; caches are created by their owners.
pub const DOMAIN_DIRS: &[&str] = &[
    "workspace",
    "agents",
    "models",
    "tools",
    "skills",
    "sessions/rollouts",
    "sessions/subagents",
    "memory",
    "automation/cron/output",
    "automation/workflows",
    "evolution",
    "artifacts/uploads",
    "usage",
    "security/audit",
    "browser",
    "ui",
    "logs",
];

/// Bootstrap state lives beside its owning domain, never in retired root files.
pub(crate) const INITIAL_STATE_FILES: &[(&str, &str)] = &[
    ("config.toml", "# Astro configuration\n"),
    ("memory/dreaming.json", "{\n  \"enabled\": false\n}\n"),
    ("automation/cron/jobs.json", "{\n  \"jobs\": []\n}\n"),
    ("agents/active.json", "{\n  \"id\": \"default\"\n}\n"),
];

/// Never initialize an empty parallel store over an unmigrated installation.
pub const RETIRED_LAYOUT_PATHS: &[&str] = &[
    "data",
    "cache",
    "cron",
    "workflows",
    "uploads",
    "learning",
    "audit",
    "pending",
    "config.yaml",
    "providers.json",
    "skills-enabled.json",
    "tools-enabled.json",
    "skill-origins.json",
    "skills-lock.json",
    "active-agent.json",
    "onboarding.json",
    "app-icon.json",
];

pub fn require_current_layout(base: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(!extension_migration_marker(base).exists(), "extension migration is incomplete; restore its recorded backup before restarting");
    anyhow::ensure!(!base.join("backups/layout-in-progress.json").exists(),
        "Astro home migration is incomplete; inspect backups/layout-in-progress.json before restarting");
    for old in RETIRED_LAYOUT_PATHS {
        anyhow::ensure!(!base.join(old).exists(),
            "Astro home uses the retired layout ({old}); stop Astro and run tools/migrate_home_layout.py --root <Astro home> --apply before restarting");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn domain_paths_are_relative_to_the_requested_root() {
        let root = Path::new("test-home");
        assert_eq!(config_path(root), root.join("config.toml"));
        assert_eq!(cron_dir(root), root.join("automation/cron"));
        assert_eq!(tool_spills_dir(root), root.join("sessions/tool_spills"));
    }

    #[test]
    fn rejects_old_store_without_creating_a_parallel_store() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("data")).unwrap();
        std::fs::write(temp.path().join("data/state.db"), "old").unwrap();
        assert!(crate::ensure_workspace_dirs(temp.path()).is_err());
        assert!(!temp.path().join("sessions").exists());
    }
}
