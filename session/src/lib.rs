//! 会话持久化：单库 `SessionStore`（sessions / messages / FTS5）与上下文召回。
//!
//! 与精炼记忆（`MEMORY.md`）无关；数据路径由调用方传入（通常
//! `{ASTRO_MEMORY_DIR|~/.astro}/sessions/state.db`）。

pub mod message_db;
pub mod store;
pub mod tools;

pub use message_db::{build_conversation_context, ScrolledMessage};
pub use store::{
    BillingDelta, ChatActivityStored, ChatHistoryMessage, NewMessage, RecentSession, SearchHit,
    SessionBillingRow, SessionStore, StoredMessage, StoredSession, SCHEMA_VERSION,
};
pub use tools::dispatch_session_tool;
