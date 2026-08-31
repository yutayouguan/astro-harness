//! Step 级不可变状态——单次模型采样请求的会话快照与工具集。

use std::sync::Arc;

use super::{ToolRouter, TurnContext};

/// 单次采样请求的快照：turn 上下文、历史消息、可用工具。
#[derive(Debug)]
pub(crate) struct StepContext {
    pub(crate) turn: Arc<TurnContext>,
    pub(crate) history: Vec<agent_protocol::ResponseItem>,
    pub(crate) prompt_context: Vec<crate::prompt::context_state::PromptContextEvent>,
    pub(crate) tool_router: Arc<ToolRouter>,
}

impl StepContext {
    pub(crate) fn new(
        turn: Arc<TurnContext>,
        history: Vec<agent_protocol::ResponseItem>,
        prompt_context: Vec<crate::prompt::context_state::PromptContextEvent>,
        tool_router: Arc<ToolRouter>,
    ) -> Self {
        Self {
            turn,
            history,
            prompt_context,
            tool_router,
        }
    }

    /// 工具是否存在于本次 Step 捕获的完整可调用路由中。
    pub(crate) fn routes_tool(&self, name: &str) -> bool {
        self.tool_router.has_tool(name)
    }
}
