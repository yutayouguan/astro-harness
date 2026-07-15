//! 技能更新状态比对（纯函数，无网络请求）。

use crate::models::{SkillOriginRecord, SkillUpdateStatus};

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
        && origin.remote_version.is_none()
        && origin.remote_updated_at.is_none()
        && origin.last_updated_at.is_none()
    {
        return SkillUpdateStatus::Current;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SkillUpdateStatus;

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
            scope: None,
            installed_at,
            last_updated_at,
            remote_version: remote_version.map(str::to_string),
            remote_updated_at,
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
    fn classify_first_baseline_remote_version_only_is_current() {
        let origin = sample_origin(None, None, None, 100);
        assert_eq!(
            classify_update_status(&origin, Some("1.0.0"), None),
            SkillUpdateStatus::Current
        );
    }
}
