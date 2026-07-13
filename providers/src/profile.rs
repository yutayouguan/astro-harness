//! Hermes 风格 `ProviderProfile` 表：id → ApiMode / 默认 base / 认证。

use crate::trait_::AuthKind;

/// 底层协议适配器种类（对齐 Hermes 三协议）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiMode {
    /// OpenAI Chat Completions（含兼容网关、Gemini OpenAI compat、Ollama `/v1`）。
    ChatCompletions,
    /// Anthropic Messages API。
    AnthropicMessages,
    /// OpenAI Responses API（骨架；默认表暂无绑定）。
    Responses,
}

/// 单个供应商的静态配置。
#[derive(Debug, Clone, Copy)]
pub struct ProviderProfile {
    /// 注册表 id，如 `openai`、`google`。
    pub id: &'static str,
    /// 协议模式。
    pub api_mode: ApiMode,
    /// 默认 API 基址；Azure 为空字符串表示必须由用户配置。
    pub default_base_url: &'static str,
    /// 认证方式。
    pub auth: AuthKind,
    /// 依次尝试的环境变量名。
    pub env_keys: &'static [&'static str],
    /// Azure：deployment 风格 URL + `api-key` header。
    pub azure_deployment_style: bool,
    /// 默认推荐模型 / deployment id。
    pub default_model: &'static str,
    /// 是否支持图片生成。
    pub supports_image_gen: bool,
    /// 是否支持 embedding。
    pub supports_embedding: bool,
}

/// 内置 profile 表（含全部 registry id）。
pub static PROFILES: &[ProviderProfile] = &[
    ProviderProfile {
        id: "openai",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://api.openai.com/v1",
        auth: AuthKind::Bearer,
        env_keys: &["OPENAI_API_KEY"],
        azure_deployment_style: false,
        default_model: "gpt-5.6",
        supports_image_gen: true,
        supports_embedding: true,
    },
    ProviderProfile {
        id: "claude",
        api_mode: ApiMode::AnthropicMessages,
        default_base_url: "https://api.anthropic.com",
        auth: AuthKind::AnthropicKey,
        env_keys: &["ANTHROPIC_API_KEY", "CLAUDE_API_KEY"],
        azure_deployment_style: false,
        default_model: "claude-opus-4-8",
        supports_image_gen: false,
        supports_embedding: false,
    },
    ProviderProfile {
        id: "deepseek",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://api.deepseek.com/v1",
        auth: AuthKind::Bearer,
        env_keys: &["DEEPSEEK_API_KEY"],
        azure_deployment_style: false,
        default_model: "deepseek-chat",
        supports_image_gen: false,
        supports_embedding: false,
    },
    ProviderProfile {
        id: "google",
        api_mode: ApiMode::ChatCompletions,
        // Gemini OpenAI 兼容：https://ai.google.dev/gemini-api/docs/openai
        default_base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        auth: AuthKind::Bearer,
        env_keys: &["GOOGLE_API_KEY", "GEMINI_API_KEY", "GOOGLE_AI_API_KEY"],
        azure_deployment_style: false,
        default_model: "gemini-3.5-flash",
        supports_image_gen: true,
        supports_embedding: false,
    },
    ProviderProfile {
        id: "ollama",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "http://localhost:11434/v1",
        auth: AuthKind::None,
        env_keys: &[],
        azure_deployment_style: false,
        default_model: "llama3.3",
        supports_image_gen: false,
        supports_embedding: false,
    },
    ProviderProfile {
        id: "azure",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "",
        auth: AuthKind::AzureHeader,
        env_keys: &["AZURE_OPENAI_API_KEY", "AZURE_API_KEY"],
        azure_deployment_style: true,
        default_model: "gpt-5.6",
        supports_image_gen: false,
        supports_embedding: false,
    },
    ProviderProfile {
        id: "zhipu",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://open.bigmodel.cn/api/paas/v4",
        auth: AuthKind::Bearer,
        env_keys: &["ZHIPU_API_KEY", "BIGMODEL_API_KEY"],
        azure_deployment_style: false,
        default_model: "glm-4.7-flash",
        supports_image_gen: false,
        supports_embedding: false,
    },
    ProviderProfile {
        id: "openrouter",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://openrouter.ai/api/v1",
        auth: AuthKind::Bearer,
        env_keys: &["OPENROUTER_API_KEY"],
        azure_deployment_style: false,
        default_model: "openai/gpt-5.6",
        supports_image_gen: false,
        supports_embedding: false,
    },
    ProviderProfile {
        id: "bailian",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        auth: AuthKind::Bearer,
        env_keys: &["DASHSCOPE_API_KEY", "BAILIAN_API_KEY"],
        azure_deployment_style: false,
        default_model: "qwen3.6-plus",
        supports_image_gen: false,
        supports_embedding: false,
    },
    ProviderProfile {
        id: "nvidia",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://integrate.api.nvidia.com/v1",
        auth: AuthKind::Bearer,
        env_keys: &["NVIDIA_API_KEY"],
        azure_deployment_style: false,
        default_model: "meta/llama-3.3-70b-instruct",
        supports_image_gen: false,
        supports_embedding: false,
    },
    ProviderProfile {
        id: "moonshot",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://api.moonshot.cn/v1",
        auth: AuthKind::Bearer,
        env_keys: &["MOONSHOT_API_KEY", "KIMI_API_KEY"],
        azure_deployment_style: false,
        default_model: "kimi-k2.5",
        supports_image_gen: false,
        supports_embedding: false,
    },
    ProviderProfile {
        id: "volcengine",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://ark.cn-beijing.volces.com/api/v3",
        auth: AuthKind::Bearer,
        env_keys: &["ARK_API_KEY", "VOLCENGINE_API_KEY"],
        azure_deployment_style: false,
        default_model: "ep-",
        supports_image_gen: false,
        supports_embedding: false,
    },
    ProviderProfile {
        id: "minimax",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://api.minimax.chat/v1",
        auth: AuthKind::Bearer,
        env_keys: &["MINIMAX_API_KEY", "MINMAX_API_KEY"],
        azure_deployment_style: false,
        default_model: "MiniMax-M2.5",
        supports_image_gen: false,
        supports_embedding: false,
    },
    ProviderProfile {
        id: "mimo",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://api.xiaomimimo.com/v1",
        auth: AuthKind::Bearer,
        env_keys: &["MIMO_API_KEY"],
        azure_deployment_style: false,
        default_model: "mimo-v2-flash",
        supports_image_gen: false,
        supports_embedding: false,
    },
];

/// 规范化别名后查找 profile。
pub fn resolve(provider_id: &str) -> Option<&'static ProviderProfile> {
    let id = normalize_provider_id(provider_id);
    PROFILES.iter().find(|p| p.id == id)
}

/// 查找失败时回退到 OpenAI 兼容（custom 等）。
pub fn resolve_or_openai_compat(provider_id: &str) -> &'static ProviderProfile {
    resolve(provider_id).unwrap_or(&OPENAI_COMPAT_FALLBACK)
}

/// custom / 未知 id 的回退 profile（不在 PROFILES 中单独注册）。
static OPENAI_COMPAT_FALLBACK: ProviderProfile = ProviderProfile {
    id: "openai",
    api_mode: ApiMode::ChatCompletions,
    default_base_url: "https://api.openai.com/v1",
    auth: AuthKind::Bearer,
    env_keys: &["OPENAI_API_KEY"],
    azure_deployment_style: false,
    default_model: "gpt-5.6",
    supports_image_gen: false,
    supports_embedding: false,
};

/// 将常见别名规范化为表内 id。
pub fn normalize_provider_id(provider_id: &str) -> &str {
    match provider_id {
        "minmax" => "minimax",
        "anthropic" => "claude",
        other => other,
    }
}

/// 表驱动默认基址（未知 id → OpenAI）。
pub fn default_base_for(provider: &str) -> &'static str {
    resolve(provider)
        .map(|p| p.default_base_url)
        .unwrap_or("https://api.openai.com/v1")
}

/// 表驱动环境变量名列表。
pub fn env_api_key_names(provider_id: &str) -> &'static [&'static str] {
    resolve(provider_id)
        .map(|p| p.env_keys)
        .unwrap_or(&["OPENAI_API_KEY"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_uses_openai_compat_chat() {
        let p = resolve("google").expect("google");
        assert_eq!(p.api_mode, ApiMode::ChatCompletions);
        assert!(p.default_base_url.contains("/v1beta/openai"));
        assert_eq!(p.auth, AuthKind::Bearer);
        assert!(!p.azure_deployment_style);
    }

    #[test]
    fn ollama_uses_v1_chat_completions() {
        let p = resolve("ollama").expect("ollama");
        assert_eq!(p.api_mode, ApiMode::ChatCompletions);
        assert_eq!(p.default_base_url, "http://localhost:11434/v1");
        assert_eq!(p.auth, AuthKind::None);
    }

    #[test]
    fn azure_is_chat_completions_with_quirk() {
        let p = resolve("azure").expect("azure");
        assert_eq!(p.api_mode, ApiMode::ChatCompletions);
        assert!(p.azure_deployment_style);
        assert_eq!(p.auth, AuthKind::AzureHeader);
    }

    #[test]
    fn anthropic_alias_and_messages_mode() {
        let p = resolve("anthropic").expect("claude via alias");
        assert_eq!(p.id, "claude");
        assert_eq!(p.api_mode, ApiMode::AnthropicMessages);
    }

    #[test]
    fn no_default_profile_uses_responses() {
        assert!(PROFILES.iter().all(|p| p.api_mode != ApiMode::Responses));
    }
}
