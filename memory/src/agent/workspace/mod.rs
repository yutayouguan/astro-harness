//! Agent 工作区：路径 / 脚手架来自 `home`；完整引导含会话库与技能播种。

mod lifecycle;

pub use home::{
    active_agent_id, agent_config_dir, agent_id_from_workspace_dir_name, agent_workspace_dir,
    create_agent, create_agent_with_profile, daily_memory_path, default_agent_workspace_dir,
    default_memory_dir, ensure_agent_space, ensure_daily_memory, generated_dir,
    list_agents, list_daily_memory_dates, normalize_agent_id, set_active_agent, today_date_string,
    write_agent_config, AgentInfo, AgentProfile, AgentRuntimeConfig, EnsureWorkspaceReport,
    GeneratedKind, GENERATED_SUBDIRS, DEFAULT_AGENT_ID,
};
pub use lifecycle::{
    ensure_default_workspace, ensure_workspace, seed_create_agent_skill,
};
