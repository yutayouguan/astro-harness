//! Agent 工具集开关的持久化与运行时校验。
//!
//! 全局配置位于 `config.toml [desktop.tools]`；各 Agent 可覆写于
//! `AgentRuntimeConfig.tools_enabled`。缺失条目默认启用（`true`）。
//! 工具调用前通过 [`is_tool_call_allowed`] 检查对应工具集是否开启。

use std::collections::HashMap;
use std::path::PathBuf;

use crate::{
    active_agent_id, agent_workspace_dir, default_memory_dir, ensure_default_workspace_dirs,
};

/// 与前端 `AGENT_TOOLS` id 对齐的已知工具集标识列表。
pub const KNOWN_TOOLSET_IDS: &[&str] = &[
    "web_search",
    "browser",
    "exec_command",
    "apply_patch",
    "code_exec",
    "image_analyze",
    "robotics",
    "audio_analyze",
    "image_gen",
    "desktop_pet",
    "ui_style",
    "video_gen",
    "video_analyze",
    "speech_gen",
    "music_gen",
    "skills",
    "memory",
    "context_search",
    "pin_context",
    "ask_user",
    "request_user_input_async",
    "switch_mode",
    "present",
    "subagents",
    "cron",
    "workflow",
    "persona",
    "todo",
];

/// Global editable tool gates live in config.toml [desktop.tools].
pub fn tools_enabled_path() -> PathBuf {
    crate::settings::path(&default_memory_dir())
}

pub fn load_tools_enabled() -> anyhow::Result<HashMap<String, bool>> {
    Ok(crate::settings::read(&default_memory_dir(), &["desktop", "tools"])?.unwrap_or_default())
}

pub fn load_tools_enabled_for_agent(
    agent_id: Option<&str>,
) -> anyhow::Result<HashMap<String, bool>> {
    let base = default_memory_dir();
    if let Some(id) = agent_id {
        let id = crate::settings::migration::canonical_agent_id(id);
        if let Some(tools) =
            crate::settings::read(&base, &["desktop", "agents", &id, "tools_enabled"])?
        {
            return Ok(tools);
        }
    }
    load_tools_enabled()
}

fn edit_tools(
    agent_id: Option<&str>,
    supplied: &HashMap<String, bool>,
    replace: bool,
    fill_defaults: bool,
) -> anyhow::Result<HashMap<String, bool>> {
    ensure_default_workspace_dirs()?;
    let base = default_memory_dir();
    let id = agent_id.map(crate::settings::migration::canonical_agent_id);
    crate::settings::update(&base, |doc| {
        let mut state: HashMap<String, bool> = if replace {
            HashMap::new()
        } else {
            let specific = id
                .as_ref()
                .map(|id| crate::settings::get(doc, &["desktop", "agents", id, "tools_enabled"]))
                .transpose()?
                .flatten();
            specific
                .or(crate::settings::get(doc, &["desktop", "tools"])?)
                .unwrap_or_default()
        };
        state.extend(supplied.clone());
        if fill_defaults {
            for key in KNOWN_TOOLSET_IDS {
                state.entry((*key).into()).or_insert(true);
            }
        }
        if let Some(id) = id.as_ref() {
            let mut config = crate::settings::get::<serde_json::Value>(doc, &["desktop", "agents", id])?
                .unwrap_or_else(|| serde_json::json!({"id":id,"name":if id == "default" { "Astro" } else { id.as_str() }, "created_at":chrono::Local::now().to_rfc3339()}));
            config["tools_enabled"] = serde_json::to_value(&state)?;
            crate::settings::put(doc, &["desktop", "agents", id], &config)?;
            if id == "default" {
                crate::settings::put(doc, &["desktop", "tools"], &state)?;
            }
        } else {
            crate::settings::put(doc, &["desktop", "tools"], &state)?;
        }
        Ok(state)
    })
}

pub fn save_tools_enabled(state: &HashMap<String, bool>) -> anyhow::Result<()> {
    edit_tools(None, state, true, false).map(|_| ())
}
pub fn save_tools_enabled_for_agent(
    agent_id: Option<&str>,
    state: &HashMap<String, bool>,
) -> anyhow::Result<()> {
    edit_tools(agent_id, state, true, false).map(|_| ())
}

/// UI updates only supplied keys; read/merge/write is one transaction.
pub fn patch_tools_enabled_for_agent(
    agent_id: Option<&str>,
    state: &HashMap<String, bool>,
) -> anyhow::Result<()> {
    edit_tools(agent_id, state, false, true).map(|_| ())
}

pub fn sync_tools_enabled_defaults() -> anyhow::Result<HashMap<String, bool>> {
    sync_tools_enabled_defaults_for_agent(None)
}

pub fn sync_tools_enabled_defaults_for_agent(
    agent_id: Option<&str>,
) -> anyhow::Result<HashMap<String, bool>> {
    let mut state = load_tools_enabled_for_agent(agent_id)?;
    for key in KNOWN_TOOLSET_IDS {
        state.entry((*key).into()).or_insert(true);
    }
    Ok(state)
}

pub fn is_toolset_enabled(toolset: &str) -> bool {
    let base = default_memory_dir();
    let agent = active_agent_id(&base);
    match load_tools_enabled_for_agent(Some(&agent)) {
        Ok(state) => state.get(toolset).copied().unwrap_or(true),
        Err(error) => {
            tracing::warn!(%error, "tool configuration unavailable; denying toolset");
            false
        }
    }
}

/// 将具体工具调用名映射为工具集 id（与前端开关及 [`KNOWN_TOOLSET_IDS`] 对应）。
pub fn tool_name_to_toolset(name: &str) -> &str {
    if name.starts_with("mcp__") {
        return "mcp";
    }
    match name {
        "mcp_resources" | "mcp_prompts" => "mcp",
        "memory" => "memory",
        "context_search" => "context_search",
        "pin_context" => "pin_context",
        "notes" | "history" | "get_context_remaining" | "new_context_window" => "system",
        "cron" | "cron_add" | "cron_list" | "cron_remove" | "cron_enable" | "cron_disable"
        | "cron.add" | "cron.list" | "cron.remove" | "cron.enable" | "cron.disable" => "cron",
        "image_gen" => "image_gen",
        "desktop_pet" => "desktop_pet",
        "ui_style" => "ui_style",
        "video_gen" => "video_gen",
        "video_analyze" | "video_understand" => "video_analyze",
        "exec_command" | "write_stdin" | "request_permissions" => "exec_command",
        "apply_patch" => "apply_patch",
        "web_search" | "web_fetch" | "web_extract" | "http_fetch" => "web_search",
        "browser_open" | "browser_snapshot" | "browser_click" | "browser_type"
        | "browser_scroll" | "browser_wait" | "browser_screenshot" | "browser_close" => "browser",
        "code_exec" => "code_exec",
        "image_analyze" | "image_understand" => "image_analyze",
        "robotics" => "robotics",
        "audio_analyze" | "audio_understand" => "audio_analyze",
        "speech_gen" | "tts" => "speech_gen",
        "music_gen" => "music_gen",
        "skills" => "skills",
        "ask_user" => "ask_user",
        "request_user_input_async" => "request_user_input_async",
        "switch_mode" => "switch_mode",
        "present" => "present",
        "spawn_agent" | "list_agents" | "followup_task" | "send_message" | "wait_agent"
        | "interrupt_agent" => "subagents",
        "persona_create" => "persona",
        name if name.starts_with("workflow__") || name.starts_with("workflow.") => "workflow",
        "todo" => "todo",
        other => other,
    }
}

/// 根据工具名解析工具集并检查是否允许调用。
pub fn is_tool_call_allowed(name: &str) -> bool {
    is_toolset_enabled(tool_name_to_toolset(name))
}

/// 返回指定 Agent 的工作区目录路径；供 MCP 配置读写等场景使用。
pub fn agent_workspace_for(agent_id: &str) -> std::path::PathBuf {
    agent_workspace_dir(&default_memory_dir(), agent_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn missing_defaults_to_enabled() {
        let dir = TempDir::new().unwrap();
        let _env = crate::test_env::AstroMemoryDirGuard::set(dir.path());
        assert!(is_toolset_enabled("memory"));
        assert!(is_tool_call_allowed("memory"));
    }

    #[test]
    fn disabled_toolset_blocks_calls() {
        let dir = TempDir::new().unwrap();
        let _env = crate::test_env::AstroMemoryDirGuard::set(dir.path());
        let mut state = HashMap::new();
        state.insert("memory".into(), false);
        state.insert("cron".into(), true);
        save_tools_enabled(&state).unwrap();
        assert!(!is_tool_call_allowed("memory"));
        assert!(is_tool_call_allowed("cron"));
    }

    #[test]
    fn web_fetch_maps_to_web_search_toolset() {
        assert_eq!(tool_name_to_toolset("web_fetch"), "web_search");
        assert_eq!(tool_name_to_toolset("web_extract"), "web_search");
        assert_eq!(tool_name_to_toolset("http_fetch"), "web_search");
        assert_eq!(tool_name_to_toolset("web_search"), "web_search");
        let dir = TempDir::new().unwrap();
        let _env = crate::test_env::AstroMemoryDirGuard::set(dir.path());
        let mut state = HashMap::new();
        state.insert("web_search".into(), false);
        save_tools_enabled(&state).unwrap();
        assert!(!is_tool_call_allowed("web_fetch"));
    }

    #[test]
    fn mcp_brokers_share_the_mcp_toolset_gate() {
        assert_eq!(tool_name_to_toolset("mcp_resources"), "mcp");
        assert_eq!(tool_name_to_toolset("mcp_prompts"), "mcp");
    }

    #[test]
    fn async_user_input_uses_the_canonical_toolset() {
        assert_eq!(
            tool_name_to_toolset("request_user_input_async"),
            "request_user_input_async"
        );
    }

    #[test]
    fn non_default_agent_tools_enabled_overrides_global() {
        let dir = TempDir::new().unwrap();
        let _env = crate::test_env::AstroMemoryDirGuard::set(dir.path());

        let mut global = HashMap::new();
        global.insert("memory".into(), true);
        save_tools_enabled(&global).unwrap();

        let mut agent = HashMap::new();
        agent.insert("memory".into(), false);
        save_tools_enabled_for_agent(Some("other"), &agent).unwrap();

        let loaded = load_tools_enabled_for_agent(Some("other")).unwrap();
        assert_eq!(loaded.get("memory"), Some(&false));

        let map = load_tools_enabled().unwrap();
        assert_eq!(map.get("memory"), Some(&true));
    }

    #[test]
    fn workflow_names_share_the_workflow_toolset() {
        assert_eq!(tool_name_to_toolset("workflow__saved-id"), "workflow");
        assert_eq!(tool_name_to_toolset("workflow.weekly_report"), "workflow");
        assert!(KNOWN_TOOLSET_IDS.contains(&"workflow"));
    }

    #[test]
    fn ui_style_has_a_dedicated_toolset_gate() {
        assert_eq!(tool_name_to_toolset("ui_style"), "ui_style");
        assert!(KNOWN_TOOLSET_IDS.contains(&"ui_style"));
    }

    #[test]
    fn desktop_pet_has_a_dedicated_toolset_gate() {
        assert_eq!(tool_name_to_toolset("desktop_pet"), "desktop_pet");
        assert!(KNOWN_TOOLSET_IDS.contains(&"desktop_pet"));
    }
}
