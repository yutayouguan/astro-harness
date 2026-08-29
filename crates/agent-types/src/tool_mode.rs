//! 与 Codex 对齐的模型工具暴露模式。

use serde::{Deserialize, Serialize};

/// 单次模型采样时的工具暴露方式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolMode {
    /// 直接暴露常规工具，不暴露 Code Mode 控制工具。
    #[default]
    Direct,
    /// 同时暴露常规工具与 `exec` / `wait`。
    CodeMode,
    /// 仅暴露 `exec` / `wait`；常规工具仍可在 `exec` 内调用。
    CodeModeOnly,
}

impl ToolMode {
    /// 当前 Codex 模型目录的默认值；未知模型保持 Direct。
    pub fn codex_default_for_model(model_id: &str) -> Option<Self> {
        match model_id.trim().to_ascii_lowercase().as_str() {
            "gpt-5.6-sol"
            | "gpt-5.6-terra"
            | "gpt-5.6-luna"
            | "gpt-daybreak-blue-latest"
            | "gpt-daybreak-red-latest"
            | "codex-auto-review" => Some(Self::CodeModeOnly),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_current_codex_catalog_defaults() {
        assert_eq!(
            ToolMode::codex_default_for_model("gpt-5.6-sol"),
            Some(ToolMode::CodeModeOnly)
        );
        assert_eq!(ToolMode::codex_default_for_model("gpt-5.5"), None);
    }
}
