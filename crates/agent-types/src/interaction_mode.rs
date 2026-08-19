//! 聊天交互模式枚举（Agent / Plan / Ask）。

use serde::{Deserialize, Serialize};

/// 与前端 `ChatInteractionMode` 对齐的交互模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InteractionMode {
    #[default]
    Agent,
    Plan,
    Ask,
}

impl InteractionMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "plan" => Self::Plan,
            "ask" => Self::Ask,
            _ => Self::Agent,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Plan => "plan",
            Self::Ask => "ask",
        }
    }

    pub fn system_guidance(self) -> &'static str {
        match self {
            Self::Agent => {
                "# Interaction mode: Agent / 交互模式：Agent\n\
Tools enabled. For complex multi-step work, call switch_mode(to=\"plan\", reason=…) first, then return to Agent after authorization.\n\
可执行工具完成任务。复杂多步工作可先调用 switch_mode(to=\"plan\", reason=…) 进入规划，再在授权后回到 Agent 执行。"
            }
            Self::Plan => {
                "# Interaction mode: Plan (read-only planning) / 交互模式：Plan（只读规划）\n\
Read-only: file_ops(read/list/search), web_search, todo. No writes, terminal, code_exec, agent-thread tools, or memory. When ready, call switch_mode(to=\"agent\", reason=…, summary=plan summary).\n\
可用 file_ops(read/list/search)、web_search、todo 等只读工具。禁止写文件、terminal、code_exec、Agent Thread 工具、memory。\n\
计划就绪后调用 switch_mode(to=\"agent\", reason=…, summary=计划摘要) 请求执行授权。"
            }
            Self::Ask => {
                "# Interaction mode: Ask (read-only Q&A) / 交互模式：Ask（只读问答）\n\
Explain and retrieve; do not modify files or run side effects. To implement, call switch_mode(to=\"agent\", reason=…, summary=plan).\n\
以解释与检索为主，不要修改文件或执行有副作用的操作。若需落地实现，可 switch_mode(to=\"agent\", reason=…, summary=计划摘要)。"
            }
        }
    }

    pub fn is_readonly_gate(self) -> bool {
        matches!(self, Self::Plan | Self::Ask)
    }
}
