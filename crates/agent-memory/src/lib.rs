//! Astro 记忆真域：精炼记忆、审批队列、回顾、入梦与 `MemoryManager`。
//!
//! 路径 / 定时 / 会话库 / 用量等已独立为 `home`、`cron`、`session`、`usage` 等 crate。
//! 完整工作区引导仍由 [`ensure_workspace`] 编排（`home` 脚手架 + 会话库 + 技能播种）。

pub mod agent;
pub mod config;
pub mod decision_log;
pub mod dreaming;
pub mod pending;
pub mod permission_audit;
pub mod protocol;
pub mod review;
pub mod session;

pub use agent::store::{parse_memory_entries, MemoryStore, MemoryWriteResult};
pub use agent::workspace;
pub use config::{
    add_command_to_allowlist, amend_network_domain, load_approvals_config, load_auxiliary_config,
    load_compression_config, load_evolution_config, load_learning_config, load_memory_config,
    load_permission_settings, remove_command_from_allowlist, reset_all_auxiliary_routes,
    reset_all_evolution_routes, reset_compression_config, resolve_auxiliary, set_approval_mode,
    set_auto_refresh_on_update, set_auxiliary_route, set_background_review_enabled,
    set_compression_config, set_evolution_auto, set_evolution_curator, set_evolution_dspy,
    set_evolution_enabled, set_evolution_gates, set_evolution_route, set_evolution_search,
    set_permission_preset, set_write_approval, ApprovalsConfig, AuxiliaryConfig, AuxiliaryKind,
    AuxiliaryRoute, CompressionConfig, EvolutionAuto, EvolutionConfig, EvolutionCurator,
    EvolutionDspy, EvolutionGates, EvolutionRouteKind, EvolutionSearch, LearningConfig,
    LoadedPermissionSettings, MemoryConfig, PermissionConfigDiagnostic, PermissionConfigSource,
    PermissionDiagnosticCode,
};
pub use decision_log::{
    append_decision, decisions_path, list_recent as list_recent_decisions, try_append_decision,
    DecisionEntry, DecisionKind,
};
pub use dreaming::{
    load_dreaming_state, prepare_all_dream_jobs, save_dreaming_state, set_dreaming_enabled,
    DreamMemoryUpdate, DreamRunReport, DreamingState,
};
pub use pending::{
    approve as approve_pending_memory, enqueue as enqueue_pending_memory, list_pending,
    pending_dir, reject as reject_pending_memory, PendingMemoryWrite,
};
pub use permission_audit::{
    append_permission_audit, clear_permission_audits, list_permission_audits_before,
    list_recent_permission_audits, permission_audit_archive_path, permission_audit_path,
    permission_snapshot_hash, try_append_permission_audit, PermissionAuditCapability,
    PermissionAuditEvent, PermissionAuditKind, MAX_PERMISSION_AUDIT_FILE_BYTES,
    PERMISSION_AUDIT_ARCHIVE_COUNT,
};
pub use review::{
    apply_review_suggestions, build_review_digest, parse_review_llm_output, ReviewOutput,
    ReviewSuggestion, REVIEW_SYSTEM_PROMPT,
};
pub use session::manager::{dispatch_memory_tool, MemoryManager, MemoryTarget};
pub use workspace::{ensure_default_workspace, ensure_workspace};
