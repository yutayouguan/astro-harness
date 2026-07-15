//! 工作区生命周期 facade：本机脚手架在 `home`；会话库 + 技能播种在此编排。

use std::path::Path;

use session::SessionStore;

pub use home::EnsureWorkspaceReport;

/// 完整首次引导：home 目录脚手架 + 会话库 + 内置技能播种。
pub fn ensure_workspace(base: &Path) -> anyhow::Result<EnsureWorkspaceReport> {
    let mut report = home::ensure_workspace(base)?;

    let _ = SessionStore::open_sessions_dir(&base.join("sessions"))?;

    let bundled = skills::seed_bundled_into(base);
    for name in &bundled.installed {
        let rel = format!("skills/{name}/SKILL.md");
        if !report.created_files.iter().any(|f| f == &rel) {
            report.created_files.push(rel);
        }
    }

    Ok(report)
}

/// 播种内置 `create-agent`（及同批 bundled skills）。
pub fn seed_create_agent_skill(base: &Path) -> anyhow::Result<bool> {
    let report = skills::seed_bundled_into(base);
    Ok(report.installed.iter().any(|n| n == "create-agent"))
}

/// 对默认目录执行 [`ensure_workspace`]。
pub fn ensure_default_workspace() -> anyhow::Result<EnsureWorkspaceReport> {
    ensure_workspace(&home::default_memory_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ensure_workspace_seeds_session_db_and_bundled_skills() {
        let dir = TempDir::new().unwrap();
        let report = ensure_workspace(dir.path()).unwrap();

        assert!(dir.path().join("sessions").join("state.db").is_file());
        assert!(report
            .created_files
            .iter()
            .any(|f| f == "skills/create-agent/SKILL.md"));
        assert!(report
            .created_files
            .iter()
            .any(|f| f == "skills/storyboard-video/SKILL.md"));
        assert!(dir.path().join("skills/create-agent/SKILL.md").is_file());
    }
}
