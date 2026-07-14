//! 轻量路径 / 日志 / 嵌套深度工具库——无 SQLite 依赖。
//!
//! 提取自 `memory` crate 的纯路径与基础设施层，供不需要持久化存储的 crate（如 `mcp`）直接依赖，
//! 避免引入 `rusqlite` 的编译开销。

pub mod infra;
pub mod spawn_depth;
pub mod test_env;
pub mod workspace;

// ─── flat re-exports ─────────────────────────────────────────────────────────

pub use workspace::*;

pub use spawn_depth::{
    can_spawn_nested, current_spawn_depth, effective_max_spawn_depth, scope_spawn_depth,
    scoped_max_spawn_depth, SpawnDepthCtx, DEFAULT_MAX_SPAWN_DEPTH,
};

pub use infra::logging::{init_logging, logs_dir};
pub use infra::log_query::{
    default_agent_log_query, query_agent_logs, AgentLogLine, AgentLogQuery, LogSource,
};
pub use infra::tool_calls::record_tool_call;
