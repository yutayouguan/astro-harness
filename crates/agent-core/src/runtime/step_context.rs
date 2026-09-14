//! Step 级不可变状态——单次模型采样请求的会话快照与工具集。

use std::sync::Arc;

use super::turn_context::TurnProviderSettings;
use super::{ToolRouter, TurnContext};

/// 单次采样请求的快照：turn 上下文、历史消息、可用工具。
#[derive(Debug)]
pub(crate) struct StepContext {
    pub(crate) turn: Arc<TurnContext>,
    /// 本次 sampling 捕获的 provider/model/request 设置版本。
    ///
    /// 活跃 turn 一定会在首个 Step 前初始化该值；独立测试或历史工具路径可能没有
    /// provider，因此保留 `None`，但不得回读 live turn 设置来改变已发出的 Step。
    pub(crate) provider_settings: Option<Arc<TurnProviderSettings>>,
    pub(crate) history: Vec<agent_protocol::ResponseItem>,
    pub(crate) prompt_context: Vec<crate::prompt::context_state::PromptContextEvent>,
    pub(crate) tool_router: Arc<ToolRouter>,
}

impl StepContext {
    pub(crate) fn new(
        turn: Arc<TurnContext>,
        provider_settings: Option<Arc<TurnProviderSettings>>,
        history: Vec<agent_protocol::ResponseItem>,
        prompt_context: Vec<crate::prompt::context_state::PromptContextEvent>,
        tool_router: Arc<ToolRouter>,
    ) -> Self {
        Self {
            turn,
            provider_settings,
            history,
            prompt_context,
            tool_router,
        }
    }

    /// 工具是否存在于本次 Step 捕获的完整可调用路由中。
    pub(crate) fn routes_tool(&self, name: &str) -> bool {
        self.tool_router.has_tool(None, name)
    }

    pub(crate) fn provider_settings(&self) -> Option<&TurnProviderSettings> {
        self.provider_settings.as_deref()
    }
}
