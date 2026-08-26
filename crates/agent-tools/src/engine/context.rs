//! 工具执行上下文：由 AgentLoop 注入的运行时依赖与凭证。
//!
//! 工具执行上下文与凭证。

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use memory::MemoryManager;
use session::ConversationStore;
use types::DANGER_FULL_ACCESS_PROFILE;

use super::network::InProcessNetworkGrant;

pub use types::credentials::{ImageGenCreds, ImageGenParts, ImageGenTargets, ModelCredentials};

/// 使用 Provider 默认模型名构建 ImageGenTargets。
pub fn image_gen_targets_from_parts(p: ImageGenParts<'_>) -> ImageGenTargets {
    ImageGenTargets::from_parts(p, |provider| {
        providers::image_gen::default_image_model(provider).to_string()
    })
}

/// 单次工具调用的共享运行时上下文，由 AgentLoop 在每次 `dispatch_tool` 前构造。
pub struct ToolContext<'a> {
    /// 当前 Agent 的记忆管理器；只在同步 memory/context/persona 操作期间短暂加锁。
    pub memory: &'a RwLock<MemoryManager>,
    /// 共享会话库（`{memory_dir}/sessions`），供 `search` 使用。
    pub sessions: &'a dyn ConversationStore,
    /// Agent 根目录（`~/.astro`），用于定位 `agents/{id}/` 等全局路径。
    pub memory_dir: PathBuf,
    /// 当前 Agent 工作区目录（记忆空间），与代码仓分离。
    pub workspace_dir: PathBuf,
    /// 可选代码/项目根（git worktree 或 `ASTRO_PROJECT_ROOT`）；有值时 terminal/file_ops 以此为根。
    pub project_root: Option<PathBuf>,
    /// 项目全部授权根；第一个元素是主 cwd。
    pub workspace_roots: Vec<PathBuf>,
    /// 媒体生成主备凭证，由前端 Provider 面板注入。
    pub image_gen_targets: &'a ImageGenTargets,
    /// 当前会话 id；subagent threads、todo 等持久记录用它关联父会话。
    pub session_id: String,
    /// 当前流式 run 的 turn_id（与 agent `run_id` 相同）；未在 run 内为 `None`。
    pub turn_id: Option<String>,
    /// 当前聊天会话的 LLM 凭证（provider / model / api_key / base_url）。
    pub credentials: &'a ModelCredentials,
    /// 含 primary 的聊天 fallback 链，供子 Agent thread 继承。
    pub chat_targets: &'a [types::ChatTarget],
    /// 子 Agent 执行调度器（由 AgentLoop 注入；工具层测试可为 None）。
    pub execution: Option<Arc<dyn crate::AgentThreadDispatch>>,
    /// 当前会话的权限 profile；子 Agent 缺省继承，可由 custom agent 收紧。
    pub permission_profile: Option<String>,
    /// Ephemeral custom-agent skill enable/disable layer.
    pub skill_config_overrides: &'a [(PathBuf, bool)],
    /// 插件钩子总线（由 AgentLoop 注入；无 bus 时对应工具跳过 transform 钩子）。
    pub hook_bus: Option<Arc<hooks::PluginHookBus>>,
    /// 完整 HookRuntime；Agent Thread 用它继承 Plugin/Gateway/Shell 三套 transport。
    pub hook_runtime: Option<Arc<hooks::HookRuntime>>,
    /// 当前单次工具调用已获得 workspace-write 临时授权。
    ///
    /// 该值只存在于本次 `ToolContext` 生命周期，不会持久化或扩大到后续工具调用。
    pub workspace_write_grant: bool,
    /// Current attempt-scoped sandbox policy selected by the orchestrator.
    ///
    /// Only the orchestrator may set this for initial or escalated attempts.
    /// It is never persisted or inherited by later tool calls.
    pub sandbox_policy: Option<sandbox::SandboxPolicy>,
    /// 当前单次工具调用已获得的进程内网络主机授权。
    ///
    /// 与命令沙箱的 `network.enabled` 相互独立，不会持久化或跨工具调用复用。
    pub network_grant: InProcessNetworkGrant,
    /// Current attempt-scoped managed proxy lease.
    ///
    /// The lease keeps the listener alive through sandbox setup and process execution.
    pub managed_network: Option<Arc<network_proxy::StartedNetworkProxy>>,
    /// 当前模型上下文窗口总容量（token 数）；由 AgentLoop 注入，`None` 表示未知。
    pub context_window: Option<u64>,
    /// 当前已使用的上下文 token 数；由 AgentLoop 注入，`None` 表示未知。
    pub context_tokens_used: Option<u64>,
}

impl<'a> ToolContext<'a> {
    /// 获取当前 Agent 记忆的只读 guard；不得跨 `.await` 持有。
    pub fn memory(&self) -> RwLockReadGuard<'_, MemoryManager> {
        self.memory.read().expect("memory manager lock poisoned")
    }

    /// 获取当前 Agent 记忆的写 guard；不得跨 `.await` 持有。
    pub fn memory_mut(&self) -> RwLockWriteGuard<'_, MemoryManager> {
        self.memory.write().expect("memory manager lock poisoned")
    }

    /// 返回当前 Agent 标识的拥有所有权快照。
    pub fn agent_id(&self) -> String {
        self.memory().agent_id.clone()
    }

    /// 确保记忆工作区存在。
    pub fn ensure_workspace(&self) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(&self.workspace_dir)?;
        Ok(self.workspace_dir.clone())
    }

    /// 优先 project_root，否则 workspace_dir。
    pub fn project_or_workspace(&self) -> &Path {
        self.project_root
            .as_deref()
            .unwrap_or(self.workspace_dir.as_path())
    }

    /// 确保 [`Self::project_or_workspace`] 目录存在。
    pub fn ensure_project_or_workspace(&self) -> anyhow::Result<PathBuf> {
        let root = self.project_or_workspace().to_path_buf();
        std::fs::create_dir_all(&root)?;
        Ok(root)
    }

    /// 从当前磁盘配置构造本次工具调用的不可变命令沙箱策略。
    pub fn command_sandbox_policy(&self) -> anyhow::Result<sandbox::SandboxPolicy> {
        let root = self.ensure_project_or_workspace()?;
        build_command_sandbox_policy_with_roots(
            &self.memory_dir,
            &root,
            &self.workspace_roots,
            self.permission_profile.as_deref(),
            self.workspace_write_grant,
            self.sandbox_policy.clone(),
        )
    }

    /// 返回当前调用实际生效的进程内网络授权。
    pub fn effective_in_process_network_grant(&self) -> InProcessNetworkGrant {
        let loaded = memory::load_permission_settings(&self.memory_dir);
        let profile_id = self
            .permission_profile
            .as_deref()
            .unwrap_or(&loaded.selection.profile_id);
        if profile_id == DANGER_FULL_ACCESS_PROFILE {
            InProcessNetworkGrant::unrestricted()
        } else {
            self.network_grant.clone()
        }
    }

    /// Prepare the managed network environment for a child process.
    ///
    /// Returns `None` when no managed proxy is active for this attempt, meaning
    /// the caller should not alter the child environment for network proxying.
    pub fn prepare_managed_network_env(
        &self,
        env: std::collections::HashMap<String, String>,
    ) -> Option<network_proxy::PreparedManagedNetwork> {
        self.managed_network
            .as_ref()
            .map(|started| started.proxy().prepare(env))
    }

    /// Drain the managed proxy's blocked-request queue and return the latest denial.
    ///
    /// The queue is drained per attempt; `.pop()` reports the most recent denial
    /// deterministically when a process made several blocked requests.
    pub fn take_managed_network_denial(&self) -> Option<types::NetworkPolicyDecisionPayload> {
        self.managed_network
            .as_ref()?
            .proxy()
            .take_blocked_requests()
            .pop()
            .map(|blocked| blocked.to_policy_decision_payload())
    }

    /// 当前调用的有效权限 profile id。
    pub fn active_permission_profile_id(&self) -> String {
        self.permission_profile.clone().unwrap_or_else(|| {
            memory::load_permission_settings(&self.memory_dir)
                .selection
                .profile_id
        })
    }

    /// 构造不包含命令正文或路径的沙箱审计上下文。
    pub fn sandbox_audit_metadata(&self, tool_name: &str) -> sandbox::SandboxAuditMetadata {
        sandbox::SandboxAuditMetadata::new(
            self.memory_dir.clone(),
            Some(self.session_id.clone()),
            self.turn_id.clone(),
            tool_name,
            self.active_permission_profile_id(),
        )
    }
}

/// 从当前权限配置构造不可变的子进程沙箱策略。
///
/// terminal、code_exec 与 MCP stdio 共用此入口，避免不同进程启动路径对 profile
/// 产生不同解释。`workspace_write_grant` 仅供单次已审批的工具调用使用；
/// `sandbox_policy` 由 orchestrator 为当前 attempt 选择；兼容入口传 `None` 时仍在此解析。
/// 常驻 MCP 连接必须传 `false, None`，不得继承临时授权。
pub fn build_command_sandbox_policy(
    memory_dir: &Path,
    execution_root: &Path,
    permission_profile: Option<&str>,
    workspace_write_grant: bool,
    sandbox_policy: Option<sandbox::SandboxPolicy>,
) -> anyhow::Result<sandbox::SandboxPolicy> {
    build_command_sandbox_policy_with_roots(
        memory_dir,
        execution_root,
        &[],
        permission_profile,
        workspace_write_grant,
        sandbox_policy,
    )
}

pub fn build_command_sandbox_policy_with_roots(
    memory_dir: &Path,
    execution_root: &Path,
    workspace_roots: &[PathBuf],
    permission_profile: Option<&str>,
    workspace_write_grant: bool,
    sandbox_policy: Option<sandbox::SandboxPolicy>,
) -> anyhow::Result<sandbox::SandboxPolicy> {
    if let Some(policy) = sandbox_policy {
        return Ok(policy);
    }
    std::fs::create_dir_all(execution_root)?;
    let loaded = memory::load_permission_settings(memory_dir);
    let profile_id = permission_profile.unwrap_or(&loaded.selection.profile_id);
    let mut mode = loaded.permissions.command_sandbox_mode_for(profile_id)?;
    if workspace_write_grant && mode == types::SandboxMode::ReadOnly {
        mode = types::SandboxMode::WorkspaceWrite;
    }
    let extra_roots: Vec<PathBuf> = workspace_roots
        .iter()
        .filter(|root| root.as_path() != execution_root)
        .cloned()
        .collect();
    sandbox::SandboxPolicy::new(mode, execution_root, extra_roots, false).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_only_custom_profile_uses_inherited_command_sandbox_mode() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(
            dir.path().join("config.yaml"),
            r#"
permissions:
  default_profile: network-only
  profiles:
    network-only:
      extends: ":workspace"
      network:
        enabled: true
        domains:
          example.com: allow
network_proxy:
  enabled: true
"#,
        )
        .unwrap();

        let policy =
            build_command_sandbox_policy(dir.path(), &workspace, Some("network-only"), false, None)
                .unwrap();

        assert_eq!(policy.mode, types::SandboxMode::WorkspaceWrite);
        assert!(!policy.network_access);
    }

    #[test]
    fn custom_filesystem_rules_still_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(
            dir.path().join("config.yaml"),
            r#"
permissions:
  default_profile: restricted-files
  profiles:
    restricted-files:
      extends: ":workspace"
      filesystem:
        paths:
          /tmp/secret: deny
"#,
        )
        .unwrap();

        let error = build_command_sandbox_policy(
            dir.path(),
            &workspace,
            Some("restricted-files"),
            false,
            None,
        )
        .unwrap_err();

        assert!(error.to_string().contains("not yet executable"), "{error}");
    }

    #[test]
    fn one_call_grant_upgrades_read_only_to_workspace_write_without_network() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        memory::set_permission_preset(dir.path(), types::PermissionPreset::ReadOnly).unwrap();
        let manager = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&manager.base_dir.join("sessions")).unwrap();
        let manager = std::sync::RwLock::new(manager);
        let targets = ImageGenTargets::default();
        let credentials = ModelCredentials::default();
        let ctx = ToolContext {
            memory: &manager,
            sessions: &sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: workspace.clone(),
            project_root: None,
            workspace_roots: Vec::new(),
            image_gen_targets: &targets,
            session_id: "test".into(),
            turn_id: None,
            credentials: &credentials,
            chat_targets: &[],
            execution: None,
            permission_profile: None,
            skill_config_overrides: &[],
            hook_bus: None,
            hook_runtime: None,
            workspace_write_grant: true,
            sandbox_policy: None,
            network_grant: InProcessNetworkGrant::default(),
            managed_network: None,
            context_window: None,
            context_tokens_used: None,
        };

        let policy = ctx.command_sandbox_policy().unwrap();
        assert_eq!(policy.mode, types::SandboxMode::WorkspaceWrite);
        assert_eq!(
            policy.writable_roots,
            vec![workspace.canonicalize().unwrap()]
        );
        assert!(!policy.network_access);
    }

    #[test]
    fn one_attempt_sandbox_override_expands_filesystem_without_network() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        memory::set_permission_preset(dir.path(), types::PermissionPreset::AskForApproval).unwrap();
        let override_policy =
            sandbox::SandboxPolicy::unrestricted_file_system(&workspace, false).unwrap();

        let policy = build_command_sandbox_policy(
            dir.path(),
            &workspace,
            None,
            false,
            Some(override_policy),
        )
        .unwrap();

        let workspace = workspace.canonicalize().unwrap();
        let filesystem_root = workspace.ancestors().last().unwrap().to_path_buf();
        assert_eq!(policy.mode, types::SandboxMode::WorkspaceWrite);
        assert_eq!(policy.writable_roots, vec![workspace, filesystem_root]);
        assert!(!policy.network_access);
    }

    #[test]
    fn selected_attempt_policy_wins_over_later_profile_change() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let selected = sandbox::SandboxPolicy::new(
            types::SandboxMode::ReadOnly,
            &workspace,
            Vec::new(),
            false,
        )
        .unwrap();

        let policy = build_command_sandbox_policy(
            dir.path(),
            &workspace,
            Some("profile-changed-after-selection"),
            false,
            Some(selected.clone()),
        )
        .unwrap();

        assert_eq!(policy, selected);
    }

    #[test]
    fn workspace_profile_materializes_all_project_roots() {
        let dir = tempfile::tempdir().unwrap();
        let primary = dir.path().join("primary");
        let secondary = dir.path().join("secondary");
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&secondary).unwrap();
        memory::set_permission_preset(dir.path(), types::PermissionPreset::ApproveForMe).unwrap();

        let policy = build_command_sandbox_policy_with_roots(
            dir.path(),
            &primary,
            &[primary.clone(), secondary.clone()],
            None,
            false,
            None,
        )
        .unwrap();

        assert_eq!(
            policy.writable_roots,
            vec![
                primary.canonicalize().unwrap(),
                secondary.canonicalize().unwrap()
            ]
        );
    }

    #[test]
    fn from_parts_writes_primary_music_model_and_leaves_fallback_music_model_empty() {
        let targets = image_gen_targets_from_parts(ImageGenParts {
            provider: "google",
            model: "image-model",
            api_key: "primary-key",
            base_url: "primary-base",
            fb_provider: "openai",
            fb_model: "fallback-image-model",
            fb_api_key: "fallback-key",
            fb_base_url: "fallback-base",
            video_model: "",
            music_model: "  configured-music-model  ",
            tts_model: "",
            fb_video_model: "",
            fb_music_model: "",
            fb_tts_model: "",
            vision_model: "",
            fb_vision_model: "",
        });

        let primary = targets.primary.as_ref().expect("primary credentials");
        assert_eq!(primary.music_model, "configured-music-model");
        let fallback = targets.fallback.as_ref().expect("fallback credentials");
        assert_eq!(fallback.music_model, "");
    }
}
