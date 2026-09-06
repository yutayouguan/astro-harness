//! 辅助模型任务类型与跨进程目标链。
//!
//! `AuxiliaryTask` 与 `memory::AuxiliaryKind` 一一对应，但独立定义在 `common`：
//! `common` 位于依赖图底层（无 `memory` 依赖），供 `agent` / `backend` / `proto` 边界共用。
//! `AuxiliaryTargetChain` 随 `ChatRequest` 透传给后端，仅在内存中持有，不落盘（含 API key）。

use crate::ModelTarget;
use serde::{Deserialize, Serialize};

/// 辅助任务类型：标题生成 / 压缩 / 智能审批 / 入梦 / 回合后自我改进 review / 工作流 AI 辅助。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuxiliaryTask {
    TitleGeneration,
    Compaction,
    SmartApproval,
    Dreaming,
    BackgroundReview,
    /// 工作流配置面板 ✨ AI 润色/生成按钮使用的模型。
    WorkflowAiPolish,
}

impl AuxiliaryTask {
    pub const ALL: [Self; 6] = [
        Self::TitleGeneration,
        Self::Compaction,
        Self::SmartApproval,
        Self::Dreaming,
        Self::BackgroundReview,
        Self::WorkflowAiPolish,
    ];

    /// 稳定字符串 id（对齐 `memory::AuxiliaryKind::config_key`），用于跨进程透传与配置读写。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TitleGeneration => "title_generation",
            Self::Compaction => "compaction",
            Self::SmartApproval => "smart_approval",
            Self::Dreaming => "dreaming",
            Self::BackgroundReview => "background_review",
            Self::WorkflowAiPolish => "workflow_ai_polish",
        }
    }

    /// 从字符串 id 解析；未知值返回 `None`（调用方按需跳过，不 panic）。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "title_generation" => Some(Self::TitleGeneration),
            "compaction" => Some(Self::Compaction),
            "smart_approval" => Some(Self::SmartApproval),
            "dreaming" => Some(Self::Dreaming),
            "background_review" => Some(Self::BackgroundReview),
            "workflow_ai_polish" => Some(Self::WorkflowAiPolish),
            _ => None,
        }
    }
}

/// 单个辅助任务的已解析目标链：`targets[0]` 为 preferred，`targets[1]`（若有）为 fallback。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuxiliaryTargetChain {
    pub task: AuxiliaryTask,
    pub targets: Vec<ModelTarget>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_strings_round_trip() {
        for task in AuxiliaryTask::ALL {
            let s = task.as_str();
            assert_eq!(AuxiliaryTask::parse(s), Some(task));
        }
    }

    #[test]
    fn parse_rejects_unknown() {
        assert_eq!(AuxiliaryTask::parse("nope"), None);
        assert_eq!(AuxiliaryTask::parse(""), None);
    }

    #[test]
    fn serde_uses_snake_case() {
        let json = serde_json::to_string(&AuxiliaryTask::SmartApproval).unwrap();
        assert_eq!(json, "\"smart_approval\"");
        let back: AuxiliaryTask = serde_json::from_str(&json).unwrap();
        assert_eq!(back, AuxiliaryTask::SmartApproval);
    }
}
