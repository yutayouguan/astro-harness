//! Astro 记忆子系统：精炼记忆真域 + 跨 crate facade。
//!
//! **真域：** `MemoryStore` / pending / review / dreaming / config / `MemoryManager`，
//! 以及 `ensure_workspace`（会话库 + 技能播种编排）。
//!
//! 路径、日志、定时、会话库、用量等已独立；本包 `pub use` 仅为兼容既有 `memory::*`。

pub mod agent;
pub mod config;
pub mod dreaming;
pub mod pending;
pub mod review;
pub mod session;

#[cfg(test)]
pub(crate) mod test_env;

// ── 真域 ──────────────────────────────────────────────────────
pub use agent::store::{parse_memory_entries, MemoryStore, MemoryWriteResult};
pub use agent::workspace;
pub use config::{
    load_auxiliary_config, load_memory_config, resolve_auxiliary, set_background_review_enabled,
    set_auto_refresh_on_update, set_write_approval, AuxiliaryConfig, AuxiliaryKind, AuxiliaryRoute,
    MemoryConfig,
};
pub use pending::{
    approve as approve_pending_memory, enqueue as enqueue_pending_memory, list_pending,
    pending_dir, reject as reject_pending_memory, PendingMemoryWrite,
};
pub use review::{
    apply_review_suggestions, build_review_digest, parse_review_llm_output, ReviewOutput,
    ReviewSuggestion, REVIEW_SYSTEM_PROMPT,
};
pub use dreaming::{
    load_dreaming_state, prepare_all_dream_jobs, save_dreaming_state, set_dreaming_enabled,
    DreamMemoryUpdate, DreamRunReport, DreamingState,
};
pub use session::manager;
pub use session::message_db;
pub use session::store as session_store;

// ── facade：已拆出域 ───────────────────────────────────────────
pub use ::cron::run_db as cron_run_db;
pub use ::usage::db as usage_db;
pub use ::usage::pricing as usage_pricing;
pub use ::usage::stats as usage_stats;
pub use ::usage::trace_insights;
pub use ::orchestration::collab_insights;
pub use ::orchestration::db as orchestration_db;
pub use ::orchestration::spawn as orchestration_spawn;
pub use ::artifacts::db as artifact_db;
pub use ::delegate::async_reg as delegate_async;
pub use ::delegate::git_worktree;
pub use ::delegate::spawn as delegate_spawn;

// ── 根级符号（兼容 `memory::Foo`）─────────────────────────────
pub use artifact_db::{
    artifacts_db_path, category_from_name, is_junk_artifact_name, open_default, ArtifactDb,
    ArtifactRow, ArtifactSource, ReconcileReport,
};
pub use manager::{
    dispatch_memory_tool, format_recalled_context, MemoryManager, MemoryTarget,
};
pub use message_db::{build_conversation_context, ScrolledMessage};
pub use session_store::{
    BillingDelta, ChatActivityStored, ChatHistoryMessage, NewMessage, RecentSession, SearchHit,
    SessionBillingRow, SessionStore, StoredMessage, StoredSession,
};
pub use workspace::{
    active_agent_id, agent_config_dir, agent_id_from_workspace_dir_name, agent_workspace_dir,
    create_agent, create_agent_with_profile, daily_memory_path, default_agent_workspace_dir,
    default_memory_dir, ensure_agent_space, ensure_daily_memory, ensure_default_workspace,
    ensure_workspace, generated_dir, list_agents, list_daily_memory_dates, normalize_agent_id,
    seed_create_agent_skill, set_active_agent, today_date_string, write_agent_config, AgentInfo,
    AgentProfile, AgentRuntimeConfig, EnsureWorkspaceReport, GeneratedKind, GENERATED_SUBDIRS,
    DEFAULT_AGENT_ID,
};

pub use home::config::agent_icons;
pub use home::config::auto_icon;
pub use home::config::tools_enabled;

pub use home::{
    apply_pending_agent_icons, clear_pending_agent_icon, resolve_icon_field, set_pending_agent_icon,
    update_agent_icons, write_agent_icon, AgentIconKind,
    apply_auto_lucide_icon, lucide_svg_bytes, suggest_lucide_icon_id, AutoLucideIcon,
    AUTO_LUCIDE_ICONS, scan_memory_content,
    is_tool_call_allowed, is_toolset_enabled, load_tools_enabled, load_tools_enabled_for_agent,
    save_tools_enabled, save_tools_enabled_for_agent, sync_tools_enabled_defaults,
    sync_tools_enabled_defaults_for_agent, tool_name_to_toolset, tools_enabled_path,
    KNOWN_TOOLSET_IDS,
    init_logging, logs_dir,
    default_agent_log_query, query_agent_logs, AgentLogLine, AgentLogQuery, LogSource,
    record_tool_call,
    can_spawn_nested, current_spawn_depth, effective_max_spawn_depth, scoped_max_spawn_depth,
    scope_spawn_depth, SpawnDepthCtx, DEFAULT_MAX_SPAWN_DEPTH,
};

pub use ::cron::{
    cron_dir, cron_extract_preamble, dispatch_cron_tool, normalize_cron_extract, tick_default,
    CronJob, CronJobExtract, CronStore, NewCronJob,
};
pub use cron_run_db::{
    cron_db_path, CronRunDb, CronRunFilters, CronRunRow, NewCronRun,
};
pub use usage_stats::{
    get_usage_summary, load_usage_stats, record_tool_call as record_usage_tool_call,
    save_usage_stats, AgentUsageStats, AgentUsageSummary,
};
pub use usage_db::{
    period_window, usage_db_path, NewUsageEvent, UsageDb, UsageInsights, UsageInsightsQuery,
    UsagePeriod, UsageKpis, UsageRankItem, UsageRankings, UsageSeriesPoint, USAGE_SCHEMA_VERSION,
};
pub use usage_pricing::{
    estimate_usage_cost, resolve_billing_route, BillingRoute, CostResult, CostStatus, UsageTokens,
};
pub use orchestration_db::{
    orchestration_db_path, NewOrchestration, NewOrchestrationStep, OrchestrationDb,
    OrchestrationRow, OrchestrationStatus, StepRow, StepStatus,
};
pub use orchestration_spawn::{OrchestrationSpawnRequest, OrchestrationSpawner};
pub use delegate_spawn::{DelegateRole, DelegateRunRequest, DelegateRunner, DelegateTaskSpec};
pub use git_worktree::{
    create_task_worktree, find_git_root, resolve_project_root, WorktreeHandle,
};
pub use delegate_async::{
    async_delegate_cancel, async_delegate_collect, async_delegate_status,
    start_delegate_async, resume_incomplete_async_delegates, AsyncDelegateRecord,
    AsyncDelegateRegistry, AsyncDelegateStatus, DelegateAsyncSpawner,
};
pub use collab_insights::{
    query_collaboration_insights, CollaborationEdge, CollaborationGraph,
    CollaborationInsights, CollaborationInsightsQuery, CollaborationNode,
    CollaborationOrchestration, CollaborationStep, COLLAB_LIST_LIMIT, COLLAB_OUTPUT_MAX_BYTES,
};
pub use trace_insights::{
    query_trace_insights, TraceEvent, TraceInsights, TraceInsightsQuery, TraceKpis, TraceSummary,
    TRACE_EVENTS_LIMIT, TRACE_LIST_LIMIT,
};
