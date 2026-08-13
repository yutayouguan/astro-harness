//! 审批动作与模式枚举。

/// 审批动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalAction {
    /// 直接拒绝，不弹卡、不执行。
    Deny,
    /// 弹出 HITL 确认。
    Ask,
    /// 低危白名单，自动放行。
    Auto,
}

/// 分级结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalDecision {
    pub action: ApprovalAction,
    pub description: &'static str,
}

/// 审批模式（对齐 Hermes：`smart` | `manual` | `off`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApprovalMode {
    #[default]
    Smart,
    Manual,
    Off,
}

impl ApprovalMode {
    pub fn parse_lenient(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "manual" => Self::Manual,
            "off" | "yolo" => Self::Off,
            _ => Self::Smart,
        }
    }
}
