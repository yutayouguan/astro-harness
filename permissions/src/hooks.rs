//! Hook 总线：在工具 / 技能 / 记忆等节点插入可拦截回调。

use std::sync::Arc;

/// 可挂钩的生命周期事件。
#[derive(Debug, Clone)]
pub enum HookEvent {
    /// 工具调用前。
    PreToolCall { tool_name: String, args: serde_json::Value },
    /// 工具调用后。
    PostToolCall { tool_name: String, result: String },
    /// 技能执行前。
    PreSkillExec { skill_name: String },
    /// 技能执行后。
    PostSkillExec { skill_name: String, output: String },
    /// 写入记忆时。
    OnMemoryWrite { content: String },
    /// Agent 每一回合开始时（`turn` 从 0 或实现约定起算）。
    OnAgentLoop { turn: usize },
}

/// Hook 返回的动作；非 `Continue` 时总线提前结束。
#[derive(Debug, Clone)]
pub enum HookAction {
    /// 继续后续 hook。
    Continue,
    /// 用新 JSON 替换参数等（由调用方解释）。
    Modify(serde_json::Value),
    /// 阻断并附带原因。
    Block(String),
    /// 显式批准（调用方可作特殊语义）。
    Approve,
}

/// 同步 hook 回调类型。
type HookFn = Arc<dyn Fn(&HookEvent) -> HookAction + Send + Sync>;

/// 按注册顺序触发 hook；首个非 `Continue` 结果即返回。
pub struct HookBus {
    /// 已注册回调。
    hooks: Vec<HookFn>,
}

impl HookBus {
    /// 创建空总线。
    pub fn new() -> Self { HookBus { hooks: Vec::new() } }

    /// 注册一个 hook（顺序追加）。
    pub fn register<F>(&mut self, f: F)
    where F: Fn(&HookEvent) -> HookAction + Send + Sync + 'static
    {
        self.hooks.push(Arc::new(f));
    }

    /// 依次触发；全部 `Continue` 则返回 `Continue`。
    pub async fn fire(&self, event: HookEvent) -> HookAction {
        for hook in &self.hooks {
            let action = hook(&event);
            if !matches!(action, HookAction::Continue) {
                return action;
            }
        }
        HookAction::Continue
    }
}

impl Default for HookBus {
    /// 等价于 [`HookBus::new`]。
    fn default() -> Self { Self::new() }
}
