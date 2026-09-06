//! 会话召回类型与上下文构建。

use crate::traits::ConversationStore;

/// 对话上下文中的原生 item，可标记为 FTS 召回的锚点。
#[derive(Debug, Clone, PartialEq)]
pub struct ScrolledResponseItem {
    /// item 主键，与 `response_items.id` 一致。
    pub id: i64,
    pub item: agent_protocol::ResponseItem,
    /// 是否为 FTS 召回窗口的中心锚点 item。
    pub is_anchor: bool,
}

/// 构建会话上下文：先取最近 `recent_turns` 条，再按 FTS 关键词各召回最多 3 个锚点及其窗口。
///
/// 合并后按 `id` 排序并去重。`fts_keywords` 为 `None` 时仅返回最近 item。
pub async fn build_conversation_context(
    store: &impl ConversationStore,
    session_id: &str,
    recent_turns: usize,
    fts_keywords: Option<&str>,
) -> anyhow::Result<Vec<ScrolledResponseItem>> {
    let mut context = store.recent_messages(session_id, recent_turns).await?;

    if let Some(keywords) = fts_keywords {
        for anchor_id in store.recall_message_ids(session_id, keywords, 3).await? {
            let window = store
                .scroll_context_window(session_id, anchor_id, 5)
                .await?;
            context.extend(window);
        }
        context.sort_by_key(|item| item.id);
        context.dedup_by_key(|item| item.id);
    }

    Ok(context)
}
