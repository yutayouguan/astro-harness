//! Astro 记忆子系统：Agent 工作区、会话持久化、工具开关与用量统计的统一入口。
//!
//! 本 crate 将 Markdown 记忆文件（`MEMORY.md`、`USER.md`、每日记忆）、SQLite 消息/会话库、
//! 工具启用配置、调用计数、定时任务与图标资源等能力聚合为可被 agent 与前端调用的 API。
//! 数据根目录默认为 `~/.astro`（可通过环境变量覆盖，见 `workspace` 模块）。

pub mod artifact_db;
pub mod files;
pub mod message_db;
pub mod session_db;
pub mod manager;
pub mod workspace;
pub mod agent_icons;
pub mod logging;
pub mod cron;
pub mod cron_run_db;
pub mod tools_enabled;
pub mod tool_calls;
pub mod dreaming;
pub mod usage_stats;
pub mod usage_db;
pub mod usage_pricing;
pub mod orchestration_db;
pub mod orchestration_spawn;
pub mod collab_insights;

pub use artifact_db::{
    artifacts_db_path, category_from_name, is_junk_artifact_name, open_default, ArtifactDb,
    ArtifactRow, ArtifactSource, ReconcileReport,
};
pub use manager::{
    dispatch_memory_tool, format_recalled_context, MemoryManager, MemoryTarget,
};
pub use message_db::{build_conversation_context, MessageDb, ScrolledMessage};
pub use session_db::{SessionDb, SessionSnippet};
pub use workspace::{
    active_agent_id, agent_config_dir, agent_id_from_workspace_dir_name, agent_workspace_dir,
    create_agent, create_agent_with_profile, daily_memory_path, default_agent_workspace_dir,
    default_memory_dir, default_workspace_dir, ensure_agent_space, ensure_daily_memory,
    ensure_default_workspace, ensure_workspace, list_agents, list_daily_memory_dates,
    normalize_agent_id, seed_create_agent_skill, set_active_agent, today_date_string,
    write_agent_config, AgentInfo, AgentProfile, AgentRuntimeConfig, EnsureWorkspaceReport,
    DEFAULT_AGENT_ID,
};
pub use agent_icons::{
    apply_pending_agent_icons, clear_pending_agent_icon, resolve_icon_field, set_pending_agent_icon,
    update_agent_icons, write_agent_icon, AgentIconKind,
};
pub use logging::{init_logging, logs_dir};
pub use cron::{
    cron_dir, cron_extract_preamble, dispatch_cron_tool, normalize_cron_extract, tick_default,
    CronJob, CronJobExtract, CronStore, NewCronJob,
};
pub use cron_run_db::{
    cron_db_path, CronRunDb, CronRunFilters, CronRunRow, NewCronRun,
};
pub use tools_enabled::{
    is_tool_call_allowed, is_toolset_enabled, load_tools_enabled, load_tools_enabled_for_agent,
    save_tools_enabled, save_tools_enabled_for_agent, sync_tools_enabled_defaults,
    sync_tools_enabled_defaults_for_agent, tool_name_to_toolset, tools_enabled_path,
    KNOWN_TOOLSET_IDS,
};
pub use tool_calls::record_tool_call;
pub use dreaming::{
    load_dreaming_state, prepare_all_dream_jobs, save_dreaming_state, set_dreaming_enabled,
    DreamMemoryUpdate, DreamRunReport, DreamingState,
};
pub use usage_stats::{
    get_usage_summary, load_usage_stats, record_tool_call as record_usage_tool_call,
    save_usage_stats, AgentUsageStats, AgentUsageSummary,
};
pub use usage_db::{
    period_window, usage_db_path, NewUsageEvent, UsageDb, UsageInsights, UsageInsightsQuery,
    UsagePeriod, UsageKpis, UsageRankItem, UsageRankings, UsageSeriesPoint,
};
pub use usage_pricing::estimate_llm_cost;
pub use orchestration_db::{
    orchestration_db_path, NewOrchestration, NewOrchestrationStep, OrchestrationDb,
    OrchestrationRow, OrchestrationStatus, StepRow, StepStatus,
};
pub use orchestration_spawn::{
    request_orchestration_spawn, set_orchestration_spawner, OrchestrationSpawnRequest,
    OrchestrationSpawner,
};
pub use collab_insights::{
    query_collaboration_insights, CollaborationEdge, CollaborationGraph,
    CollaborationInsights, CollaborationInsightsQuery, CollaborationNode,
    CollaborationOrchestration, CollaborationStep, COLLAB_LIST_LIMIT, COLLAB_OUTPUT_MAX_BYTES,
};
