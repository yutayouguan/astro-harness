//! 辅助模型任务类型与跨进程目标链。
//!
//! `AuxiliaryTask` 与 `memory::AuxiliaryKind` 一一对应，但独立定义在 `common`：
//! `common` 位于依赖图底层（无 `memory` 依赖），供 `agent` / `backend` / `proto` 边界共用。
//! `AuxiliaryTargetChain` 随 `ChatRequest` 透传给后端，仅在内存中持有，不落盘（含 API key）。

use crate::ChatTarget;
use serde::{Deserialize, Serialize};

/// 辅助任务类型：标题生成 / 压缩 / 智能审批 / 入梦 / 回合后自我改进 review。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuxiliaryTask {
    TitleGeneration,
    Compaction,
    SmartApproval,
    Dreaming,
    BackgroundReview,
}

impl AuxiliaryTask {
    pub const ALL: [Self; 5] = [
        Self::TitleGeneration,
        Self::Compaction,
        Self::SmartApproval,
        Self::Dreaming,
        Self::BackgroundReview,
    ];

    /// 稳定字符串 id（对齐 `memory::AuxiliaryKind::config_key`），用于跨进程透传与配置读写。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TitleGeneration => "title_generation",
            Self::Compaction => "compaction",
            Self::SmartApproval => "smart_approval",
            Self::Dreaming => "dreaming",
            Self::BackgroundReview => "background_review",
        }
    }

    /// 从字符串 id 解析；未知值返回 `None`（调用方按需跳过，不 panic）。
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "title_generation" => Some(Self::TitleGeneration),
            "compaction" => Some(Self::Compaction),
            "smart_approval" => Some(Self::SmartApproval),
            "dreaming" => Some(Self::Dreaming),
            "background_review" => Some(Self::BackgroundReview),
            _ => None,
        }
    }
}

/// 单个辅助任务的已解析目标链：`targets[0]` 为 preferred，`targets[1]`（若有）为 fallback。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuxiliaryTargetChain {
    pub task: AuxiliaryTask,
    pub targets: Vec<ChatTarget>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_strings_round_trip() {
        for task in AuxiliaryTask::ALL {
            let s = task.as_str();
            assert_eq!(AuxiliaryTask::from_str(s), Some(task));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert_eq!(AuxiliaryTask::from_str("nope"), None);
        assert_eq!(AuxiliaryTask::from_str(""), None);
    }

    #[test]
    fn serde_uses_snake_case() {
        let json = serde_json::to_string(&AuxiliaryTask::SmartApproval).unwrap();
        assert_eq!(json, "\"smart_approval\"");
        let back: AuxiliaryTask = serde_json::from_str(&json).unwrap();
        assert_eq!(back, AuxiliaryTask::SmartApproval);
    }
}
