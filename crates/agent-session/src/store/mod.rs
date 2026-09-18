//! 单库会话存储：sessions、原生 Response items 与 FTS5。

mod branches;
pub mod projects;
mod response_items;
mod rollout_projection;
mod schema;
mod search;
mod sessions;
mod thread_attachments;
mod thread_context;
pub use thread_attachments::{
    MAX_THREAD_ATTACHMENTS, MAX_THREAD_ATTACHMENT_IDENTITY_KEY_BYTES,
    MAX_THREAD_ATTACHMENT_PAGE_SIZE, MAX_THREAD_ATTACHMENT_PAYLOAD_BYTES,
    MAX_THREAD_ATTACHMENT_TYPE_BYTES,
};
pub use thread_context::{ThreadContext, MAX_THREAD_NOTES_CHARS};

use agent_db::sqlx::{self, Row};
use agent_db::{AstroDb, DbSpec, SqlitePool};
pub use agent_protocol::ResponseItem;
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DB_SPEC: DbSpec = DbSpec::new("session", "state.db");

pub use rollout_projection::rebuild_response_items_from_rollout;
pub use schema::SCHEMA_VERSION;

/// 单次 LLM 调用的账单增量（累加到 sessions 行）。
#[derive(Debug, Clone, Default)]
pub struct BillingDelta {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub estimated_cost_usd: f64,
    pub api_call_count: i64,
    pub billing_provider: Option<String>,
    pub billing_base_url: Option<String>,
    pub billing_mode: Option<String>,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
    pub pricing_version: Option<String>,
    pub model: Option<String>,
}

/// 从 sessions 读出的账单列快照。
#[derive(Debug, Clone)]
pub struct SessionBillingRow {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub api_call_count: i64,
    pub estimated_cost_usd: f64,
    pub actual_cost_usd: Option<f64>,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
    pub pricing_version: Option<String>,
    pub billing_provider: Option<String>,
    pub billing_base_url: Option<String>,
    pub billing_mode: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewResponseItem<'a> {
    pub session_id: &'a str,
    pub item: &'a ResponseItem,
    pub token_count: Option<i64>,
    pub finish_reason: Option<&'a str>,
}

impl<'a> NewResponseItem<'a> {
    pub fn new(session_id: &'a str, item: &'a ResponseItem) -> Self {
        Self {
            session_id,
            item,
            token_count: None,
            finish_reason: None,
        }
    }
}

/// 从库中读出的原生 Responses item。
#[derive(Debug, Clone)]
pub struct StoredResponseItem {
    pub id: i64,
    pub session_id: String,
    pub item: ResponseItem,
    pub timestamp: f64,
    pub token_count: Option<i64>,
    pub finish_reason: Option<String>,
}

impl StoredResponseItem {
    pub fn role(&self) -> Option<&str> {
        self.item.role().or_else(|| {
            if self.item.is_tool_output() {
                Some("tool")
            } else if matches!(
                self.item,
                ResponseItem::FunctionCall { .. }
                    | ResponseItem::CustomToolCall { .. }
                    | ResponseItem::ToolSearchCall { .. }
                    | ResponseItem::Reasoning { .. }
                    | ResponseItem::LocalShellCall { .. }
                    | ResponseItem::WebSearchCall { .. }
                    | ResponseItem::ImageGenerationCall { .. }
                    | ResponseItem::AgentMessage { .. }
            ) {
                Some("assistant")
            } else {
                None
            }
        })
    }

    pub fn text(&self) -> String {
        self.item.text()
    }

    pub fn tool_name(&self) -> Option<&str> {
        self.item.tool_name()
    }

    pub fn qualified_tool_name(&self) -> Option<String> {
        self.item.qualified_tool_name()
    }

    pub fn call_id(&self) -> Option<&str> {
        self.item.call_id()
    }

    pub fn compressed_text(&self) -> Option<&str> {
        self.item.compressed_text()
    }

    pub fn is_tool_output(&self) -> bool {
        self.item.is_tool_output()
    }
}

/// 从库中读出的会话元数据行。
#[derive(Debug, Clone)]
pub struct StoredSession {
    pub id: String,
    pub source: String,
    pub title: Option<String>,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub end_reason: Option<String>,
    pub model: Option<String>,
    pub parent_session_id: Option<String>,
    pub message_count: i64,
    pub tool_call_count: i64,
    pub archived_at: Option<f64>,
    pub pinned_at: Option<f64>,
    /// 分支类型：`"branch"`、`"side"`、`"agent"`，根会话为 `None`。
    pub branch_kind: Option<String>,
    /// 此分支在父会话中分叉的消息 id。
    pub branch_parent_message_id: Option<i64>,
    pub branch_parent_turn_index: Option<i64>,
    pub branch_inherited_turn_count: Option<i64>,
    pub branch_created_at: Option<f64>,
}

/// FTS 搜索命中。
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub snippet: String,
    /// 邻接上下文（同会话前后消息摘要）；无则空串。
    pub context: String,
    pub tool_name: Option<String>,
}

/// 侧栏「近期会话」列表项：`title` 优先，否则用首条 user `content` 截断作 preview。
#[derive(Debug, Clone)]
pub struct RecentSession {
    pub id: String,
    pub source: String,
    pub project_id: Option<String>,
    pub title: Option<String>,
    pub started_at: f64,
    /// 首条 user 消息正文截断；无则 `None`。
    pub preview: Option<String>,
    pub ended_at: Option<f64>,
    pub end_reason: Option<String>,
    pub archived_at: Option<f64>,
    pub pinned_at: Option<f64>,
}

/// 会话列表筛选条件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionListFilter {
    Active,
    Archived,
}

/// 侧栏会话的互斥展示分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPlacementFilter {
    All,
    Pinned,
    Project,
    Automation,
    Recent,
}

pub(crate) fn now_epoch_secs() -> Result<f64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_secs_f64())
}

// ---------------------------------------------------------------------------
// 分支 / 谱系类型
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BranchKind {
    Fork,
    Side,
    Agent,
}

impl BranchKind {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            BranchKind::Fork => "fork",
            BranchKind::Side => "side",
            BranchKind::Agent => "agent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForkBoundary {
    ThroughTurn,
    BeforeTurn,
}

#[derive(Debug, Clone)]
pub struct ForkedSession {
    pub session_id: String,
    pub parent_session_id: String,
    pub parent_message_id: Option<i64>,
    pub parent_turn_index: Option<i64>,
    pub inherited_turn_count: i64,
    pub copied_message_count: i64,
    pub copied_user_turns: i64,
    pub created_at: f64,
}

/// 会话谱系节点中的单个 user-turn 锚点。
#[derive(Debug, Clone)]
pub struct SessionTurnNode {
    pub user_message_id: i64,
    pub turn_index: i64,
    pub content: Option<String>,
    pub completed: bool,
    pub child_session_ids: Vec<String>,
}

/// 谱系图中的一个会话节点。
#[derive(Debug, Clone)]
pub struct SessionLineageNode {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub parent_message_id: Option<i64>,
    pub parent_turn_index: Option<i64>,
    pub inherited_turn_count: i64,
    pub branch_created_at: Option<f64>,
    pub orphaned: bool,
    pub turns: Vec<SessionTurnNode>,
}

/// 会话树的完整谱系图。
#[derive(Debug, Clone)]
pub struct SessionLineageGraph {
    pub requested_session_id: String,
    pub root_session_id: String,
    pub nodes: Vec<SessionLineageNode>,
    pub cycle_detected: bool,
    pub orphaned_parent_ids: Vec<String>,
}

/// 单库会话存储：元数据、富消息行与消息级 FTS。
pub struct SessionStore {
    pub(crate) pool: SqlitePool,
    path: PathBuf,
}

impl SessionStore {
    /// 打开或创建 `state.db`。已有数据库必须匹配当前 schema。
    pub async fn open(path: &Path) -> Result<Self> {
        let parent = path.parent().unwrap_or(Path::new("."));
        let db = AstroDb::new(parent);
        let pool = db
            .open_pool_at_path(&DB_SPEC, path)
            .await
            .with_context(|| format!("open session store at {}", path.display()))?;
        sqlx::query("PRAGMA foreign_keys=ON").execute(&pool).await?;
        let store = Self {
            pool,
            path: path.to_path_buf(),
        };
        store.initialize_schema().await?;
        Ok(store)
    }

    /// 打开 `database_dir/state.db`。
    pub async fn open_sessions_dir(database_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(database_dir)
            .with_context(|| format!("create session database dir {}", database_dir.display()))?;
        Self::open(&database_dir.join("state.db")).await
    }

    /// 数据库文件路径
    pub fn db_path(&self) -> &Path {
        &self.path
    }

    /// 读取当前 `schema_version` 表中的版本号。
    pub async fn schema_version(&self) -> Result<i32> {
        let row = sqlx::query("SELECT version FROM schema_version LIMIT 1")
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(r) => Ok(r.get::<i32, _>(0)),
            None => Err(anyhow!("schema_version table is empty")),
        }
    }
}

#[async_trait::async_trait]
impl crate::ConversationStore for SessionStore {
    async fn invalidate_thread_notes(&self, session_id: &str) -> Result<()> {
        self.invalidate_thread_notes(session_id).await
    }
    async fn mark_memory_polluted(&self, session_id: &str) -> Result<()> {
        self.mark_memory_polluted(session_id).await
    }
    async fn is_memory_polluted(&self, session_id: &str) -> Result<bool> {
        self.is_memory_polluted(session_id).await
    }
    async fn thread_context(&self, session_id: &str) -> Result<ThreadContext> {
        self.thread_context(session_id).await
    }
    async fn write_thread_notes(
        &self,
        session_id: &str,
        notes: &str,
        expected_revision: i64,
    ) -> Result<i64> {
        self.write_thread_notes(session_id, notes, expected_revision)
            .await
    }
    async fn request_context_compaction(
        &self,
        session_id: &str,
        turn_id: &str,
        reason: &str,
    ) -> Result<()> {
        self.request_context_compaction(session_id, turn_id, reason)
            .await
    }
    async fn take_context_compaction(&self, session_id: &str, turn_id: &str) -> Result<bool> {
        self.take_context_compaction(session_id, turn_id).await
    }
    async fn finish_context_compaction(
        &self,
        session_id: &str,
        turn_id: &str,
        status: &str,
    ) -> Result<()> {
        self.finish_context_compaction(session_id, turn_id, status)
            .await
    }
    #[allow(refining_impl_trait)]
    async fn append_response_item(&self, item: NewResponseItem<'_>) -> Result<i64> {
        SessionStore::append_response_item(self, item).await
    }

    #[allow(refining_impl_trait)]
    async fn get_response_items(&self, session_id: &str) -> Result<Vec<StoredResponseItem>> {
        SessionStore::get_response_items(self, session_id).await
    }

    #[allow(refining_impl_trait)]
    async fn replace_response_items(
        &self,
        session_id: &str,
        items: &[agent_protocol::ResponseItem],
    ) -> Result<()> {
        SessionStore::replace_response_items(self, session_id, items).await
    }

    #[allow(refining_impl_trait)]
    async fn update_response_item_compressed_content(
        &self,
        message_id: i64,
        compressed: Option<&str>,
    ) -> Result<()> {
        SessionStore::update_response_item_compressed_content(self, message_id, compressed).await
    }

    #[allow(refining_impl_trait)]
    async fn patch_last_assistant_metadata(&self, session_id: &str, details: &Value) -> Result<()> {
        SessionStore::patch_last_assistant_metadata(self, session_id, details).await
    }

    #[allow(refining_impl_trait)]
    async fn ensure_session(&self, id: &str, source: &str) -> Result<()> {
        SessionStore::ensure_session(self, id, source).await
    }

    #[allow(refining_impl_trait)]
    async fn update_session_billing(&self, id: &str, d: BillingDelta) -> Result<()> {
        SessionStore::update_session_billing(self, id, d).await
    }

    #[allow(refining_impl_trait)]
    async fn recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<crate::ScrolledResponseItem>> {
        SessionStore::recent_messages(self, session_id, limit).await
    }

    #[allow(refining_impl_trait)]
    async fn recall_message_ids(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<i64>> {
        SessionStore::recall_message_ids(self, session_id, query, limit).await
    }

    #[allow(refining_impl_trait)]
    async fn scroll_context_window(
        &self,
        session_id: &str,
        around_message_id: i64,
        window_size: i64,
    ) -> Result<Vec<crate::ScrolledResponseItem>> {
        SessionStore::scroll_context_window(self, session_id, around_message_id, window_size).await
    }

    #[allow(refining_impl_trait)]
    async fn search_messages(
        &self,
        query: &str,
        source_filter: Option<&str>,
        role_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SearchHit>> {
        SessionStore::search_messages(self, query, source_filter, role_filter, limit).await
    }
}

impl types::SqliteStore for SessionStore {
    fn pool(&self) -> &types::SqlitePool {
        &self.pool
    }
}

pub(crate) fn is_unique_constraint(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(e) => e.code().is_some_and(|c| c == "2067"),
        _ => false,
    }
}

pub(crate) use types::truncate_chars;
