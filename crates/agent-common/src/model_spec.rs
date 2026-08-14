//! Agno 风格模型声明：`provider:model_id` 简写 + 可选采样参数。
//!
//! 与运行时 [`crate::ChatTarget`]（含 API key）分离：`ModelSpec` 是「要用哪家模型」的
//! 声明；凭据由钥匙串 / 环境解析后再 [`ModelSpec::apply_to`] 或 [`ModelSpec::to_chat_target`]。

use crate::auxiliary_target::AuxiliaryTask;
use crate::chat_target::ChatTarget;
use serde::{Deserialize, Serialize};

/// 模型角色：主聊或辅助任务（对齐 Agno `ModelType`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    /// 主对话模型。
    Main,
    /// 辅助任务模型（标题 / 压缩 / 审批 / 入梦 / review）。
    Auxiliary(AuxiliaryTask),
}

/// 模型规格声明（不含 API key）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSpec {
    /// Provider / backend id，如 `openai`、`claude`、`google`。
    pub provider_id: String,
    /// 模型 id，如 `gpt-5.6`、`claude-sonnet-4-5`。
    pub model_id: String,
    /// 可选采样温度；`None` 表示沿用 Agent / 默认配置。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// 可选 max_tokens；`None` 表示沿用默认。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

impl ModelSpec {
    pub fn new(provider_id: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            temperature: None,
            max_tokens: None,
        }
    }

    pub fn with_temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }

    pub fn with_max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = Some(n);
        self
    }

    /// 解析 `"provider:model_id"` 或 `"provider/model_id"`。
    ///
    /// - 含 `:` 或 `/`：左侧为 provider，右侧为 model（右侧可再含 `:`，如带日期的 Claude id）
    /// - 仅 model：`provider_id` 为空，由调用方填当前主 provider
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let s = s.trim();
        if s.is_empty() {
            anyhow::bail!("model spec cannot be empty");
        }
        if let Some((prov, model)) = s.split_once(':') {
            let provider_id = prov.trim();
            let model_id = model.trim();
            if provider_id.is_empty() || model_id.is_empty() {
                anyhow::bail!("invalid model spec `{s}` (expected provider:model_id)");
            }
            return Ok(Self::new(provider_id, model_id));
        }
        if let Some((prov, model)) = s.split_once('/') {
            let provider_id = prov.trim();
            let model_id = model.trim();
            if provider_id.is_empty() || model_id.is_empty() {
                anyhow::bail!("invalid model spec `{s}` (expected provider/model_id)");
            }
            return Ok(Self::new(provider_id, model_id));
        }
        Ok(Self::new("", s))
    }

    /// 规范化简写：`provider:model_id`（provider 空时仅 model）。
    pub fn as_str(&self) -> String {
        if self.provider_id.trim().is_empty() {
            self.model_id.clone()
        } else {
            format!("{}:{}", self.provider_id.trim(), self.model_id.trim())
        }
    }

    /// 用本规格覆盖 `target` 的 provider/model；保留 api_key / base_url。
    ///
    /// 若 `provider_id` 为空，保留 target 原有 provider。
    pub fn apply_to(&self, target: &ChatTarget) -> ChatTarget {
        let provider = if self.provider_id.trim().is_empty() {
            target.backend_id.clone()
        } else {
            self.provider_id.trim().to_string()
        };
        let model = if self.model_id.trim().is_empty() {
            target.model.clone()
        } else {
            self.model_id.trim().to_string()
        };
        ChatTarget {
            provider_id: provider.clone(),
            backend_id: provider,
            model,
            api_key: target.api_key.clone(),
            base_url: target.base_url.clone(),
        }
    }

    /// 从凭据构造完整 [`ChatTarget`]。
    pub fn to_chat_target(
        &self,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
    ) -> ChatTarget {
        let provider = self.provider_id.trim().to_string();
        ChatTarget {
            provider_id: provider.clone(),
            backend_id: provider,
            model: self.model_id.trim().to_string(),
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_provider_colon_model() {
        let s = ModelSpec::parse("claude:claude-sonnet-4-5-20250929").unwrap();
        assert_eq!(s.provider_id, "claude");
        assert_eq!(s.model_id, "claude-sonnet-4-5-20250929");
        assert_eq!(s.as_str(), "claude:claude-sonnet-4-5-20250929");
    }

    #[test]
    fn parse_slash_and_bare_model() {
        let s = ModelSpec::parse("openai/gpt-5.6").unwrap();
        assert_eq!(s.provider_id, "openai");
        assert_eq!(s.model_id, "gpt-5.6");
        let bare = ModelSpec::parse("gpt-5.6").unwrap();
        assert!(bare.provider_id.is_empty());
        assert_eq!(bare.model_id, "gpt-5.6");
    }

    #[test]
    fn apply_to_keeps_credentials() {
        let base = ChatTarget {
            provider_id: "openai".into(),
            backend_id: "openai".into(),
            model: "old".into(),
            api_key: "sk".into(),
            base_url: "https://api.openai.com/v1".into(),
        };
        let spec = ModelSpec::parse("claude:opus").unwrap();
        let t = spec.apply_to(&base);
        assert_eq!(t.backend_id, "claude");
        assert_eq!(t.model, "opus");
        assert_eq!(t.api_key, "sk");
        assert_eq!(t.base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn bare_model_keeps_provider_on_apply() {
        let base = ChatTarget {
            provider_id: "google".into(),
            backend_id: "google".into(),
            model: "old".into(),
            api_key: "k".into(),
            base_url: "https://generativelanguage.googleapis.com".into(),
        };
        let t = ModelSpec::parse("gemini-2.5-flash")
            .unwrap()
            .apply_to(&base);
        assert_eq!(t.backend_id, "google");
        assert_eq!(t.model, "gemini-2.5-flash");
    }
}
