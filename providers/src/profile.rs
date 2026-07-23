//! Hermes 风格 `ProviderProfile` 表：id → ApiMode / 默认 base / 认证 / 媒体能力。

use crate::trait_::AuthKind;

/// 底层协议适配器种类（对齐 Hermes 三协议 + Gemini Interactions + Gemini Native）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiMode {
    /// OpenAI Chat Completions（含兼容网关、Ollama `/v1`）。
    ChatCompletions,
    /// Anthropic Messages API。
    AnthropicMessages,
    /// OpenAI Responses API（`POST /v1/responses`）。
    Responses,
    /// Google Gemini Interactions API（`POST /v1beta/interactions`）。
    Interactions,
    /// Google Gemini 原生 `streamGenerateContent`（支持 function_declarations 工具调用）。
    GeminiNative,
}

/// 图片生成协议路由。新厂商走 OpenAI 兼容 API 只需设 `OpenAi`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageGenMode {
    /// OpenAI `POST /images/generations`（OpenAI / Azure / 兼容网关）。
    OpenAi,
    /// Google Interactions `POST /v1beta/interactions` with generateImage。
    GoogleInteractions,
    /// MiniMax T2I `POST /v1/text/image`。
    MiniMax,
}

/// 单个供应商的静态配置。
///
/// 新增纯聊天厂商：加一行 PROFILES + 一行 vendor type alias。
/// 新增带媒体能力的厂商：额外设 `image_mode` / `default_*_model` 字段即可。
#[derive(Debug, Clone, Copy)]
pub struct ProviderProfile {
    pub id: &'static str,
    pub api_mode: ApiMode,
    pub default_base_url: &'static str,
    pub auth: AuthKind,
    pub env_keys: &'static [&'static str],
    pub azure_deployment_style: bool,
    pub default_model: &'static str,
    pub supports_image_gen: bool,
    pub supports_embedding: bool,

    // ── 媒体能力（表驱动，消除 provider-id 硬编码） ──

    /// 图片生成协议模式。`None` = 不支持。
    pub image_mode: Option<ImageGenMode>,
    /// 图片生成默认模型名（空 = 不支持或用 fallback）。
    pub default_image_model: &'static str,
    /// 视觉理解默认模型名（空 = 不支持独立视觉模型）。
    pub default_vision_model: &'static str,
    /// 是否支持 `stream_options.include_usage`（部分 OpenAI 兼容网关不支持）。
    pub supports_stream_usage: bool,
    /// TTS 默认模型名（空 = 不支持）。
    pub default_tts_model: &'static str,
    /// 视频生成默认模型名（空 = 不支持）。
    pub default_video_model: &'static str,
    /// 音乐生成默认模型名（空 = 不支持）。
    pub default_music_model: &'static str,
    /// ASR / 语音识别默认模型名（空 = 不支持）。
    pub default_asr_model: &'static str,
    /// 嵌入模型默认名（空 = 不支持或用通用 fallback）。
    pub default_embedding_model: &'static str,
}

impl ProviderProfile {
    pub fn supports_asr(&self) -> bool {
        !self.default_asr_model.is_empty()
    }
}

impl ProviderProfile {
    pub fn supports_tts(&self) -> bool {
        !self.default_tts_model.is_empty()
    }
    pub fn supports_video(&self) -> bool {
        !self.default_video_model.is_empty()
    }
    pub fn supports_music(&self) -> bool {
        !self.default_music_model.is_empty()
    }
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
        default_model: "gpt-5.6-sol",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::OpenAi),
        default_image_model: "gpt-image-2",
        default_vision_model: "gpt-4o",
        supports_stream_usage: true,
        default_tts_model: "openai-tts-v3",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "whisper-v3-turbo",
        default_embedding_model: "text-embedding-4-large",
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
        image_mode: None,
        default_image_model: "",
        default_vision_model: "",
        supports_stream_usage: false,
        default_tts_model: "",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "",
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
        image_mode: None,
        default_image_model: "",
        default_vision_model: "",
        supports_stream_usage: true,
        default_tts_model: "",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "",
    },
    ProviderProfile {
        id: "google",
        api_mode: ApiMode::Interactions,
        default_base_url: "https://generativelanguage.googleapis.com",
        auth: AuthKind::GoogleApiKey,
        env_keys: &["GOOGLE_API_KEY", "GEMINI_API_KEY", "GOOGLE_AI_API_KEY"],
        azure_deployment_style: false,
        default_model: "gemini-3.1-ultra",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::GoogleInteractions),
        default_image_model: "nanobanana-v2",
        default_vision_model: "gemini-3.1-ultra",
        supports_stream_usage: false,
        default_tts_model: "gemini-3.1-flash-tts",
        default_video_model: "veo-3.1",
        default_music_model: "lyria-3-pro",
        default_asr_model: "google-stt-v2-live",
        default_embedding_model: "gemini-embedding-v3",
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
        image_mode: None,
        default_image_model: "",
        default_vision_model: "",
        supports_stream_usage: true,
        default_tts_model: "",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "",
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
        image_mode: None,
        default_image_model: "",
        default_vision_model: "",
        supports_stream_usage: true,
        default_tts_model: "",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "",
    },
    ProviderProfile {
        id: "zhipu",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://open.bigmodel.cn/api/paas/v4",
        auth: AuthKind::Bearer,
        env_keys: &["ZHIPU_API_KEY", "BIGMODEL_API_KEY"],
        azure_deployment_style: false,
        default_model: "glm-5.2-plus",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::OpenAi),
        default_image_model: "cogview-4",
        default_vision_model: "glm-5.2-plus",
        supports_stream_usage: false,
        default_tts_model: "glm-tts-v1.2",
        default_video_model: "cogvideox-v1.5",
        default_music_model: "cogmusic-v1.1",
        default_asr_model: "glm-asr-v1.2",
        default_embedding_model: "cogembedding-v2",
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
        image_mode: None,
        default_image_model: "",
        default_vision_model: "",
        supports_stream_usage: true,
        default_tts_model: "",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "",
    },
    ProviderProfile {
        id: "bailian",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        auth: AuthKind::Bearer,
        env_keys: &["DASHSCOPE_API_KEY", "BAILIAN_API_KEY"],
        azure_deployment_style: false,
        default_model: "qwen3.8-max",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::OpenAi),
        default_image_model: "wanimage-2.7",
        default_vision_model: "qwen3.8-max",
        supports_stream_usage: false,
        default_tts_model: "qwen-tts-v2.5",
        default_video_model: "wan-2.7",
        default_music_model: "",
        default_asr_model: "qwen-asr-v2.5",
        default_embedding_model: "qwen-embedding-v3",
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
        image_mode: None,
        default_image_model: "",
        default_vision_model: "",
        supports_stream_usage: true,
        default_tts_model: "",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "",
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
        image_mode: None,
        default_image_model: "",
        default_vision_model: "",
        supports_stream_usage: true,
        default_tts_model: "",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "",
    },
    ProviderProfile {
        id: "volcengine",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://ark.cn-beijing.volces.com/api/v3",
        auth: AuthKind::Bearer,
        env_keys: &["ARK_API_KEY", "VOLCENGINE_API_KEY"],
        azure_deployment_style: false,
        default_model: "doubao-seed-2.1-pro",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::OpenAi),
        default_image_model: "seedream-5.0-pro",
        default_vision_model: "doubao-seed-2.1-pro",
        supports_stream_usage: false,
        default_tts_model: "seed-tts-2.1",
        default_video_model: "seedance-2.5",
        default_music_model: "bytedance-music-v2.1",
        default_asr_model: "seed-asr-2.1",
        default_embedding_model: "doubao-embedding-vision-v2",
    },
    ProviderProfile {
        id: "minimax",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://api.minimaxi.com/v1",
        auth: AuthKind::Bearer,
        env_keys: &["MINIMAX_API_KEY", "MINMAX_API_KEY"],
        azure_deployment_style: false,
        default_model: "MiniMax-M3",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::MiniMax),
        default_image_model: "image-01",
        default_vision_model: "MiniMax-M3",
        supports_stream_usage: true,
        default_tts_model: "speech-2.8-hd",
        default_video_model: "MiniMax-Hailuo-2.3",
        default_music_model: "music-3.0",
        default_asr_model: "speech-asr-v2.2",
        default_embedding_model: "minimax-embedding-v3",
    },
    ProviderProfile {
        id: "hunyuan",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://api.hunyuan.cloud.tencent.com/v1",
        auth: AuthKind::Bearer,
        env_keys: &["HUNYUAN_API_KEY", "TENCENT_API_KEY"],
        azure_deployment_style: false,
        default_model: "hunyuan-hy3",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::OpenAi),
        default_image_model: "hunyuan-image-v2.1",
        default_vision_model: "hunyuan-hy3",
        supports_stream_usage: true,
        default_tts_model: "hunyuan-tts-v2.1",
        default_video_model: "hunyuan-video-v1.5",
        default_music_model: "hunyuan-music-v2",
        default_asr_model: "hunyuan-asr-v2.1",
        default_embedding_model: "hunyuan-embedding-v3",
    },
    ProviderProfile {
        id: "minimax-anthropic",
        api_mode: ApiMode::AnthropicMessages,
        default_base_url: "https://api.minimaxi.com/anthropic",
        auth: AuthKind::Bearer,
        env_keys: &["MINIMAX_API_KEY", "MINMAX_API_KEY"],
        azure_deployment_style: false,
        default_model: "MiniMax-M2.5",
        supports_image_gen: false,
        supports_embedding: false,
        image_mode: None,
        default_image_model: "",
        default_vision_model: "",
        supports_stream_usage: false,
        default_tts_model: "",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "",
    },
    ProviderProfile {
        id: "openai-responses",
        api_mode: ApiMode::Responses,
        default_base_url: "https://api.openai.com/v1",
        auth: AuthKind::Bearer,
        env_keys: &["OPENAI_API_KEY"],
        azure_deployment_style: false,
        default_model: "gpt-5.6-sol",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::OpenAi),
        default_image_model: "gpt-image-2",
        default_vision_model: "gpt-4o",
        supports_stream_usage: false,
        default_tts_model: "openai-tts-v3",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "whisper-v3-turbo",
        default_embedding_model: "text-embedding-4-large",
    },
    ProviderProfile {
        id: "minimax-responses",
        api_mode: ApiMode::Responses,
        default_base_url: "https://api.minimaxi.com/v1",
        auth: AuthKind::Bearer,
        env_keys: &["MINIMAX_API_KEY", "MINMAX_API_KEY"],
        azure_deployment_style: false,
        default_model: "MiniMax-M3",
        supports_image_gen: false,
        supports_embedding: false,
        image_mode: None,
        default_image_model: "",
        default_vision_model: "",
        supports_stream_usage: false,
        default_tts_model: "",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "",
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
        image_mode: None,
        default_image_model: "",
        default_vision_model: "",
        supports_stream_usage: true,
        default_tts_model: "",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "",
    },
    ProviderProfile {
        id: "gemini-native",
        api_mode: ApiMode::GeminiNative,
        default_base_url: "https://generativelanguage.googleapis.com",
        auth: AuthKind::GoogleApiKey,
        env_keys: &["GOOGLE_API_KEY", "GEMINI_API_KEY", "GOOGLE_AI_API_KEY"],
        azure_deployment_style: false,
        default_model: "gemini-3.5-flash",
        supports_image_gen: false,
        supports_embedding: false,
        image_mode: None,
        default_image_model: "",
        default_vision_model: "",
        supports_stream_usage: false,
        default_tts_model: "",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "",
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
    image_mode: None,
    default_image_model: "",
    default_vision_model: "",
    supports_stream_usage: false,
    default_tts_model: "",
    default_video_model: "",
    default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "",
};

/// 将常见别名规范化为表内 id。
pub fn normalize_provider_id(provider_id: &str) -> &str {
    match provider_id {
        "minmax" => "minimax",
        "minmax-anthropic" => "minimax-anthropic",
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
    fn google_uses_interactions_api() {
        let p = resolve("google").expect("google");
        assert_eq!(p.api_mode, ApiMode::Interactions);
        assert_eq!(
            p.default_base_url,
            "https://generativelanguage.googleapis.com"
        );
        assert!(!p.default_base_url.contains("/v1beta/openai"));
        assert_eq!(p.auth, AuthKind::GoogleApiKey);
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
    fn minimax_responses_profile_exists() {
        let p = resolve("minimax-responses").expect("minimax-responses profile");
        assert_eq!(p.api_mode, ApiMode::Responses);
    }

    #[test]
    fn media_capabilities_table_driven() {
        let openai = resolve("openai").unwrap();
        assert_eq!(openai.image_mode, Some(ImageGenMode::OpenAi));
        assert!(openai.supports_stream_usage);
        assert!(openai.supports_tts());
        assert!(!openai.supports_video());

        let google = resolve("google").unwrap();
        assert_eq!(google.image_mode, Some(ImageGenMode::GoogleInteractions));
        assert!(google.supports_tts());
        assert!(google.supports_video());
        assert!(google.supports_music());

        let minimax = resolve("minimax").unwrap();
        assert_eq!(minimax.image_mode, Some(ImageGenMode::MiniMax));
        assert!(minimax.supports_tts());
        assert!(minimax.supports_video());
        assert!(minimax.supports_music());

        let deepseek = resolve("deepseek").unwrap();
        assert_eq!(deepseek.image_mode, None);
        assert!(deepseek.supports_stream_usage);
        assert!(!deepseek.supports_tts());

        let zhipu = resolve("zhipu").unwrap();
        assert!(!zhipu.supports_stream_usage);
    }
}
