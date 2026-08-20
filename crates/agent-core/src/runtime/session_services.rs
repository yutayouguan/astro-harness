//! Session-scoped shared services.
//!
//! The container follows Codex's `SessionServices` ownership boundary. SQLite
//! remains synchronous, so its adapter holds a standard mutex for exactly one
//! [`ConversationStore`] call and never across an async suspension.

use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use serde_json::Value;
use session::{
    BillingDelta, ConversationStore, NewMessage, ScrolledMessage, SearchHit, StoredMessage,
};

#[cfg(test)]
pub(crate) type TurnInputDbWriteHook = Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>;
#[cfg(test)]
pub(crate) type TurnInputMemoryWriteHook = Arc<dyn Fn() + Send + Sync>;

/// Shared dependencies that remain stable for the lifetime of a session.
pub(crate) struct SessionServices {
    pub(crate) sessions: SharedConversationStore,
    pub(crate) compression_policy: Mutex<Box<dyn crate::compression::CompressionPolicy>>,
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
        agent_control: Arc<subagents::AgentControl>,
        agent_path: subagents::AgentPath,
    ) -> Self {
        Self {
            sessions: SharedConversationStore::new(sessions),
            compression_policy: Mutex::new(compression_policy),
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

/// Makes a `Send` conversation store safely shareable by a Codex-style session.
pub(crate) struct SharedConversationStore {
    inner: Mutex<Box<dyn ConversationStore>>,
}

impl SharedConversationStore {
    fn new(store: Box<dyn ConversationStore>) -> Self {
        Self {
            inner: Mutex::new(store),
        }
    }

    fn with_store<T>(
        &self,
        operation: impl FnOnce(&dyn ConversationStore) -> Result<T>,
    ) -> Result<T> {
        let store = self
            .inner
            .lock()
            .map_err(|_| anyhow!("conversation store mutex poisoned"))?;
        operation(store.as_ref())
    }
}

impl ConversationStore for SharedConversationStore {
    fn append_message(&self, msg: NewMessage<'_>) -> Result<i64> {
        self.with_store(|store| store.append_message(msg))
    }

    fn get_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        self.with_store(|store| store.get_messages(session_id))
    }

    fn update_message_compressed_content(
        &self,
        message_id: i64,
        compressed: Option<&str>,
    ) -> Result<()> {
        self.with_store(|store| store.update_message_compressed_content(message_id, compressed))
    }

    fn patch_last_assistant_reasoning_details(
        &self,
        session_id: &str,
        details: &Value,
    ) -> Result<()> {
        self.with_store(|store| store.patch_last_assistant_reasoning_details(session_id, details))
    }

    fn ensure_session(&self, id: &str, source: &str) -> Result<()> {
        self.with_store(|store| store.ensure_session(id, source))
    }

    fn update_session_billing(&self, id: &str, delta: BillingDelta) -> Result<()> {
        self.with_store(|store| store.update_session_billing(id, delta))
    }

    fn recent_messages(&self, session_id: &str, limit: usize) -> Result<Vec<ScrolledMessage>> {
        self.with_store(|store| store.recent_messages(session_id, limit))
    }

    fn recall_message_ids(&self, session_id: &str, query: &str, limit: usize) -> Result<Vec<i64>> {
        self.with_store(|store| store.recall_message_ids(session_id, query, limit))
    }

    fn scroll_context_window(
        &self,
        session_id: &str,
        around_message_id: i64,
        window_size: i64,
    ) -> Result<Vec<ScrolledMessage>> {
        self.with_store(|store| {
            store.scroll_context_window(session_id, around_message_id, window_size)
        })
    }

    fn search_messages(
        &self,
        query: &str,
        source_filter: Option<&str>,
        role_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SearchHit>> {
        self.with_store(|store| store.search_messages(query, source_filter, role_filter, limit))
    }
}
