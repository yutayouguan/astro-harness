//! Step 级不可变状态——单次模型采样请求的会话快照与工具集。

use std::sync::Arc;

use providers::types::message::Message as ProviderMessage;
use types::message::Message;

use super::{ToolRouter, TurnContext};

/// 单次采样请求的快照：turn 上下文、历史消息、可用工具。
#[derive(Debug)]
pub(crate) struct StepContext {
    pub(crate) turn: Arc<TurnContext>,
    pub(crate) history: Vec<Message>,
    pub(crate) prompt_context: Vec<ProviderMessage>,
    pub(crate) tool_router: Arc<ToolRouter>,
}

impl StepContext {
    pub(crate) fn new(
        turn: Arc<TurnContext>,
        history: Vec<Message>,
        prompt_context: Vec<ProviderMessage>,
        tool_router: Arc<ToolRouter>,
    ) -> Self {
        Self {
            turn,
            history,
            prompt_context,
            tool_router,
        }
    }

    /// 该工具是否在本次采样请求中被提供给模型。
    pub(crate) fn advertises_tool(&self, name: &str) -> bool {
        self.tool_router.has_tool(name)
    }
}
