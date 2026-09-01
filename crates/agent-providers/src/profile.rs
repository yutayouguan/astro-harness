//! Hermes 风格 `ProviderProfile` 表：id → ApiMode / 默认 base / 认证 / 媒体能力。

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
    /// OpenAI `POST /images/generations`（OpenAI / 兼容网关）。
    OpenAi,
    /// Azure AI Foundry OpenAI v1 `POST /openai/v1/images/generations`。
    AzureOpenAiV1,
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
    /// 默认模型名（**离线 fallback**；运行时优先从缓存选最新模型）。
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
    /// 是否支持 Responses API 协议切换。
    pub supports_responses: bool,
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

/// 提供商认证方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    Bearer,
    AnthropicKey,
    GoogleApiKey,
    AzureHeader,
    None,
}

impl AuthKind {
    pub fn for_provider(provider_id: &str) -> Self {
        resolve(provider_id).map(|p| p.auth).unwrap_or(Self::Bearer)
    }
}

/// 内置 profile 表（含全部 registry id）。
pub static PROFILES: &[ProviderProfile] = &[
    ProviderProfile {
        id: "openai",
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
        supports_stream_usage: true,
        default_tts_model: "openai-tts-v3",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "whisper-v3-turbo",
        default_embedding_model: "text-embedding-4-large",
        supports_responses: true,
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
        supports_responses: false,
    },
    ProviderProfile {
        id: "deepseek",
        api_mode: ApiMode::Responses,
        default_base_url: "https://api.deepseek.com/v1",
        auth: AuthKind::Bearer,
        env_keys: &["DEEPSEEK_API_KEY"],
        azure_deployment_style: false,
        default_model: "deepseek-v4-flash",
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
        supports_responses: true,
    },
    ProviderProfile {
        id: "google",
        api_mode: ApiMode::Interactions,
        default_base_url: "https://generativelanguage.googleapis.com",
        auth: AuthKind::GoogleApiKey,
        env_keys: &["GOOGLE_API_KEY", "GEMINI_API_KEY", "GOOGLE_AI_API_KEY"],
        azure_deployment_style: false,
        default_model: "gemini-3.6-flash",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::GoogleInteractions),
        default_image_model: "gemini-3.6-flash",
        default_vision_model: "gemini-3.6-flash",
        supports_stream_usage: false,
        default_tts_model: "gemini-3.1-flash-tts",
        default_video_model: "veo-3.1",
        default_music_model: "lyria-3-pro",
        default_asr_model: "google-stt-v2-live",
        default_embedding_model: "gemini-embedding-v3",
        supports_responses: false,
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
        supports_responses: false,
    },
    ProviderProfile {
        id: "azure",
        api_mode: ApiMode::Responses,
        default_base_url: "https://YOUR_RESOURCE.services.ai.azure.com/openai/v1",
        auth: AuthKind::Bearer,
        env_keys: &["AZURE_OPENAI_API_KEY", "AZURE_API_KEY"],
        azure_deployment_style: false,
        default_model: "gpt-5.6-sol",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::AzureOpenAiV1),
        default_image_model: "gpt-image-2",
        default_vision_model: "",
        supports_stream_usage: true,
        default_tts_model: "",
        default_video_model: "",
        default_music_model: "",
        default_asr_model: "",
        default_embedding_model: "text-embedding-3-small",
        supports_responses: true,
    },
    ProviderProfile {
        id: "zhipu",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://open.bigmodel.cn/api/paas/v4",
        auth: AuthKind::Bearer,
        env_keys: &["ZHIPU_API_KEY", "BIGMODEL_API_KEY"],
        azure_deployment_style: false,
        default_model: "glm-5.2",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::OpenAi),
        default_image_model: "cogview-4",
        default_vision_model: "glm-5.2",
        supports_stream_usage: false,
        default_tts_model: "glm-tts-v1.2",
        default_video_model: "cogvideox-v1.5",
        default_music_model: "cogmusic-v1.1",
        default_asr_model: "glm-asr-v1.2",
        default_embedding_model: "cogembedding-v2",
        supports_responses: false,
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
        supports_responses: false,
    },
    ProviderProfile {
        id: "bailian",
        api_mode: ApiMode::Responses,
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
        supports_responses: true,
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
        supports_responses: false,
    },
    ProviderProfile {
        id: "moonshot",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://api.moonshot.cn/v1",
        auth: AuthKind::Bearer,
        env_keys: &["MOONSHOT_API_KEY", "KIMI_API_KEY"],
        azure_deployment_style: false,
        default_model: "kimi-k3",
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
        supports_responses: false,
    },
    ProviderProfile {
        id: "volcengine",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://ark.cn-beijing.volces.com/api/v3",
        auth: AuthKind::Bearer,
        env_keys: &["ARK_API_KEY", "VOLCENGINE_API_KEY"],
        azure_deployment_style: false,
        default_model: "doubao-seed-evolving",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::OpenAi),
        default_image_model: "seedream-5.0-pro",
        default_vision_model: "doubao-seed-evolving",
        supports_stream_usage: false,
        default_tts_model: "seed-tts-2.1",
        default_video_model: "seedance-2.5",
        default_music_model: "bytedance-music-v2.1",
        default_asr_model: "seed-asr-2.1",
        default_embedding_model: "doubao-embedding-vision-v2",
        supports_responses: false,
    },
    ProviderProfile {
        id: "minimax",
        api_mode: ApiMode::Responses,
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
        default_video_model: "MiniMax-H3",
        default_music_model: "music-3.0",
        default_asr_model: "speech-asr-v2.2",
        default_embedding_model: "minimax-embedding-v3",
        supports_responses: true,
    },
    ProviderProfile {
        id: "hunyuan",
        api_mode: ApiMode::ChatCompletions,
        default_base_url: "https://api.hunyuan.cloud.tencent.com/v1",
        auth: AuthKind::Bearer,
        env_keys: &["HUNYUAN_API_KEY", "TENCENT_API_KEY"],
        azure_deployment_style: false,
        default_model: "hy3",
        supports_image_gen: true,
        supports_embedding: true,
        image_mode: Some(ImageGenMode::OpenAi),
        default_image_model: "hunyuan-image-v2.1",
        default_vision_model: "hy3",
        supports_stream_usage: true,
        default_tts_model: "hunyuan-tts-v2.1",
        default_video_model: "hunyuan-video-v1.5",
        default_music_model: "hunyuan-music-v2",
        default_asr_model: "hunyuan-asr-v2.1",
        default_embedding_model: "hunyuan-embedding-v3",
        supports_responses: false,
    },
    ProviderProfile {
        id: "minimax-anthropic",
        api_mode: ApiMode::AnthropicMessages,
        default_base_url: "https://api.minimaxi.com/anthropic",
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
        supports_responses: false,
    },
    ProviderProfile {
        id: "mimo",
        api_mode: ApiMode::Responses,
        default_base_url: "https://api.xiaomimimo.com/v1",
        auth: AuthKind::Bearer,
        env_keys: &["MIMO_API_KEY"],
        azure_deployment_style: false,
        default_model: "mimo-v2.5",
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
        supports_responses: true,
    },
    ProviderProfile {
        id: "gemini-native",
        api_mode: ApiMode::GeminiNative,
        default_base_url: "https://generativelanguage.googleapis.com",
        auth: AuthKind::GoogleApiKey,
        env_keys: &["GOOGLE_API_KEY", "GEMINI_API_KEY", "GOOGLE_AI_API_KEY"],
        azure_deployment_style: false,
        default_model: "gemini-3.6-flash",
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
        supports_responses: false,
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

/// 解析最终补全协议：显式配置优先，否则使用 provider profile 默认值。
pub fn effective_api_mode(provider_id: &str, api_mode: &str) -> ApiMode {
    match api_mode.trim() {
        "chat" | "chat_completions" => ApiMode::ChatCompletions,
        "responses" => ApiMode::Responses,
        _ => resolve_or_openai_compat(provider_id).api_mode,
    }
}

/// custom / 未知 id 的回退 profile（不在 PROFILES 中单独注册）。
static OPENAI_COMPAT_FALLBACK: ProviderProfile = ProviderProfile {
    id: "openai",
    api_mode: ApiMode::ChatCompletions,
    default_base_url: "https://api.openai.com/v1",
    auth: AuthKind::Bearer,
    env_keys: &["OPENAI_API_KEY"],
    azure_deployment_style: false,
    default_model: "gpt-5.6-sol",
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
    supports_responses: false,
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

/// 从环境变量读取第一个非空的 API Key。
pub fn read_env_api_key(provider_id: &str) -> Option<String> {
    for name in env_api_key_names(provider_id) {
        if let Ok(value) = std::env::var(name) {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

// ─── 动态模型默认值（OpenRouter 驱动）────────────────────

mod model_defaults {
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use std::time::{Duration, SystemTime};

    use serde::{Deserialize, Serialize};

    const CACHE_MAX_AGE: Duration = Duration::from_secs(72 * 3600);

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct ModelDefaultsCache {
        fetched_at_epoch: u64,
        defaults: HashMap<String, String>,
    }

    static CACHE: OnceLock<Option<HashMap<String, String>>> = OnceLock::new();

    /// 缓存路径。调用方可通过 `set_cache_dir` 覆盖。
    /// 默认 `~/.astro/model-defaults.json`。
    fn cache_path() -> PathBuf {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let dir = PathBuf::from(home).join(".astro");
        dir.join("model-defaults.json")
    }

    fn now_epoch() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn load_cache() -> Option<HashMap<String, String>> {
        let content = fs::read_to_string(cache_path()).ok()?;
        let cache: ModelDefaultsCache = serde_json::from_str(&content).ok()?;
        let age = now_epoch().saturating_sub(cache.fetched_at_epoch);
        if age > CACHE_MAX_AGE.as_secs() {
            return None;
        }
        Some(cache.defaults)
    }

    fn get_cache() -> &'static Option<HashMap<String, String>> {
        CACHE.get_or_init(load_cache)
    }

    pub fn resolve_default_model(provider_id: &str) -> Option<&'static str> {
        let cache = get_cache().as_ref()?;
        cache.get(provider_id).map(|s| s.as_str())
    }

    /// 厂商前缀映射：`(profile_id, openrouter_prefix)`。
    const VENDOR_MAP: &[(&str, &str)] = &[
        ("google", "google/gemini"),
        ("openai", "openai/gpt"),
        ("claude", "anthropic/claude"),
        ("deepseek", "deepseek/"),
        ("minimax", "minimax/"),
    ];

    const SKIP_SUFFIXES: &[&str] = &[
        "-preview",
        "-free",
        "-extended",
        ":free",
        ":extended",
        "-online",
        "-nitro",
        "-floor",
        "-exp",
    ];

    pub fn refresh_from_openrouter(api_key: &str) -> Result<(), String> {
        let url = "https://openrouter.ai/api/v1/models";
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .get(url)
            .header("Authorization", format!("Bearer {api_key}"))
            .send()
            .map_err(|e| format!("获取 OpenRouter 模型列表失败: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("OpenRouter HTTP {}", resp.status()));
        }
        let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let data = json
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or("OpenRouter 响应缺少 data 数组")?;

        let mut defaults = HashMap::new();
        for &(provider_id, prefix) in VENDOR_MAP {
            let mut best: Option<(&str, i64)> = None;
            for item in data {
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if !id.starts_with(prefix) {
                    continue;
                }
                if SKIP_SUFFIXES.iter().any(|s| id.ends_with(s)) {
                    continue;
                }
                let created = item.get("created").and_then(|v| v.as_i64()).unwrap_or(0);
                if best.is_none_or(|(_, c)| created > c) {
                    best = Some((id, created));
                }
            }
            if let Some((model_id, _)) = best {
                let short = model_id.split_once('/').map(|(_, m)| m).unwrap_or(model_id);
                defaults.insert(provider_id.to_string(), short.to_string());
            }
        }

        if defaults.is_empty() {
            return Err("OpenRouter 未返回可用模型".to_string());
        }

        let cache = ModelDefaultsCache {
            fetched_at_epoch: now_epoch(),
            defaults,
        };
        let path = cache_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(&cache).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| format!("写入 model-defaults.json 失败: {e}"))?;

        Ok(())
    }
}

/// 获取 provider 的默认聊天模型 — 优先从 OpenRouter 缓存读取，回退到 profile 静态值。
pub fn default_chat_model(provider_id: &str) -> &str {
    if let Some(m) = model_defaults::resolve_default_model(provider_id) {
        return m;
    }
    resolve(provider_id)
        .map(|p| p.default_model)
        .unwrap_or("gpt-4o")
}

/// 从 OpenRouter 刷新各厂商最新默认模型并写入 `~/.astro/model-defaults.json`。
///
/// 需要 OpenRouter API Key。建议应用启动后在后台线程调用。
pub fn refresh_model_defaults(openrouter_api_key: &str) -> Result<(), String> {
    model_defaults::refresh_from_openrouter(openrouter_api_key)
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
    fn azure_defaults_to_openai_v1_responses() {
        let p = resolve("azure").expect("azure");
        assert_eq!(p.api_mode, ApiMode::Responses);
        assert!(!p.azure_deployment_style);
        assert_eq!(p.auth, AuthKind::Bearer);
        assert_eq!(
            p.default_base_url,
            "https://YOUR_RESOURCE.services.ai.azure.com/openai/v1"
        );
        assert_eq!(p.default_model, "gpt-5.6-sol");
        assert!(p.supports_image_gen);
        assert!(p.supports_embedding);
        assert_eq!(p.image_mode, Some(ImageGenMode::AzureOpenAiV1));
        assert_eq!(p.default_image_model, "gpt-image-2");
        assert_eq!(p.default_embedding_model, "text-embedding-3-small");
    }

    #[test]
    fn anthropic_alias_and_messages_mode() {
        let p = resolve("anthropic").expect("claude via alias");
        assert_eq!(p.id, "claude");
        assert_eq!(p.api_mode, ApiMode::AnthropicMessages);
    }

    #[test]
    fn supports_responses_flag() {
        for id in ["openai", "deepseek", "minimax", "azure", "bailian", "mimo"] {
            let profile = resolve(id).unwrap();
            assert!(profile.supports_responses, "{id} should support Responses");
            assert_eq!(
                profile.api_mode,
                ApiMode::Responses,
                "{id} should default to Responses"
            );
        }
        assert!(!resolve("claude").unwrap().supports_responses);
        assert!(!resolve("google").unwrap().supports_responses);
    }

    #[test]
    fn effective_mode_uses_profile_default_and_honors_compatibility_override() {
        assert_eq!(effective_api_mode("deepseek", ""), ApiMode::Responses);
        assert_eq!(
            effective_api_mode("deepseek", "chat_completions"),
            ApiMode::ChatCompletions
        );
        assert_eq!(
            effective_api_mode("ollama", "responses"),
            ApiMode::Responses
        );
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
