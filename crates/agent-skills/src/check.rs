//! 技能更新状态比对与批量检查。

use anyhow::Result;
use std::path::Path;

use crate::agent_id::normalize as normalize_agent_id;
use crate::install::scoped_skills_path;
use crate::models::{
    SkillOriginRecord, SkillUpdateCheckResult, SkillUpdateStatus, StoreSkillDetail,
};
pub use crate::origins::origin_to_store_skill;
use crate::origins::{ensure_bundled_skillhub_origins, load_origins};
use crate::store::fetch_detail_strict;

/// 用本地 origin 快照与远端快照判定更新状态。
pub fn classify_update_status(
    origin: &SkillOriginRecord,
    remote_version: Option<&str>,
    remote_updated_at: Option<i64>,
) -> SkillUpdateStatus {
    if remote_version.is_none() && remote_updated_at.is_none() {
        return SkillUpdateStatus::Unknown;
    }

    if remote_version.is_some()
        && remote_updated_at.is_none()
        && origin.remote_version.is_none()
        && origin.remote_updated_at.is_none()
        && origin.last_updated_at.is_none()
    {
        return SkillUpdateStatus::Unknown;
    }

    if let (Some(local_v), Some(remote_v)) = (&origin.remote_version, remote_version) {
        if local_v.trim() != remote_v.trim() {
            return SkillUpdateStatus::Outdated;
        }
    }

    if let Some(remote_ts) = remote_updated_at {
        let local_ts = origin
            .remote_updated_at
            .or(origin.last_updated_at)
            .or(Some(origin.installed_at));
        if let Some(local_ts) = local_ts {
            if remote_ts > local_ts {
                return SkillUpdateStatus::Outdated;
            }
        }
    }

    SkillUpdateStatus::Current
}

/// 用 origin baseline 与已拉取的远端详情判定单条检查结果（无网络，便于单测）。
pub fn check_origin_against_detail(
    origin: &SkillOriginRecord,
    detail: &StoreSkillDetail,
) -> SkillUpdateCheckResult {
    let status = classify_update_status(origin, detail.version.as_deref(), detail.updated_at);
    SkillUpdateCheckResult {
        folder: origin.folder.clone(),
        status,
        remote_version: detail.version.clone(),
        remote_updated_at: detail.updated_at,
        message: String::new(),
    }
}

fn installed_skill_folder_exists(
    scope: &str,
    project_root: Option<&Path>,
    folder: &str,
) -> Result<bool> {
    let skills_dir = scoped_skills_path(scope, project_root)?;
    Ok(skills_dir.join(folder).is_dir())
}

/// 检查当前 Agent 下所有有来源记录的技能更新状态；不写回 origin baseline `remote_*`。
pub async fn check_updates_for_agent(
    agent_id: Option<&str>,
    scope: &str,
    project_root: Option<&Path>,
) -> Result<Vec<SkillUpdateCheckResult>> {
    let target = normalize_agent_id(agent_id);
    if scope == "global" {
        ensure_bundled_skillhub_origins(Some(&target))?;
    }
    let file = load_origins()?;
    let origins: Vec<SkillOriginRecord> = file
        .records
        .into_iter()
        .filter(|r| {
            normalize_agent_id(r.agent_id.as_deref()) == target && r.scope.as_deref() == Some(scope)
        })
        .collect();

    let mut results = Vec::with_capacity(origins.len());
    for origin in origins {
        match installed_skill_folder_exists(scope, project_root, &origin.folder) {
            Ok(false) => continue,
            Ok(true) => {
                let store_skill = origin_to_store_skill(&origin);
                let item = match fetch_detail_strict(&store_skill).await {
                    Ok(detail) => check_origin_against_detail(&origin, &detail),
                    Err(e) => SkillUpdateCheckResult {
                        folder: origin.folder.clone(),
                        status: SkillUpdateStatus::Error,
                        remote_version: None,
                        remote_updated_at: None,
                        message: e.to_string(),
                    },
                };
                results.push(item);
            }
            Err(e) => {
                results.push(SkillUpdateCheckResult {
                    folder: origin.folder.clone(),
                    status: SkillUpdateStatus::Error,
                    remote_version: None,
                    remote_updated_at: None,
                    message: e.to_string(),
                });
            }
        }
    }
    Ok(results)
}

/// 从检查结果中取出 `Outdated` 技能的 folder 列表。
pub fn filter_outdated_folders(results: &[SkillUpdateCheckResult]) -> Vec<String> {
    results
        .iter()
        .filter(|r| r.status == SkillUpdateStatus::Outdated)
        .map(|r| r.folder.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SkillUpdateStatus;
    use crate::origins::{load_origins, upsert_origin};
    use crate::ENV_TEST_LOCK;
    use tempfile::tempdir;

    fn sample_origin(
        remote_version: Option<&str>,
        remote_updated_at: Option<i64>,
        last_updated_at: Option<i64>,
        installed_at: i64,
    ) -> SkillOriginRecord {
        SkillOriginRecord {
            folder: "demo-skill".into(),
            skill_id: None,
            name: "Demo".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:owner/demo-skill".into(),
            agent_id: Some("workspace".into()),
            scope: Some("global".into()),
            installed_at,
            last_updated_at,
            remote_version: remote_version.map(str::to_string),
            remote_updated_at,
            content_digest: None,
        }
    }

    fn sample_detail(version: Option<&str>, updated_at: Option<i64>) -> StoreSkillDetail {
        StoreSkillDetail {
            name: "Demo".into(),
            slug: "demo-skill".into(),
            description: String::new(),
            overview: String::new(),
            source: "owner".into(),
            store: "skillhub".into(),
            installs: None,
            downloads: None,
            stars: None,
            install_ref: "skillhub:owner/demo-skill".into(),
            homepage: None,
            detail_url: "https://skillhub.cn/skills/demo-skill".into(),
            icon_url: None,
            category: None,
            sub_categories: vec![],
            version: version.map(str::to_string),
            updated_at,
            owner_name: None,
            verified: None,
        }
    }

    #[test]
    fn classify_version_mismatch_is_outdated() {
        let origin = sample_origin(Some("1.0.0"), None, None, 100);
        assert_eq!(
            classify_update_status(&origin, Some("2.0.0"), None),
            SkillUpdateStatus::Outdated
        );
    }

    #[test]
    fn classify_remote_updated_at_newer_is_outdated() {
        let origin = sample_origin(None, Some(100), None, 50);
        assert_eq!(
            classify_update_status(&origin, None, Some(200)),
            SkillUpdateStatus::Outdated
        );
    }

    #[test]
    fn classify_both_remote_fields_empty_is_unknown() {
        let origin = sample_origin(Some("1.0.0"), Some(100), None, 50);
        assert_eq!(
            classify_update_status(&origin, None, None),
            SkillUpdateStatus::Unknown
        );
    }

    #[test]
    fn classify_equal_version_is_current() {
        let origin = sample_origin(Some("1.0.0"), None, None, 100);
        assert_eq!(
            classify_update_status(&origin, Some("1.0.0"), None),
            SkillUpdateStatus::Current
        );
    }

    #[test]
    fn classify_uses_installed_at_when_no_remote_timestamps() {
        let origin = sample_origin(None, None, None, 100);
        assert_eq!(
            classify_update_status(&origin, None, Some(50)),
            SkillUpdateStatus::Current
        );
        assert_eq!(
            classify_update_status(&origin, None, Some(150)),
            SkillUpdateStatus::Outdated
        );
    }

    #[test]
    fn classify_first_baseline_remote_version_only_is_unknown() {
        let origin = sample_origin(None, None, None, 100);
        assert_eq!(
            classify_update_status(&origin, Some("1.0.0"), None),
            SkillUpdateStatus::Unknown
        );
    }

    #[test]
    fn classify_first_baseline_remote_version_and_updated_at_compares_time() {
        let origin = sample_origin(None, None, None, 100);
        assert_eq!(
            classify_update_status(&origin, Some("1.0"), Some(200)),
            SkillUpdateStatus::Outdated
        );
    }

    #[test]
    fn origin_to_store_skill_maps_fields() {
        let origin = sample_origin(None, None, None, 1);
        let skill = origin_to_store_skill(&origin);
        assert_eq!(skill.id, "skillhub:owner/demo-skill");
        assert_eq!(skill.name, "Demo");
        assert_eq!(skill.store, "skillhub");
        assert_eq!(skill.install_ref, "skillhub:owner/demo-skill");
        assert_eq!(skill.source, "owner");
    }

    #[test]
    fn check_origin_against_detail_outdated() {
        let origin = sample_origin(Some("1.0.0"), None, None, 100);
        let detail = sample_detail(Some("2.0.0"), None);
        let result = check_origin_against_detail(&origin, &detail);
        assert_eq!(result.status, SkillUpdateStatus::Outdated);
        assert_eq!(result.folder, "demo-skill");
        assert_eq!(result.remote_version.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn filter_outdated_folders_returns_only_outdated() {
        let results = vec![
            SkillUpdateCheckResult {
                folder: "a".into(),
                status: SkillUpdateStatus::Outdated,
                remote_version: Some("2.0.0".into()),
                remote_updated_at: None,
                message: String::new(),
            },
            SkillUpdateCheckResult {
                folder: "b".into(),
                status: SkillUpdateStatus::Current,
                remote_version: Some("1.0.0".into()),
                remote_updated_at: None,
                message: String::new(),
            },
            SkillUpdateCheckResult {
                folder: "c".into(),
                status: SkillUpdateStatus::Unknown,
                remote_version: None,
                remote_updated_at: None,
                message: String::new(),
            },
        ];
        assert_eq!(filter_outdated_folders(&results), vec!["a".to_string()]);
    }

    #[test]
    fn filter_outdated_folders_empty_when_none_outdated() {
        let results = vec![
            SkillUpdateCheckResult {
                folder: "b".into(),
                status: SkillUpdateStatus::Current,
                remote_version: Some("1.0.0".into()),
                remote_updated_at: None,
                message: String::new(),
            },
            SkillUpdateCheckResult {
                folder: "c".into(),
                status: SkillUpdateStatus::Error,
                remote_version: None,
                remote_updated_at: None,
                message: "fetch failed".into(),
            },
        ];
        assert!(filter_outdated_folders(&results).is_empty());
    }

    #[test]
    fn check_origin_against_detail_unknown_when_no_remote_meta() {
        let origin = sample_origin(Some("1.0.0"), Some(100), None, 50);
        let detail = sample_detail(None, None);
        let result = check_origin_against_detail(&origin, &detail);
        assert_eq!(result.status, SkillUpdateStatus::Unknown);
    }

    #[tokio::test]
    async fn check_updates_skips_orphan_without_origin_mutation() {
        let _guard = ENV_TEST_LOCK.lock().await;
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        upsert_origin(SkillOriginRecord {
            folder: "ghost-skill".into(),
            skill_id: None,
            name: "ghost".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:owner/ghost".into(),
            agent_id: Some("workspace".into()),
            scope: Some("global".into()),
            installed_at: 1,
            last_updated_at: None,
            remote_version: Some("1.0.0".into()),
            remote_updated_at: Some(100),
            content_digest: None,
        })
        .unwrap();

        let results = check_updates_for_agent(Some("workspace"), "global", None)
            .await
            .unwrap();
        assert!(results.is_empty());

        let file = load_origins().unwrap();
        assert_eq!(file.records.len(), 1);
        assert_eq!(file.records[0].remote_version.as_deref(), Some("1.0.0"));
        assert_eq!(file.records[0].remote_updated_at, Some(100));
    }
}
