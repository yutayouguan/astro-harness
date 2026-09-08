//! 工具执行的通用记账、权限与分发逻辑。

use std::sync::Arc;

use crate::context::ToolContext;
use crate::engine::executor::CoreToolRuntime;

/// 执行已由 [`crate::registry::ToolRegistry`] 解析的运行时。
///
/// 这是 `ToolRegistry::dispatch` 调用的唯一记账、权限与执行入口。
pub(crate) async fn dispatch_runtime(
    allowed: bool,
    runtime: Option<&Arc<dyn CoreToolRuntime>>,
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<types::ToolOutput> {
    if !allowed {
        let toolset = home::tool_name_to_toolset(name);
        anyhow::bail!("工具已禁用（tools/enabled.json → {toolset}=false）: {name}");
    }

    let agent_id = ctx.agent_id();
    let _ = home::record_tool_call(&agent_id, name, args);
    let _ = usage::record_tool_call(
        &agent_id,
        name,
        args,
        Some(ctx.session_id.as_str()),
        ctx.turn_id.as_deref(),
    )
    .await;
    enforce_in_process_write_policy(ctx, name, args)?;

    if let Some(runtime) = runtime {
        return runtime.handle(ctx, args).await;
    }

    anyhow::bail!("未知工具: {name}")
}

fn enforce_in_process_write_policy(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<()> {
    if !tool_requires_in_process_write(name, args) {
        return Ok(());
    }
    let settings = memory::load_permission_settings(&ctx.memory_dir);
    let profile = ctx
        .permission_profile
        .as_deref()
        .unwrap_or(&settings.selection.profile_id);
    match profile {
        types::READ_ONLY_PROFILE if ctx.workspace_write_grant => Ok(()),
        types::READ_ONLY_PROFILE => anyhow::bail!(
            "permission denied: read-only profile does not allow {name} to modify local state"
        ),
        types::WORKSPACE_PROFILE | types::DANGER_FULL_ACCESS_PROFILE => Ok(()),
        custom => anyhow::bail!(
            "custom permission profile {custom:?} is not executable until its filesystem rules are fully resolved"
        ),
    }
}

pub fn tool_requires_in_process_write(name: &str, args: &serde_json::Value) -> bool {
    let action = || {
        args.get("action")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    };
    match name {
        "memory" | "todo" | "persona_create" => true,
        "skills" => action() == "manage",
        "pin_context" => matches!(action().as_str(), "pin" | "unpin" | "clear"),
        "cron_add" | "cron_remove" | "cron_enable" | "cron_disable" => true,
        "image_gen" | "video_gen" | "speech_gen" | "music_gen" => true,
        "desktop_pet" => action() != "status",
        _ => false,
    }
}

#[cfg(test)]
mod permission_tests {
    use super::*;
    use crate::context::ImageGenTargets;

    async fn with_ctx(
        dir: &tempfile::TempDir,
        write_grant: bool,
        f: impl FnOnce(&ToolContext<'_>),
    ) {
        let manager = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&manager.base_dir.join("sessions"))
            .await
            .unwrap();
        let manager = std::sync::RwLock::new(manager);
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let ctx = ToolContext {
            memory: &manager,
            sessions: &sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: dir.path().join("workspace"),
            project_root: None,
            workspace_roots: Vec::new(),
            image_gen_targets: &targets,
            session_id: "test".into(),
            turn_id: None,
            credentials: &creds,
            service_tier: None,
            model_targets: &[],
            execution: None,
            permission_profile: None,
            skill_config_overrides: &[],
            hook_bus: None,
            hook_runtime: None,
            workspace_write_grant: write_grant,
            sandbox_policy: None,
            managed_network: None,
            context_window: None,
            context_tokens_used: None,
            tool_registry: None,
        };
        f(&ctx);
    }

    #[test]
    fn classifies_in_process_writes_without_blocking_read_actions() {
        assert!(tool_requires_in_process_write(
            "skills",
            &serde_json::json!({"action": "manage"})
        ));
        assert!(!tool_requires_in_process_write(
            "skills",
            &serde_json::json!({"action": "load"})
        ));
        assert!(tool_requires_in_process_write(
            "pin_context",
            &serde_json::json!({"action": "clear"})
        ));
        assert!(!tool_requires_in_process_write(
            "pin_context",
            &serde_json::json!({"action": "list"})
        ));
        assert!(tool_requires_in_process_write(
            "cron_disable",
            &serde_json::json!({})
        ));
        assert!(!tool_requires_in_process_write(
            "cron_list",
            &serde_json::json!({})
        ));
        assert!(tool_requires_in_process_write(
            "image_gen",
            &serde_json::json!({})
        ));
        assert!(!tool_requires_in_process_write(
            "desktop_pet",
            &serde_json::json!({"action": "status"})
        ));
        assert!(tool_requires_in_process_write(
            "desktop_pet",
            &serde_json::json!({"action": "apply"})
        ));
    }

    #[tokio::test]
    async fn read_only_denies_and_workspace_allows_in_process_writes() {
        let dir = tempfile::tempdir().unwrap();
        memory::set_permission_preset(dir.path(), types::PermissionPreset::ReadOnly).unwrap();
        with_ctx(&dir, false, |ctx| {
            let error = enforce_in_process_write_policy(
                ctx,
                "todo",
                &serde_json::json!({"action": "create"}),
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("read-only"), "{error}");
            assert!(
                enforce_in_process_write_policy(ctx, "context_search", &serde_json::json!({}))
                    .is_ok()
            );
        })
        .await;
        with_ctx(&dir, true, |ctx| {
            assert!(enforce_in_process_write_policy(
                ctx,
                "todo",
                &serde_json::json!({"action": "create"})
            )
            .is_ok());
        })
        .await;

        memory::set_permission_preset(dir.path(), types::PermissionPreset::AskForApproval).unwrap();
        with_ctx(&dir, false, |ctx| {
            assert!(enforce_in_process_write_policy(
                ctx,
                "todo",
                &serde_json::json!({"action": "create"})
            )
            .is_ok());
        })
        .await;
    }
}
