//! 模型目录提供的运行时能力契约。

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_context_percent() -> u8 {
    100
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelVerbosity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyPatchToolType {
    Freeform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchToolType {
    #[default]
    Text,
    TextAndImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelInputModality {
    Text,
    Image,
    Audio,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelMultiAgentVersion {
    V1,
    V2,
}

/// Provider 目录声明的模型级运行时能力。
///
/// 默认值保持现有 Responses 行为；权威目录可以显式收窄能力。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfile {
    #[serde(default = "default_true")]
    pub supports_search_tool: bool,
    #[serde(default = "default_true")]
    pub supports_parallel_tool_calls: bool,
    #[serde(default)]
    pub support_verbosity: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_verbosity: Option<ModelVerbosity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_patch_tool_type: Option<ApplyPatchToolType>,
    #[serde(default)]
    pub web_search_tool_type: WebSearchToolType,
    #[serde(default = "default_input_modalities")]
    pub input_modalities: Vec<ModelInputModality>,
    #[serde(default = "default_context_percent")]
    pub effective_context_window_percent: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_token_limit: Option<u64>,
    #[serde(default)]
    pub supports_reasoning_summaries: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_agent_version: Option<ModelMultiAgentVersion>,
}

fn default_input_modalities() -> Vec<ModelInputModality> {
    vec![ModelInputModality::Text]
}

impl Default for ModelProfile {
    fn default() -> Self {
        Self {
            supports_search_tool: true,
            supports_parallel_tool_calls: true,
            support_verbosity: false,
            default_verbosity: None,
            apply_patch_tool_type: None,
            web_search_tool_type: WebSearchToolType::Text,
            input_modalities: default_input_modalities(),
            effective_context_window_percent: default_context_percent(),
            auto_compact_token_limit: None,
            supports_reasoning_summaries: false,
            multi_agent_version: None,
        }
    }
}

impl ModelProfile {
    pub fn usable_context_window(&self, context_window: u64) -> u64 {
        context_window.saturating_mul(u64::from(self.effective_context_window_percent)) / 100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_existing_responses_tool_surface() {
        let profile = ModelProfile::default();
        assert!(profile.supports_search_tool);
        assert!(profile.supports_parallel_tool_calls);
        assert_eq!(profile.effective_context_window_percent, 100);
    }

    #[test]
    fn usable_context_window_reserves_declared_headroom() {
        let profile = ModelProfile {
            effective_context_window_percent: 95,
            ..ModelProfile::default()
        };
        assert_eq!(profile.usable_context_window(1_048_576), 996_147);
    }
}
