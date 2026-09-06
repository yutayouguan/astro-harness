//! 会话持久化：单库 `SessionStore`（sessions / response_items / FTS5）与上下文召回。
//!
//! 与精炼记忆（`MEMORY.md`）无关；数据路径由调用方传入（通常
//! `{ASTRO_MEMORY_DIR|~/.astro}/data/state.db`）。

pub mod context_recall;
pub mod format;
pub mod store;
pub mod tools;
pub mod traits;

pub use agent_protocol::FunctionCallOutputPayload;
pub use context_recall::{build_conversation_context, ScrolledResponseItem};
pub use format::format_recalled_context;
pub use store::{
    projects::{Project, DEFAULT_PROJECT_ICON, DEFAULT_PROJECT_ID, DEFAULT_PROJECT_NAME},
    BillingDelta, NewResponseItem, RecentSession, ResponseItem, SearchHit, SessionBillingRow,
    SessionListFilter, SessionPlacementFilter, SessionStore, StoredResponseItem, StoredSession,
    SCHEMA_VERSION,
};
pub use tools::{dispatch_session_tool, record_message};
pub use traits::ConversationStore;
