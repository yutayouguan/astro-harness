//! 钩子载荷与返回动作。

use serde_json::Value;

/// 传入钩子回调的上下文快照。
#[derive(Debug, Clone, Default)]
pub struct HookPayload {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub detail: String,
    pub tool_name: Option<String>,
    pub tool_args: Option<Value>,
    pub tool_result: Option<String>,
    pub message: Option<String>,
    pub system_prompt_chars: Option<usize>,
    pub assistant_chars: Option<usize>,
    pub turn: Option<usize>,
    pub error: Option<String>,
}

/// 钩子返回值；观察型应返回 [`Continue`](Self::Continue)。
#[derive(Debug, Clone)]
pub enum HookOutcome {
    Continue,
    /// `pre_tool_call`：阻断工具执行。
    Block(String),
    /// `pre_tool_call`：替换参数。
    Modify(Value),
    /// `pre_llm_call`：注入本轮附加上下文。
    InjectContext(String),
    /// `pre_gateway_dispatch`：放行。
    Allow,
    /// `pre_gateway_dispatch`：跳过入队。
    Skip(String),
    /// `pre_gateway_dispatch`：改写用户消息。
    Rewrite(String),
}

impl Default for HookOutcome {
    fn default() -> Self {
        Self::Continue
    }
}

impl HookOutcome {
    pub fn is_continue(&self) -> bool {
        matches!(self, Self::Continue | Self::Allow)
    }
}
