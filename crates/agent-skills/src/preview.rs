//! 更新前预览本地技能目录是否与安装 baseline digest 一致。

use anyhow::Result;
use std::path::Path;

use crate::digest::skill_content_digest;
use crate::install::scoped_skills_path;
use crate::models::SkillUpdatePreview;
use crate::origins::find_origin;

/// 比对 origin `content_digest` 与当前技能目录 digest，判断是否有本地改动。
pub fn preview_skill_update(
    agent_id: Option<&str>,
    scope: &str,
    project_root: Option<&Path>,
    folder: &str,
) -> Result<SkillUpdatePreview> {
    let origin = find_origin(agent_id, scope, folder)?;
    let baseline_digest = origin.as_ref().and_then(|o| o.content_digest.clone());
    let has_baseline_digest = baseline_digest.is_some();

    let current_digest = match scoped_skills_path(scope, project_root) {
        Ok(skills_dir) => {
            let skill_path = skills_dir.join(folder);
            if skill_path.is_dir() {
                Some(skill_content_digest(&skill_path)?)
            } else {
                None
            }
        }
        Err(_) => None,
    };

    let has_local_changes = match (&baseline_digest, &current_digest) {
        (Some(baseline), Some(current)) => baseline != current,
        _ => false,
    };

    Ok(SkillUpdatePreview {
        folder: folder.to_string(),
        has_local_changes,
        has_baseline_digest,
        current_digest,
        baseline_digest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::skill_content_digest;
    use crate::install::scoped_skills_dir;
    use crate::models::SkillOriginRecord;
    use crate::origins::upsert_origin;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_skill_md(dir: &std::path::Path, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let mut f = std::fs::File::create(dir.join("SKILL.md")).unwrap();
        writeln!(f, "{body}").unwrap();
    }

    fn upsert_test_origin(folder: &str, content_digest: Option<String>) {
        upsert_origin(SkillOriginRecord {
            folder: folder.into(),
            skill_id: None,
            name: "demo".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:owner/demo".into(),
            agent_id: Some("workspace".into()),
            scope: Some("global".into()),
            installed_at: 1,
            last_updated_at: None,
            remote_version: None,
            remote_updated_at: None,
            content_digest,
        })
        .unwrap();
    }

    #[test]
    fn preview_no_baseline_digest_reports_no_local_changes() {
        let dir = tempdir().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());

        let skills_dir = scoped_skills_dir("global", None).unwrap();
        write_skill_md(&skills_dir.join("demo-skill"), "# Demo");

        upsert_test_origin("demo-skill", None);

        let preview =
            preview_skill_update(Some("workspace"), "global", None, "demo-skill").unwrap();
        assert!(!preview.has_baseline_digest);
        assert!(!preview.has_local_changes);
        assert!(preview.baseline_digest.is_none());
        assert!(preview.current_digest.is_some());
    }

    #[test]
    fn preview_equal_digests_reports_no_local_changes() {
        let dir = tempdir().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());

        let skills_dir = scoped_skills_dir("global", None).unwrap();
        let skill_dir = skills_dir.join("demo-skill");
        write_skill_md(&skill_dir, "# Demo");
        let digest = skill_content_digest(&skill_dir).unwrap();

        upsert_test_origin("demo-skill", Some(digest.clone()));

        let preview =
            preview_skill_update(Some("workspace"), "global", None, "demo-skill").unwrap();
        assert!(preview.has_baseline_digest);
        assert!(!preview.has_local_changes);
        assert_eq!(preview.baseline_digest.as_deref(), Some(digest.as_str()));
        assert_eq!(preview.current_digest.as_deref(), Some(digest.as_str()));
    }

    #[test]
    fn preview_different_digests_reports_local_changes() {
        let dir = tempdir().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());

        let skills_dir = scoped_skills_dir("global", None).unwrap();
        let skill_dir = skills_dir.join("demo-skill");
        write_skill_md(&skill_dir, "# Changed locally");
        let current = skill_content_digest(&skill_dir).unwrap();

        upsert_test_origin("demo-skill", Some("stale-baseline-digest".into()));

        let preview =
            preview_skill_update(Some("workspace"), "global", None, "demo-skill").unwrap();
        assert!(preview.has_baseline_digest);
        assert!(preview.has_local_changes);
        assert_eq!(
            preview.baseline_digest.as_deref(),
            Some("stale-baseline-digest")
        );
        assert_eq!(preview.current_digest.as_deref(), Some(current.as_str()));
        assert_ne!(preview.baseline_digest, preview.current_digest);
    }
}
