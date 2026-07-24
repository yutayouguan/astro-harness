//! Agent 运行时与会话存储之间的抽象接口。

use anyhow::Result;
use serde_json::Value;

use crate::{BillingDelta, NewMessage, ScrolledMessage, SearchHit, StoredMessage};

/// Agent 运行时与会话存储之间的抽象接口。
///
/// [`crate::SessionStore`]（SQLite WAL）是默认实现。实现此 trait 可替换为
/// Postgres、远程 API 或纯内存 mock。
///
/// 要求 `Send`（`AgentLoop` 通过 `tokio::spawn` 跨线程移交），不要求 `Sync`。
pub trait ConversationStore: Send {
    // ── 消息 CRUD ──

    /// 追加一条消息到会话。返回新消息的自增 id。
    fn append_message(&self, msg: NewMessage<'_>) -> Result<i64>;

    /// 获取指定会话的全部消息（按时间 + id 升序）。
    fn get_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>>;

    /// 更新 tool 消息的 provider-facing 压缩视图。
    ///
    /// `compressed` 为 `None` 时清除压缩视图（恢复原文）。
    /// 不修改原始 `content` 列和 FTS 索引。
    fn update_message_compressed_content(
        &self,
        message_id: i64,
        compressed: Option<&str>,
    ) -> Result<()>;

    /// 回写本会话最近一条 assistant 的 reasoning_details。
    fn patch_last_assistant_reasoning_details(
        &self,
        session_id: &str,
        details: &Value,
    ) -> Result<()>;

    // ── 会话生命周期 ──

    /// 确保会话存在；不存在时自动创建。
    fn ensure_session(&self, id: &str, source: &str) -> Result<()>;

    // ── 账单 ──

    /// 增量更新会话的 token 用量和费用。
    fn update_session_billing(&self, id: &str, d: BillingDelta) -> Result<()>;

    // ── 上下文召回 ──

    /// 获取最近 `limit` 条消息（按 id 升序返回）。
    fn recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<ScrolledMessage>>;

    /// FTS 召回：返回与 `query` 相关的消息 id（最多 `limit` 个）。
    fn recall_message_ids(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<i64>>;

    /// 获取指定消息周围的上下文窗口（±`window_size` 行）。
    fn scroll_context_window(
        &self,
        session_id: &str,
        around_message_id: i64,
        window_size: i64,
    ) -> Result<Vec<ScrolledMessage>>;

    // ── 跨会话搜索 ──

    /// 跨会话全文搜索。
    fn search_messages(
        &self,
        query: &str,
        source_filter: Option<&str>,
        role_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SearchHit>>;
}

impl<S: ConversationStore + ?Sized> ConversationStore for Box<S> {
    fn append_message(&self, msg: NewMessage<'_>) -> Result<i64> {
        (**self).append_message(msg)
    }
    fn get_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        (**self).get_messages(session_id)
    }
    fn update_message_compressed_content(&self, message_id: i64, compressed: Option<&str>) -> Result<()> {
        (**self).update_message_compressed_content(message_id, compressed)
    }
    fn patch_last_assistant_reasoning_details(&self, session_id: &str, details: &Value) -> Result<()> {
        (**self).patch_last_assistant_reasoning_details(session_id, details)
    }
    fn ensure_session(&self, id: &str, source: &str) -> Result<()> {
        (**self).ensure_session(id, source)
    }
    fn update_session_billing(&self, id: &str, d: BillingDelta) -> Result<()> {
        (**self).update_session_billing(id, d)
    }
    fn recent_messages(&self, session_id: &str, limit: usize) -> Result<Vec<ScrolledMessage>> {
        (**self).recent_messages(session_id, limit)
    }
    fn recall_message_ids(&self, session_id: &str, query: &str, limit: usize) -> Result<Vec<i64>> {
        (**self).recall_message_ids(session_id, query, limit)
    }
    fn scroll_context_window(&self, session_id: &str, around_message_id: i64, window_size: i64) -> Result<Vec<ScrolledMessage>> {
        (**self).scroll_context_window(session_id, around_message_id, window_size)
    }
    fn search_messages(&self, query: &str, source_filter: Option<&str>, role_filter: Option<&str>, limit: i64) -> Result<Vec<SearchHit>> {
        (**self).search_messages(query, source_filter, role_filter, limit)
    }
}
