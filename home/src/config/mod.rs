//! 本机配置层：图标、工具开关、内容安全扫描——无 SQLite。

pub mod agent_icons;
pub mod auto_icon;
pub mod scan;
pub mod tools_enabled;

pub use agent_icons::{
    apply_pending_agent_icons, clear_pending_agent_icon, pending_icons_dir, resolve_icon_field,
    set_pending_agent_icon, update_agent_icons, write_agent_icon, AgentIconKind,
};
pub use auto_icon::{
    apply_auto_lucide_icon, lucide_svg_bytes, suggest_lucide_icon_id, AutoLucideIcon,
    AUTO_LUCIDE_ICONS,
};
pub use scan::scan_memory_content;
pub use tools_enabled::{
    is_tool_call_allowed, is_toolset_enabled, load_tools_enabled, load_tools_enabled_for_agent,
    save_tools_enabled, save_tools_enabled_for_agent, sync_tools_enabled_defaults,
    sync_tools_enabled_defaults_for_agent, tool_name_to_toolset, tools_enabled_path,
    KNOWN_TOOLSET_IDS,
};
