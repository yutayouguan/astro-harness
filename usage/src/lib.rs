//! 用量事件库、按 Agent 统计与费用估算。
//!
//! - [`db`]：`~/.astro/usage.db` 事件写入与洞察聚合
//! - [`stats`]：`usage-stats.json` 工具集/技能计数
//! - [`pricing`]：路由感知费用估算
//! - [`trace_insights`]：按 session 聚合调用链（可读 `session` 库）

pub mod db;
pub mod eval_export;
pub mod pricing;
pub mod stats;
pub mod trace_insights;

pub use db::{
    period_window, usage_db_path, NewUsageEvent, UsageDb, UsageInsights, UsageInsightsQuery,
    UsagePeriod, UsageKpis, UsageRankItem, UsageRankings, UsageSeriesPoint, USAGE_SCHEMA_VERSION,
};
pub use eval_export::{
    export_session_eval_jsonl, export_session_eval_jsonl_with_db, write_eval_record_jsonl,
    EvalEvent, EvalMessagePreview, EvalSessionRecord,
};
pub use pricing::{
    estimate_usage_cost, resolve_billing_route, BillingRoute, CostResult, CostStatus, UsageTokens,
};
pub use stats::{
    get_usage_summary, load_usage_stats, record_tool_call, save_usage_stats, AgentUsageStats,
    AgentUsageSummary,
};
pub use trace_insights::{
    query_trace_insights, TraceEvent, TraceInsights, TraceInsightsQuery, TraceKpis, TraceSummary,
    TRACE_EVENTS_LIMIT, TRACE_LIST_LIMIT,
};
