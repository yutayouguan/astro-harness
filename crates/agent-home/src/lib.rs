//! Astro 本机根（默认 `~/.astro` / `ASTRO_MEMORY_DIR`）——无 SQLite 依赖。
//!
//! 提供路径解析、日志以及配置层（图标、工具开关、内容扫描）。
//! 供不需要持久化存储的 crate（如 `mcp`）直接依赖，避免引入 SQLite。

pub mod cache;
pub mod config;
pub mod config_file;
pub mod infra;
pub mod layout;
pub mod settings;
pub mod storage_cleanup;
pub mod storage_diagnostics;
pub mod test_env;
pub mod workspace;

// ─── flat re-exports ─────────────────────────────────────────────────────────

pub use layout::*;
pub use workspace::*;

pub use infra::log_query::{
    default_agent_log_query, query_agent_logs, AgentLogLine, AgentLogQuery, LogSource,
};
pub use infra::logging::{init_logging, logs_dir};
pub use infra::tool_calls::record_tool_call;

pub use config::{
    apply_auto_lucide_icon, apply_pending_agent_icons, clear_pending_agent_icon,
    is_tool_call_allowed, is_toolset_enabled, load_tool_loading_modes, load_tools_enabled,
    load_tools_enabled_for_agent, lucide_svg_bytes, patch_tools_enabled_for_agent,
    pending_icons_dir, resolve_icon_field, save_tools_enabled, save_tools_enabled_for_agent,
    scan_memory_content, set_pending_agent_icon, set_tool_loading_mode, suggest_lucide_icon_id,
    sync_tools_enabled_defaults, sync_tools_enabled_defaults_for_agent, tool_name_to_toolset,
    tools_enabled_path, update_agent_icons, write_agent_icon, AgentIconKind, AutoLucideIcon,
    ToolLoadingMode, AUTO_LUCIDE_ICONS, KNOWN_TOOLSET_IDS,
};
