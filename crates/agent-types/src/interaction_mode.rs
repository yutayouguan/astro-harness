//! 聊天交互模式枚举（Agent / Plan）。

use serde::{Deserialize, Serialize};

/// 与前端 `ChatInteractionMode` 对齐的交互模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InteractionMode {
    #[default]
    Agent,
    Plan,
}

impl InteractionMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "agent" => Some(Self::Agent),
            "plan" => Some(Self::Plan),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Plan => "plan",
        }
    }

    pub fn system_guidance(self) -> &'static str {
        match self {
            Self::Agent => {
                "# Interaction mode: Agent / 交互模式：Agent\n\
Classify the request before using side-effect tools. Answer pure questions directly without a todo. For a bounded execution task with two or more concrete steps, create and maintain a compact todo list. For complex, high-risk, ambiguous, cross-module, or externally mutating work, call switch_mode(to=\"plan\", reason=…) before any side effect. Agent to Plan is automatic; returning to Agent requires explicit user review.\n\
使用有副作用的工具前先判断任务类型：纯问答直接回答，不创建 todo；有两个及以上明确步骤的简单执行任务，创建并持续更新紧凑 todo；复杂、高风险、需求不清、跨模块或涉及外部变更的任务，必须先调用 switch_mode(to=\"plan\", reason=…)。进入 Plan 自动完成，回到 Agent 必须等待用户明确审阅。"
            }
            Self::Plan => {
                "# Interaction mode: Plan (read-only planning) / 交互模式：Plan（只读规划）\n\
Read-only: web_search, todo. No writes, terminal, code_exec, agent-thread tools, or memory. Produce a concrete, reviewable plan with scope, steps, verification, and risks. When ready, call switch_mode(to=\"agent\", reason=…, summary=complete plan). Then stop and wait for explicit user approval; never assume approval or continue executing.\n\
可用 web_search、todo 等只读工具。禁止写文件、terminal、code_exec、Agent Thread 工具、memory。\n\
输出可审阅的具体计划，包含范围、步骤、验证与风险。计划就绪后调用 switch_mode(to=\"agent\", reason=…, summary=完整计划)，随后停止并等待用户明确批准，不得自行继续执行。"
            }
        }
    }

    pub fn is_readonly_gate(self) -> bool {
        self == Self::Plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_agent_and_plan() {
        assert_eq!(InteractionMode::parse(""), Some(InteractionMode::Agent));
        assert_eq!(
            InteractionMode::parse(" agent "),
            Some(InteractionMode::Agent)
        );
        assert_eq!(InteractionMode::parse("PLAN"), Some(InteractionMode::Plan));
        assert_eq!(InteractionMode::parse("ask"), None);
        assert_eq!(InteractionMode::parse("unknown"), None);
    }
}
