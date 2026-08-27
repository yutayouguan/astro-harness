//! Agent 工具集开关的持久化与运行时校验。
//!
//! 全局配置位于 `~/.astro/tools-enabled.json`；各 Agent 可覆写于
//! `AgentRuntimeConfig.tools_enabled`。缺失条目默认启用（`true`）。
//! 工具调用前通过 [`is_tool_call_allowed`] 检查对应工具集是否开启。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::{
    active_agent_id, agent_workspace_dir, default_memory_dir, ensure_default_workspace_dirs,
    AgentRuntimeConfig, DEFAULT_AGENT_ID,
};

/// 与前端 `AGENT_TOOLS` id 对齐的已知工具集标识列表。
pub const KNOWN_TOOLSET_IDS: &[&str] = &[
    "web_search",
    "exec_command",
    "apply_patch",
    "code_exec",
    "image_analyze",
    "robotics",
    "audio_analyze",
    "image_gen",
    "video_gen",
    "video_analyze",
    "speech_gen",
    "music_gen",
    "skills",
    "memory",
    "context_search",
    "pin_context",
    "ask_user",
    "send_user_message_async",
    "switch_mode",
    "present",
    "subagents",
    "cron",
    "persona",
    "todo",
];

/// 全局工具开关配置文件路径（`~/.astro/tools-enabled.json`）。
pub fn tools_enabled_path() -> PathBuf {
    default_memory_dir().join("tools-enabled.json")
}

/// 规范化 Agent 键：去空白、`"default"` 映射为 [`DEFAULT_AGENT_ID`]；空则返回 `None`。
fn normalize_agent_key(agent_id: Option<&str>) -> Option<String> {
    agent_id.map(str::trim).filter(|s| !s.is_empty()).map(|s| {
        if s == "default" {
            DEFAULT_AGENT_ID.to_string()
        } else {
            s.to_string()
        }
    })
}

/// 读取全局工具开关；文件不存在或解析失败时返回空 map。
pub fn load_tools_enabled() -> HashMap<String, bool> {
    let path = tools_enabled_path();
    if !path.exists() {
        return HashMap::new();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// 原子写入全局工具开关（临时文件 + `rename`）。
pub fn save_tools_enabled(state: &HashMap<String, bool>) -> anyhow::Result<()> {
    ensure_default_workspace_dirs()?;
    let path = tools_enabled_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(state)?)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// 从 JSON 对象提取 `string → bool` 映射；非对象类型返回空 map。
fn map_from_json(value: &serde_json::Value) -> HashMap<String, bool> {
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b)))
            .collect(),
        _ => HashMap::new(),
    }
}

/// 读取指定 Agent 的工具开关；无专属配置时回退全局 [`load_tools_enabled`]。
pub fn load_tools_enabled_for_agent(agent_id: Option<&str>) -> HashMap<String, bool> {
    let base = default_memory_dir();
    if let Some(id) = normalize_agent_key(agent_id) {
        if let Ok(cfg) = AgentRuntimeConfig::load(&base, &id) {
            if let Some(ref tools) = cfg.tools_enabled {
                return map_from_json(tools);
            }
        }
    }
    load_tools_enabled()
}

/// 写入指定 Agent 的工具开关；`agent_id` 为 `None` 时写全局配置。
///
/// 默认 Agent 同时写入全局 `tools-enabled.json`，供无 Agent 专属配置时的回退读取。
pub fn save_tools_enabled_for_agent(
    agent_id: Option<&str>,
    state: &HashMap<String, bool>,
) -> anyhow::Result<()> {
    ensure_default_workspace_dirs()?;
    let base = default_memory_dir();
    if let Some(id) = normalize_agent_key(agent_id) {
        let mut cfg = AgentRuntimeConfig::load(&base, &id).unwrap_or_else(|_| {
            let name = if id == DEFAULT_AGENT_ID {
                "Astro".to_string()
            } else {
                id.clone()
            };
            AgentRuntimeConfig {
                id: id.clone(),
                name,
                inherit_from: None,
                provider_id: None,
                model: None,
                temperature: None,
                max_turns: None,
                additional_params: None,
                tools_enabled: None,
                created_at: chrono::Local::now().to_rfc3339(),
            }
        });
        cfg.tools_enabled = Some(serde_json::to_value(state)?);
        cfg.save(&base)?;
        if id == DEFAULT_AGENT_ID {
            save_tools_enabled(state)?;
        }
        return Ok(());
    }
    save_tools_enabled(state)
}

/// 确保全局已知工具集均有配置条目（缺失默认 `true`），必要时写盘。
pub fn sync_tools_enabled_defaults() -> anyhow::Result<HashMap<String, bool>> {
    sync_tools_enabled_defaults_for_agent(None)
}

/// 确保指定 Agent 的已知工具集均有配置条目（缺失默认 `true`），必要时写盘。
pub fn sync_tools_enabled_defaults_for_agent(
    agent_id: Option<&str>,
) -> anyhow::Result<HashMap<String, bool>> {
    ensure_default_workspace_dirs()?;
    let mut state = load_tools_enabled_for_agent(agent_id);
    let mut dirty = false;
    for id in KNOWN_TOOLSET_IDS {
        if !state.contains_key(*id) {
            state.insert((*id).to_string(), true);
            dirty = true;
        }
    }
    if dirty {
        save_tools_enabled_for_agent(agent_id, &state)?;
    }
    Ok(state)
}

/// 检查工具集是否启用；读取当前活跃 Agent 配置，缺失条目视为 `true`。
pub fn is_toolset_enabled(toolset: &str) -> bool {
    let base = default_memory_dir();
    let agent = active_agent_id(&base);
    load_tools_enabled_for_agent(Some(&agent))
        .get(toolset)
        .copied()
        .unwrap_or(true)
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
        "cron" | "cron_add" | "cron_list" | "cron_remove" | "cron_enable" | "cron_disable"
        | "cron.add" | "cron.list" | "cron.remove" | "cron.enable" | "cron.disable" => "cron",
        "image_gen" => "image_gen",
        "video_gen" => "video_gen",
        "video_analyze" | "video_understand" => "video_analyze",
        "exec_command" | "write_stdin" | "request_permissions" => "exec_command",
        "apply_patch" => "apply_patch",
        "web_search" | "web_fetch" | "web_extract" | "http_fetch" => "web_search",
        "code_exec" => "code_exec",
        "image_analyze" | "image_understand" => "image_analyze",
        "robotics" => "robotics",
        "audio_analyze" | "audio_understand" => "audio_analyze",
        "speech_gen" | "tts" => "speech_gen",
        "music_gen" => "music_gen",
        "skills" => "skills",
        "ask_user" => "ask_user",
        "send_user_message_async" => "send_user_message_async",
        "switch_mode" => "switch_mode",
        "present" => "present",
        "spawn_agent" | "list_agents" | "followup_task" | "send_message" | "wait_agent"
        | "interrupt_agent" => "subagents",
        "persona_create" => "persona",
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
    fn non_default_agent_tools_enabled_overrides_global() {
        let dir = TempDir::new().unwrap();
        let _env = crate::test_env::AstroMemoryDirGuard::set(dir.path());

        let mut global = HashMap::new();
        global.insert("memory".into(), true);
        save_tools_enabled(&global).unwrap();

        let mut agent = HashMap::new();
        agent.insert("memory".into(), false);
        save_tools_enabled_for_agent(Some("other"), &agent).unwrap();

        let loaded = load_tools_enabled_for_agent(Some("other"));
        assert_eq!(loaded.get("memory"), Some(&false));

        let raw = fs::read_to_string(dir.path().join("tools-enabled.json")).unwrap();
        let map: HashMap<String, bool> = serde_json::from_str(&raw).unwrap();
        assert_eq!(map.get("memory"), Some(&true));
    }
}
