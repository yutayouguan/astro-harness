//! 会话级可变运行时状态。
//!
//! 本边界遵循 `SessionState` 模式：跨采样步骤存活的可变状态聚合在一起，
//! 而活跃任务注册表则直接保留在 [`super::Session`] 上。[`super::Session`]
//! 通过短生命周期互斥锁持有本容器，确保调用方不会暴露与状态守卫绑定的引用。

use agent_protocol::ResponseItem;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use types::message::Message;

use super::{compression_state, model_ctx, turn_budget, StepContext, TurnContext};

/// 此前直接存储在 [`super::Session`] 上的持久化可变状态。
pub(crate) struct SessionState {
    /// Canonical model history. Chat-shaped [`Message`] values are only
    /// compatibility projections for storage/search/UI consumers.
    pub(crate) history: Vec<ResponseItem>,
    pub(crate) pending_session_start_source: Option<String>,
    pub(crate) model_ctx: model_ctx::ModelContext,
    pub(crate) compression: compression_state::CompressionState,
    pub(crate) turn: turn_budget::TurnState,
    pub(crate) pending_inject_context: Option<String>,
    pub(crate) pending_learning_nudge: Option<String>,
    /// 用于 WorldState 差分比较的最新持久化角色上下文基线。
    pub(crate) prompt_context_snapshot: Option<Value>,
    /// 模型可见的初始上下文及源级别更新，保存在聊天存储之外。
    pub(crate) prompt_context_history: Vec<crate::prompt::context_state::PromptContextEvent>,
    pub(crate) interaction_mode: types::InteractionMode,
    pub(crate) current_turn_context: Option<Arc<TurnContext>>,
    pub(crate) current_step_context: Option<Arc<StepContext>>,
    pub(crate) mcp_config_override: Vec<mcp::McpServerConfig>,
    pub(crate) mcp_instructions: Vec<mcp::McpServerInstructions>,
    pub(crate) project_root: Option<PathBuf>,
    pub(crate) workspace_roots: Vec<PathBuf>,
    pub(crate) permission_profile: Option<String>,
    pub(crate) skill_config_overrides: Vec<(PathBuf, bool)>,
    pub(crate) temperature: f32,
    pub(crate) additional_params: Value,
}

impl SessionState {
    pub(crate) fn new(history: Vec<ResponseItem>, project_root: Option<PathBuf>) -> Self {
        let session_start_source = if history.is_empty() {
            "startup"
        } else {
            "resume"
        };
        Self {
            history,
            pending_session_start_source: Some(session_start_source.to_string()),
            model_ctx: model_ctx::ModelContext::default(),
            compression: compression_state::CompressionState::default(),
            turn: turn_budget::TurnState::default(),
            pending_inject_context: None,
            pending_learning_nudge: None,
            prompt_context_snapshot: None,
            prompt_context_history: Vec::new(),
            interaction_mode: types::InteractionMode::Agent,
            current_turn_context: None,
            current_step_context: None,
            mcp_config_override: Vec::new(),
            mcp_instructions: Vec::new(),
            workspace_roots: project_root.iter().cloned().collect(),
            project_root,
            permission_profile: None,
            skill_config_overrides: Vec::new(),
            temperature: 0.7,
            additional_params: Value::Null,
        }
    }

    pub(crate) fn record_items<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = ResponseItem>,
    {
        self.history.extend(items);
    }

    pub(crate) fn clone_response_history(&self) -> Vec<ResponseItem> {
        self.history.clone()
    }

    pub(crate) fn clone_message_projection(&self) -> Vec<Message> {
        agent_rollout::reconstruct_response_items(self.history.clone())
            .expect("projecting response items as messages cannot fail")
            .into_iter()
            .map(|item| item.message)
            .collect()
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new(Vec::new(), None)
    }
}
