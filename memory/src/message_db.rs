//! 会话消息读写的兼容层：委托 [`SessionStore`]，保留 [`ScrolledMessage`] /
//! [`build_conversation_context`] 供 Prompt 召回使用。
//!
//! 新代码请直接使用 [`crate::session_store::SessionStore`]。

use std::path::PathBuf;

use crate::session_store::SessionStore;

/// 对话上下文中的单条消息，可标记为 FTS 召回的锚点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrolledMessage {
    /// 消息主键，与 `messages.id` 一致。
    pub id: i64,
    /// 角色标识（如 `user`、`assistant`）。
    pub role: String,
    /// 消息正文。
    pub content: String,
    /// 是否为 FTS 召回窗口的中心锚点消息。
    pub is_anchor: bool,
}

/// 基于 [`SessionStore`] 的兼容门面（打开同一 `state.db`）。
///
/// 新调用方应优先使用 [`SessionStore`]；本类型仅为旧 API 过渡保留。
pub struct MessageDb {
    store: SessionStore,
}

impl MessageDb {
    /// 打开或创建消息数据库（schema v11），自动建表并启用 WAL。
    pub fn new(path: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            store: SessionStore::open(&path)?,
        })
    }

    /// 底层 [`SessionStore`] 引用。
    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    /// 插入一条消息并返回自增 `id`（自动 ensure session，source=`tauri`）。
    pub fn insert_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
    ) -> anyhow::Result<i64> {
        self.store.ensure_session(session_id, "tauri")?;
        self.store.append_message(crate::session_store::NewMessage {
            content: Some(content),
            ..crate::session_store::NewMessage::empty(session_id, role)
        })
    }

    /// 在指定会话内按 FTS 查询召回消息 id 列表。
    pub fn recall_message_ids(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<i64>> {
        self.store.recall_message_ids(session_id, query, limit)
    }

    /// 以 `around_message_id` 为中心取上下文窗口。
    pub fn scroll_context_window(
        &self,
        session_id: &str,
        around_message_id: i64,
        window_size: i64,
    ) -> anyhow::Result<Vec<ScrolledMessage>> {
        self.store
            .scroll_context_window(session_id, around_message_id, window_size)
    }

    /// 取指定会话最近 `limit` 条消息，按时间正序返回。
    pub fn recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<ScrolledMessage>> {
        self.store.recent_messages(session_id, limit)
    }

    /// 返回全局最近一条消息所属的 `session_id`。
    pub fn latest_session_id(&self) -> anyhow::Result<Option<String>> {
        self.store.latest_session_id()
    }
}

/// 构建会话上下文：先取最近 `recent_turns` 条，再按 FTS 关键词各召回最多 3 个锚点及其窗口。
///
/// 合并后按 `id` 排序并去重。`fts_keywords` 为 `None` 时仅返回最近消息。
pub fn build_conversation_context(
    store: &SessionStore,
    session_id: &str,
    recent_turns: usize,
    fts_keywords: Option<&str>,
) -> anyhow::Result<Vec<ScrolledMessage>> {
    let mut context = store.recent_messages(session_id, recent_turns)?;

    if let Some(keywords) = fts_keywords {
        for anchor_id in store.recall_message_ids(session_id, keywords, 3)? {
            let window = store.scroll_context_window(session_id, anchor_id, 5)?;
            context.extend(window);
        }
        context.sort_by_key(|m| m.id);
        context.dedup_by_key(|m| m.id);
    }

    Ok(context)
}
