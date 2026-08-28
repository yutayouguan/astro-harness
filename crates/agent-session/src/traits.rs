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
#[async_trait::async_trait]
pub trait ConversationStore: Send + Sync {
    // ── 消息 CRUD ──

    /// 追加一条消息到会话。返回新消息的自增 id。
    async fn append_message(&self, msg: NewMessage<'_>) -> Result<i64>;

    /// 获取指定会话的全部消息（按时间 + id 升序）。
    async fn get_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>>;

    /// 更新消息的 provider-facing 压缩视图或内部交付标记。
    ///
    /// `compressed` 为 `None` 时清除压缩视图（恢复原文）。
    /// 不修改原始 `content` 列和 FTS 索引。
    async fn update_message_compressed_content(
        &self,
        message_id: i64,
        compressed: Option<&str>,
    ) -> Result<()>;

    /// 回写本会话最近一条 assistant 的 reasoning_details。
    async fn patch_last_assistant_reasoning_details(
        &self,
        session_id: &str,
        details: &Value,
    ) -> Result<()>;

    // ── 会话生命周期 ──

    /// 确保会话存在；不存在时自动创建。
    async fn ensure_session(&self, id: &str, source: &str) -> Result<()>;

    // ── 账单 ──

    /// 增量更新会话的 token 用量和费用。
    async fn update_session_billing(&self, id: &str, d: BillingDelta) -> Result<()>;

    // ── 上下文召回 ──

    /// 获取最近 `limit` 条消息（按 id 升序返回）。
    async fn recent_messages(&self, session_id: &str, limit: usize) -> Result<Vec<ScrolledMessage>>;

    /// FTS 召回：返回与 `query` 相关的消息 id（最多 `limit` 个）。
    async fn recall_message_ids(&self, session_id: &str, query: &str, limit: usize) -> Result<Vec<i64>>;

    /// 获取指定消息周围的上下文窗口（±`window_size` 行）。
    async fn scroll_context_window(
        &self,
        session_id: &str,
        around_message_id: i64,
        window_size: i64,
    ) -> Result<Vec<ScrolledMessage>>;

    // ── 跨会话搜索 ──

    /// 跨会话全文搜索。
    async fn search_messages(
        &self,
        query: &str,
        source_filter: Option<&str>,
        role_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SearchHit>>;
}
