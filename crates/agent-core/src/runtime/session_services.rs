//! Session 生命周期内的共享服务容器（AgentControl、压缩策略等）。

use std::sync::{Arc, Mutex, RwLock};

use anyhow::Result;
use serde_json::Value;
use session::{
    BillingDelta, ConversationStore, NewMessage, ScrolledMessage, SearchHit, StoredMessage,
};

#[cfg(test)]
pub(crate) type TurnInputDbWriteHook = Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>;
#[cfg(test)]
pub(crate) type TurnInputMemoryWriteHook = Arc<dyn Fn() + Send + Sync>;

/// Session 级共享依赖：会话存储、压缩策略、记忆、工具注册表等。
pub(crate) struct SessionServices {
    pub(crate) sessions: SharedConversationStore,
    pub(crate) compression_policy: Mutex<Box<dyn crate::compression::CompressionPolicy>>,
    pub(crate) memory: RwLock<memory::MemoryManager>,
    pub(crate) tool_registry: RwLock<tools::ToolRegistry>,
    pub(crate) agent_control: Arc<subagents::AgentControl>,
    pub(crate) agent_path: subagents::AgentPath,
    #[allow(dead_code)] // wired in Task 4/5 of inline-managed-network-approval
    pub(crate) network_approval: crate::control::network_approval::NetworkApprovalService,
    #[cfg(test)]
    pub(crate) turn_input_after_db_write: Mutex<Option<TurnInputDbWriteHook>>,
    #[cfg(test)]
    pub(crate) turn_input_after_memory_write: Mutex<Option<TurnInputMemoryWriteHook>>,
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
            agent_control,
            agent_path,
            network_approval: crate::control::network_approval::NetworkApprovalService::new(),
            #[cfg(test)]
            turn_input_after_db_write: Mutex::new(None),
            #[cfg(test)]
            turn_input_after_memory_write: Mutex::new(None),
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
    async fn append_message(&self, msg: NewMessage<'_>) -> Result<i64> {
        self.inner.append_message(msg).await
    }

    async fn get_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        self.inner.get_messages(session_id).await
    }

    async fn update_message_compressed_content(
        &self,
        message_id: i64,
        compressed: Option<&str>,
    ) -> Result<()> {
        self.inner
            .update_message_compressed_content(message_id, compressed)
            .await
    }

    async fn patch_last_assistant_reasoning_details(
        &self,
        session_id: &str,
        details: &Value,
    ) -> Result<()> {
        self.inner
            .patch_last_assistant_reasoning_details(session_id, details)
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
    ) -> Result<Vec<ScrolledMessage>> {
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
    ) -> Result<Vec<ScrolledMessage>> {
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
