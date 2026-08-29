//! 工具统一分发：按名称从自注册 handler 表查找并执行。
//!
//! 所有 Agent 侧的工具执行均经 [`dispatch_tool`] 入口，确保禁用工具、
//! 调用统计与错误格式保持一致。中央 match 已移除；内置路由由
//! [`crate::registry::BuiltinToolRegistrar`] inventory 构建。

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::context::ToolContext;
use crate::registry::{BuiltinToolHandler, BuiltinToolRegistrar};

/// 从 inventory 构建的内置工具 name → handler 表（启动时检测重名）。
fn handler_table() -> &'static HashMap<&'static str, BuiltinToolHandler> {
    static TABLE: OnceLock<HashMap<&'static str, BuiltinToolHandler>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut map = HashMap::new();
        for hook in inventory::iter::<BuiltinToolRegistrar> {
            for &name in hook.names {
                if map.insert(name, hook.handler).is_some() {
                    panic!("duplicate builtin tool handler name: {name}");
                }
            }
        }
        map
    })
}

/// 当前已注册的内置 handler 名称（测试 / 观测用）。
pub fn builtin_handler_names() -> Vec<&'static str> {
    let mut names: Vec<_> = handler_table().keys().copied().collect();
    names.sort_unstable();
    names
}

/// 按工具名将调用路由到对应实现（内置 + 动态 MCP 统一入口）。
///
/// # 流程
/// 1. 通过 `registry_allows` 闭包检查 toolset 是否启用
/// 2. 记账（审计日志 + 用量统计）
/// 3. 内置 handler 表查找 → 动态 handler 查找 → Skill soft-alias → 未知工具
pub async fn dispatch_tool(
    registry_allows: impl Fn(&str) -> bool,
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
    dynamic_handler: Option<&crate::registry::DynToolHandler>,
) -> anyhow::Result<types::ToolOutput> {
    if !registry_allows(name) {
        let toolset = home::tool_name_to_toolset(name);
        anyhow::bail!("工具已禁用（tools-enabled.json → {toolset}=false）: {name}");
    }

    let agent_id = ctx.agent_id();
    let _ = home::record_tool_call(&agent_id, name, args);
    let _ = usage::record_tool_call(
        &agent_id,
        name,
        args,
        Some(ctx.session_id.as_str()),
        ctx.turn_id.as_deref(),
    );
    enforce_in_process_write_policy(ctx, name, args)?;

    // 1. 内置 handler（静态 inventory 注册）
    if let Some(handler) = handler_table().get(name) {
        return handler(ctx, name, args).await;
    }

    // 2. 动态 handler（MCP 工具等运行时注册）
    if let Some(dyn_handler) = dynamic_handler {
        return dyn_handler(name, args).await;
    }

    // 3. Skill soft-alias
    if home::is_tool_call_allowed("skills")
        && skills::list_installed()
            .into_iter()
            .any(|s| s.name == name && s.enabled)
    {
        let rewritten = serde_json::json!({
            "action": "load",
            "skill_id": name,
            "input": args,
        });
        if let Some(handler) = handler_table().get("skills") {
            return handler(ctx, "skills", &rewritten).await;
        }
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
        "cron" => matches!(action().as_str(), "add" | "remove" | "enable" | "disable"),
        "image_gen" | "video_gen" | "speech_gen" | "music_gen" => true,
        _ => false,
    }
}

#[cfg(test)]
mod permission_tests {
    use super::*;
    use crate::context::ImageGenTargets;

    fn with_ctx(dir: &tempfile::TempDir, write_grant: bool, f: impl FnOnce(&ToolContext<'_>)) {
        let manager = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&manager.base_dir.join("data")).unwrap();
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
            chat_targets: &[],
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
            "cron",
            &serde_json::json!({"action": "disable"})
        ));
        assert!(!tool_requires_in_process_write(
            "cron",
            &serde_json::json!({"action": "list"})
        ));
        assert!(tool_requires_in_process_write(
            "image_gen",
            &serde_json::json!({})
        ));
    }

    #[test]
    fn read_only_denies_and_workspace_allows_in_process_writes() {
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
        });
        with_ctx(&dir, true, |ctx| {
            assert!(enforce_in_process_write_policy(
                ctx,
                "todo",
                &serde_json::json!({"action": "create"})
            )
            .is_ok());
        });

        memory::set_permission_preset(dir.path(), types::PermissionPreset::AskForApproval).unwrap();
        with_ctx(&dir, false, |ctx| {
            assert!(enforce_in_process_write_policy(
                ctx,
                "todo",
                &serde_json::json!({"action": "create"})
            )
            .is_ok());
        });
    }
}
