//! Session 生命周期内的共享服务容器（AgentControl、压缩策略等）。

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::Result;
use serde_json::Value;
use session::{
    BillingDelta, ConversationStore, NewResponseItem, ScrolledResponseItem, SearchHit,
    StoredResponseItem,
};

fn patch_item_metadata(
    item: &mut agent_protocol::ResponseItem,
    key: &str,
    value: Option<Value>,
) -> Result<()> {
    let mut encoded = serde_json::to_value(&*item)?;
    let object = encoded
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("response item must serialize as an object"))?;
    let metadata = object
        .entry("internal_chat_message_metadata_passthrough")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let metadata = metadata
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("response item metadata must be an object"))?;
    match value {
        Some(value) => {
            metadata.insert(key.to_string(), value);
        }
        None => {
            metadata.remove(key);
        }
    }
    *item = serde_json::from_value(encoded)?;
    Ok(())
}

#[cfg(test)]
pub(crate) type TurnInputDbWriteHook = Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>;
#[cfg(test)]
pub(crate) type TurnInputMemoryWriteHook = Arc<dyn Fn() + Send + Sync>;
#[cfg(test)]
pub(crate) type ResponseProjectionWriteHook = Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>;

/// Session 级共享依赖：会话存储、压缩策略、记忆、工具注册表等。
pub(crate) struct SessionServices {
    pub(crate) sessions: SharedConversationStore,
    pub(crate) compression_policy: Mutex<Box<dyn crate::compression::CompressionPolicy>>,
    pub(crate) memory: RwLock<memory::MemoryManager>,
    pub(crate) tool_registry: RwLock<tools::ToolRegistry>,
    pub(crate) code_mode: crate::runtime::code_mode::CodeModeService,
    pub(crate) agent_control: Arc<subagents::AgentControl>,
    pub(crate) agent_path: subagents::AgentPath,
    #[allow(dead_code)] // wired in Task 4/5 of inline-managed-network-approval
    pub(crate) network_approval: crate::control::network_approval::NetworkApprovalService,
    #[cfg(test)]
    pub(crate) turn_input_after_db_write: Mutex<Option<TurnInputDbWriteHook>>,
    #[cfg(test)]
    pub(crate) turn_input_after_memory_write: Mutex<Option<TurnInputMemoryWriteHook>>,
    #[cfg(test)]
    pub(crate) response_projection_write: Mutex<Option<ResponseProjectionWriteHook>>,
}

impl SessionServices {
    pub(crate) fn new(
        sessions: Box<dyn ConversationStore>,
        compression_policy: Box<dyn crate::compression::CompressionPolicy>,
        memory: memory::MemoryManager,
        tool_registry: tools::ToolRegistry,
        agent_control: Arc<subagents::AgentControl>,
        agent_path: subagents::AgentPath,
    ) -> Self {
        Self {
            sessions: SharedConversationStore::new(sessions),
            compression_policy: Mutex::new(compression_policy),
            memory: RwLock::new(memory),
            tool_registry: RwLock::new(tool_registry),
            code_mode: crate::runtime::code_mode::CodeModeService::default(),
            agent_control,
            agent_path,
            network_approval: crate::control::network_approval::NetworkApprovalService::new(),
            #[cfg(test)]
            turn_input_after_db_write: Mutex::new(None),
            #[cfg(test)]
            turn_input_after_memory_write: Mutex::new(None),
            #[cfg(test)]
            response_projection_write: Mutex::new(None),
        }
    }
}

pub(crate) struct SharedConversationStore {
    inner: Arc<dyn ConversationStore>,
}

impl SharedConversationStore {
    fn new(store: Box<dyn ConversationStore>) -> Self {
        Self {
            inner: Arc::from(store),
        }
    }
}

#[async_trait::async_trait]
impl ConversationStore for SharedConversationStore {
    async fn invalidate_thread_notes(&self, session_id: &str) -> Result<()> {
        self.inner.invalidate_thread_notes(session_id).await
    }
    async fn thread_context(&self, session_id: &str) -> Result<session::store::ThreadContext> {
        self.inner.thread_context(session_id).await
    }
    async fn write_thread_notes(
        &self,
        session_id: &str,
        notes: &str,
        expected_revision: i64,
    ) -> Result<i64> {
        self.inner
            .write_thread_notes(session_id, notes, expected_revision)
            .await
    }
    async fn request_context_compaction(
        &self,
        session_id: &str,
        turn_id: &str,
        reason: &str,
    ) -> Result<()> {
        self.inner
            .request_context_compaction(session_id, turn_id, reason)
            .await
    }
    async fn take_context_compaction(&self, session_id: &str, turn_id: &str) -> Result<bool> {
        self.inner
            .take_context_compaction(session_id, turn_id)
            .await
    }
    async fn finish_context_compaction(
        &self,
        session_id: &str,
        turn_id: &str,
        status: &str,
    ) -> Result<()> {
        self.inner
            .finish_context_compaction(session_id, turn_id, status)
            .await
    }
    async fn append_response_item(&self, msg: NewResponseItem<'_>) -> Result<i64> {
        self.inner.append_response_item(msg).await
    }

    async fn append_response_items(
        &self,
        session_id: &str,
        items: &[agent_protocol::ResponseItem],
    ) -> Result<Vec<i64>> {
        self.inner.append_response_items(session_id, items).await
    }

    async fn get_response_items(&self, session_id: &str) -> Result<Vec<StoredResponseItem>> {
        self.inner.get_response_items(session_id).await
    }

    async fn replace_response_items(
        &self,
        session_id: &str,
        items: &[agent_protocol::ResponseItem],
    ) -> Result<()> {
        self.inner.replace_response_items(session_id, items).await
    }

    async fn update_response_item_compressed_content(
        &self,
        message_id: i64,
        compressed: Option<&str>,
    ) -> Result<()> {
        self.inner
            .update_response_item_compressed_content(message_id, compressed)
            .await
    }

    async fn ensure_session(&self, id: &str, source: &str) -> Result<()> {
        self.inner.ensure_session(id, source).await
    }

    async fn update_session_billing(&self, id: &str, delta: BillingDelta) -> Result<()> {
        self.inner.update_session_billing(id, delta).await
    }

    async fn recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<ScrolledResponseItem>> {
        self.inner.recent_messages(session_id, limit).await
    }

    async fn recall_message_ids(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<i64>> {
        self.inner
            .recall_message_ids(session_id, query, limit)
            .await
    }

    async fn scroll_context_window(
        &self,
        session_id: &str,
        around_message_id: i64,
        window_size: i64,
    ) -> Result<Vec<ScrolledResponseItem>> {
        self.inner
            .scroll_context_window(session_id, around_message_id, window_size)
            .await
    }

    async fn search_messages(
        &self,
        query: &str,
        source_filter: Option<&str>,
        role_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SearchHit>> {
        self.inner
            .search_messages(query, source_filter, role_filter, limit)
            .await
    }
}

/// Process-local conversation storage for one-shot child runs such as review.
///
/// It intentionally implements only local message semantics: billing and search
/// are not durable, while recent/context reads still observe the child's own
/// messages so the normal model loop can run unchanged.
#[derive(Default)]
pub(crate) struct EphemeralConversationStore {
    next_id: AtomicI64,
    messages: Mutex<Vec<StoredResponseItem>>,
}

#[async_trait::async_trait]
impl ConversationStore for EphemeralConversationStore {
    async fn append_response_item(&self, msg: NewResponseItem<'_>) -> Result<i64> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        self.messages
            .lock()
            .expect("ephemeral conversation mutex poisoned")
            .push(StoredResponseItem {
                id,
                session_id: msg.session_id.to_string(),
                item: msg.item.clone(),
                timestamp: chrono::Utc::now().timestamp_millis() as f64 / 1_000.0,
                token_count: msg.token_count,
                finish_reason: msg.finish_reason.map(str::to_string),
            });
        Ok(id)
    }

    async fn get_response_items(&self, session_id: &str) -> Result<Vec<StoredResponseItem>> {
        Ok(self
            .messages
            .lock()
            .expect("ephemeral conversation mutex poisoned")
            .iter()
            .filter(|message| message.session_id == session_id)
            .cloned()
            .collect())
    }

    async fn replace_response_items(
        &self,
        session_id: &str,
        items: &[agent_protocol::ResponseItem],
    ) -> Result<()> {
        let mut messages = self
            .messages
            .lock()
            .expect("ephemeral conversation mutex poisoned");
        messages.retain(|message| message.session_id != session_id);
        let now = chrono::Utc::now().timestamp_millis() as f64 / 1_000.0;
        for (index, item) in items.iter().enumerate() {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
            messages.push(StoredResponseItem {
                id,
                session_id: session_id.to_string(),
                item: item.clone(),
                timestamp: now + index as f64 * 0.000_001,
                token_count: None,
                finish_reason: None,
            });
        }
        Ok(())
    }

    async fn update_response_item_compressed_content(
        &self,
        message_id: i64,
        compressed: Option<&str>,
    ) -> Result<()> {
        if let Some(message) = self
            .messages
            .lock()
            .expect("ephemeral conversation mutex poisoned")
            .iter_mut()
            .find(|message| message.id == message_id)
        {
            patch_item_metadata(
                &mut message.item,
                "astro_compressed_output",
                compressed.map(Value::from),
            )?;
        }
        Ok(())
    }

    async fn ensure_session(&self, _id: &str, _source: &str) -> Result<()> {
        Ok(())
    }

    async fn update_session_billing(&self, _id: &str, _delta: BillingDelta) -> Result<()> {
        Ok(())
    }

    async fn recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<ScrolledResponseItem>> {
        let messages = self
            .messages
            .lock()
            .expect("ephemeral conversation mutex poisoned");
        let matching = messages
            .iter()
            .filter(|message| message.session_id == session_id)
            .collect::<Vec<_>>();
        Ok(matching
            .into_iter()
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|message| ScrolledResponseItem {
                id: message.id,
                item: message.item.clone(),
                is_anchor: false,
            })
            .collect())
    }

    async fn recall_message_ids(
        &self,
        _session_id: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<i64>> {
        Ok(Vec::new())
    }

    async fn scroll_context_window(
        &self,
        session_id: &str,
        around_message_id: i64,
        window_size: i64,
    ) -> Result<Vec<ScrolledResponseItem>> {
        Ok(self
            .messages
            .lock()
            .expect("ephemeral conversation mutex poisoned")
            .iter()
            .filter(|message| {
                message.session_id == session_id
                    && (message.id - around_message_id).abs() <= window_size
            })
            .map(|message| ScrolledResponseItem {
                id: message.id,
                item: message.item.clone(),
                is_anchor: message.id == around_message_id,
            })
            .collect())
    }

    async fn search_messages(
        &self,
        _query: &str,
        _source_filter: Option<&str>,
        _role_filter: Option<&str>,
        _limit: i64,
    ) -> Result<Vec<SearchHit>> {
        Ok(Vec::new())
    }
}
