//! 本地命令沙箱、权限 profile 与审批选择的共享类型。

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const READ_ONLY_PROFILE: &str = ":read-only";
pub const WORKSPACE_PROFILE: &str = ":workspace";
pub const DANGER_FULL_ACCESS_PROFILE: &str = ":danger-full-access";

/// 旧 sandbox 配置与平台执行层使用的低层模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

/// 何时产生审批请求。它与沙箱边界、审批人相互独立。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalPolicy {
    Untrusted,
    #[default]
    OnRequest,
    Never,
}

/// 符合资格的审批请求由谁审查。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalsReviewer {
    #[default]
    User,
    AutoReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilesystemAccess {
    Read,
    Write,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkAccess {
    Allow,
    Deny,
}

/// 文件系统边界。`workspace_roots` 中的 key 相对每个有效工作区根解释。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FilesystemPolicy {
    #[serde(default)]
    pub paths: BTreeMap<String, FilesystemAccess>,
    #[serde(default)]
    pub workspace_roots: BTreeMap<String, FilesystemAccess>,
    #[serde(default)]
    pub glob_scan_max_depth: Option<u32>,
}

/// 本地命令网络边界。域名规则仅在命令网络代理启用时具有限制作用。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NetworkPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub domains: BTreeMap<String, NetworkAccess>,
    #[serde(default)]
    pub unix_sockets: BTreeMap<String, NetworkAccess>,
    #[serde(default)]
    pub allow_local_binding: bool,
}

/// 命名 permission profile。审批策略和审批人不属于 profile。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PermissionProfile {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub workspace_roots: BTreeMap<String, bool>,
    #[serde(default)]
    pub filesystem: FilesystemPolicy,
    #[serde(default)]
    pub network: NetworkPolicy,
}

/// Profile 注册表及默认选择。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionsConfig {
    #[serde(default = "default_profile")]
    pub default_profile: String,
    #[serde(default)]
    pub profiles: BTreeMap<String, PermissionProfile>,
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            default_profile: default_profile(),
            profiles: BTreeMap::new(),
        }
    }
}

fn default_profile() -> String {
    WORKSPACE_PROFILE.to_string()
}

impl PermissionsConfig {
    pub fn validate(&self) -> Result<(), PermissionProfileError> {
        self.validate_profile_ref(&self.default_profile)?;
        for (id, profile) in &self.profiles {
            if id.is_empty() || id.starts_with(':') {
                return Err(PermissionProfileError::InvalidName(id.clone()));
            }
            if profile.filesystem.glob_scan_max_depth == Some(0) {
                return Err(PermissionProfileError::InvalidGlobDepth(id.clone()));
            }
            if matches!(profile.network.domains.get("*"), Some(NetworkAccess::Deny)) {
                return Err(PermissionProfileError::GlobalDenyWildcard(id.clone()));
            }
            self.resolve_chain(id)?;
        }
        Ok(())
    }

    pub fn sandbox_mode_for(
        &self,
        profile_id: &str,
    ) -> Result<SandboxMode, PermissionProfileError> {
        self.validate_profile_ref(profile_id)?;
        Ok(match profile_id {
            READ_ONLY_PROFILE => SandboxMode::ReadOnly,
            DANGER_FULL_ACCESS_PROFILE => SandboxMode::DangerFullAccess,
            WORKSPACE_PROFILE => SandboxMode::WorkspaceWrite,
            custom => match self.resolve_chain(custom)?.last().map(String::as_str) {
                Some(READ_ONLY_PROFILE) => SandboxMode::ReadOnly,
                _ => SandboxMode::WorkspaceWrite,
            },
        })
    }

    fn validate_profile_ref(&self, id: &str) -> Result<(), PermissionProfileError> {
        if is_builtin_profile(id) || self.profiles.contains_key(id) {
            Ok(())
        } else {
            Err(PermissionProfileError::UnknownProfile(id.to_string()))
        }
    }

    fn resolve_chain(&self, id: &str) -> Result<Vec<String>, PermissionProfileError> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = id;
        loop {
            if !seen.insert(current.to_string()) {
                return Err(PermissionProfileError::InheritanceCycle(
                    current.to_string(),
                ));
            }
            chain.push(current.to_string());
            if is_builtin_profile(current) {
                return Ok(chain);
            }
            let profile = self
                .profiles
                .get(current)
                .ok_or_else(|| PermissionProfileError::UnknownProfile(current.to_string()))?;
            let Some(parent) = profile.extends.as_deref() else {
                return Ok(chain);
            };
            if parent == DANGER_FULL_ACCESS_PROFILE {
                return Err(PermissionProfileError::ExtendsDangerFullAccess(
                    current.to_string(),
                ));
            }
            self.validate_profile_ref(parent)?;
            current = parent;
        }
    }
}

fn is_builtin_profile(id: &str) -> bool {
    matches!(
        id,
        READ_ONLY_PROFILE | WORKSPACE_PROFILE | DANGER_FULL_ACCESS_PROFILE
    )
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PermissionProfileError {
    #[error("unknown permission profile: {0}")]
    UnknownProfile(String),
    #[error("custom permission profile name must be non-empty and must not start with ':': {0}")]
    InvalidName(String),
    #[error("permission profile inheritance cycle: {0}")]
    InheritanceCycle(String),
    #[error("permission profile {0} cannot extend :danger-full-access")]
    ExtendsDangerFullAccess(String),
    #[error("permission profile {0} has glob_scan_max_depth=0")]
    InvalidGlobDepth(String),
    #[error("permission profile {0} uses '*' as a deny rule; '*' is allow-only")]
    GlobalDenyWildcard(String),
}

/// 会话真正激活的三个正交权限维度。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPermissions {
    pub profile_id: String,
    pub approval_policy: ApprovalPolicy,
    pub approvals_reviewer: ApprovalsReviewer,
}

/// 桌面端权限选择器中的四个内置组合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPreset {
    AskForApproval,
    ApproveForMe,
    ReadOnly,
    FullAccess,
}

impl PermissionPreset {
    pub fn selection(self) -> SessionPermissions {
        match self {
            Self::AskForApproval => SessionPermissions::ask_for_approval(),
            Self::ApproveForMe => SessionPermissions::approve_for_me(),
            Self::ReadOnly => SessionPermissions::read_only(),
            Self::FullAccess => SessionPermissions::full_access(),
        }
    }

    pub fn from_selection(selection: &SessionPermissions) -> Option<Self> {
        if selection == &SessionPermissions::ask_for_approval() {
            Some(Self::AskForApproval)
        } else if selection == &SessionPermissions::approve_for_me() {
            Some(Self::ApproveForMe)
        } else if selection == &SessionPermissions::read_only() {
            Some(Self::ReadOnly)
        } else if selection == &SessionPermissions::full_access() {
            Some(Self::FullAccess)
        } else {
            None
        }
    }
}

impl Default for SessionPermissions {
    fn default() -> Self {
        Self::ask_for_approval()
    }
}

impl SessionPermissions {
    pub fn ask_for_approval() -> Self {
        Self {
            profile_id: WORKSPACE_PROFILE.to_string(),
            approval_policy: ApprovalPolicy::OnRequest,
            approvals_reviewer: ApprovalsReviewer::User,
        }
    }

    pub fn approve_for_me() -> Self {
        Self {
            approvals_reviewer: ApprovalsReviewer::AutoReview,
            ..Self::ask_for_approval()
        }
    }

    pub fn read_only() -> Self {
        Self {
            profile_id: READ_ONLY_PROFILE.to_string(),
            ..Self::ask_for_approval()
        }
    }

    pub fn full_access() -> Self {
        Self {
            profile_id: DANGER_FULL_ACCESS_PROFILE.to_string(),
            approval_policy: ApprovalPolicy::Never,
            approvals_reviewer: ApprovalsReviewer::User,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_map_to_expected_sandbox_modes() {
        let config = PermissionsConfig::default();
        assert_eq!(
            config.sandbox_mode_for(READ_ONLY_PROFILE).unwrap(),
            SandboxMode::ReadOnly
        );
        assert_eq!(
            config.sandbox_mode_for(WORKSPACE_PROFILE).unwrap(),
            SandboxMode::WorkspaceWrite
        );
        assert_eq!(
            config.sandbox_mode_for(DANGER_FULL_ACCESS_PROFILE).unwrap(),
            SandboxMode::DangerFullAccess
        );
    }

    #[test]
    fn profile_inheritance_rejects_cycles_and_full_access_parent() {
        let mut config = PermissionsConfig::default();
        config.profiles.insert(
            "a".into(),
            PermissionProfile {
                extends: Some("b".into()),
                ..PermissionProfile::default()
            },
        );
        config.profiles.insert(
            "b".into(),
            PermissionProfile {
                extends: Some("a".into()),
                ..PermissionProfile::default()
            },
        );
        assert!(matches!(
            config.validate(),
            Err(PermissionProfileError::InheritanceCycle(_))
        ));

        config.profiles.clear();
        config.profiles.insert(
            "unsafe-child".into(),
            PermissionProfile {
                extends: Some(DANGER_FULL_ACCESS_PROFILE.into()),
                ..PermissionProfile::default()
            },
        );
        assert!(matches!(
            config.validate(),
            Err(PermissionProfileError::ExtendsDangerFullAccess(_))
        ));
    }

    #[test]
    fn permission_presets_keep_profile_policy_and_reviewer_separate() {
        let auto = SessionPermissions::approve_for_me();
        assert_eq!(auto.profile_id, WORKSPACE_PROFILE);
        assert_eq!(auto.approval_policy, ApprovalPolicy::OnRequest);
        assert_eq!(auto.approvals_reviewer, ApprovalsReviewer::AutoReview);

        let full = SessionPermissions::full_access();
        assert_eq!(full.profile_id, DANGER_FULL_ACCESS_PROFILE);
        assert_eq!(full.approval_policy, ApprovalPolicy::Never);
        assert_eq!(
            PermissionPreset::from_selection(&auto),
            Some(PermissionPreset::ApproveForMe)
        );
    }
}
