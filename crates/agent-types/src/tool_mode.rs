//! 模型工具暴露模式与 feature flag 选择规则。

use serde::{Deserialize, Serialize};

/// 单次模型采样时的工具暴露方式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolMode {
    /// 仅向模型直接暴露普通工具。
    #[default]
    Direct,
    /// 同时暴露普通工具与 `exec` / `wait`。
    CodeMode,
    /// 仅暴露 `exec` / `wait`，普通工具只能由 JavaScript 间接调用。
    CodeModeOnly,
}

impl ToolMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::CodeMode => "code_mode",
            Self::CodeModeOnly => "code_mode_only",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "direct" => Some(Self::Direct),
            "code_mode" | "codemode" => Some(Self::CodeMode),
            "code_mode_only" | "codemodeonly" => Some(Self::CodeModeOnly),
            _ => None,
        }
    }
}

/// 模型目录来自服务端，未知枚举值应按“未指定”处理，以便旧客户端继续使用
/// feature flag，而不是因为服务端先发布新模式就拒绝整份目录。
pub fn deserialize_optional_tool_mode<'de, D>(deserializer: D) -> Result<Option<ToolMode>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.as_deref().and_then(ToolMode::parse))
}

/// 未被模型目录覆盖时使用的实验功能开关。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolModeFeatureFlags {
    #[serde(default)]
    pub code_mode: bool,
    #[serde(default)]
    pub code_mode_only: bool,
}

impl ToolModeFeatureFlags {
    /// `code_mode_only` 权限边界更窄，因此两个开关同时启用时优先采用它。
    pub fn requested_mode(self) -> ToolMode {
        if self.code_mode_only {
            ToolMode::CodeModeOnly
        } else if self.code_mode {
            ToolMode::CodeMode
        } else {
            ToolMode::Direct
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_mode_only_feature_has_priority() {
        assert_eq!(
            ToolModeFeatureFlags {
                code_mode: true,
                code_mode_only: true,
            }
            .requested_mode(),
            ToolMode::CodeModeOnly
        );
    }

    #[test]
    fn unknown_catalog_mode_is_treated_as_omitted() {
        #[derive(Deserialize)]
        struct CatalogEntry {
            #[serde(default, deserialize_with = "deserialize_optional_tool_mode")]
            tool_mode: Option<ToolMode>,
        }

        let entry: CatalogEntry =
            serde_json::from_str(r#"{"tool_mode":"future_tool_mode"}"#).unwrap();
        assert_eq!(entry.tool_mode, None);
    }
}
