//! 模型供应商配置、校验与 LiteLLM/元数据相关 Tauri 命令。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(test)]
use std::fs;
use std::sync::Mutex;

use crate::infra::keystore::{
    delete_api_key, has_api_key, keyring_service_for_provider, load_api_key, save_api_key,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Openai,
    Anthropic,
    Deepseek,
    Ollama,
    Google,
    Azure,
    Zhipu,
    Openrouter,
    Bailian,
    Nvidia,
    Moonshot,
    Volcengine,
    Minimax,
    Hunyuan,
    Custom,
}

impl ProviderKind {
    /// 从字符串解析枚举变体。
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "openai" => Some(Self::Openai),
            "anthropic" => Some(Self::Anthropic),
            "deepseek" => Some(Self::Deepseek),
            "ollama" => Some(Self::Ollama),
            "google" => Some(Self::Google),
            "azure" => Some(Self::Azure),
            "zhipu" => Some(Self::Zhipu),
            "openrouter" => Some(Self::Openrouter),
            "bailian" => Some(Self::Bailian),
            "nvidia" => Some(Self::Nvidia),
            "moonshot" => Some(Self::Moonshot),
            "volcengine" => Some(Self::Volcengine),
            "minimax" => Some(Self::Minimax),
            "hunyuan" | "tencent" => Some(Self::Hunyuan),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    /// 序列化为稳定字符串 id。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
            Self::Deepseek => "deepseek",
            Self::Ollama => "ollama",
            Self::Google => "google",
            Self::Azure => "azure",
            Self::Zhipu => "zhipu",
            Self::Openrouter => "openrouter",
            Self::Bailian => "bailian",
            Self::Nvidia => "nvidia",
            Self::Moonshot => "moonshot",
            Self::Volcengine => "volcengine",
            Self::Minimax => "minimax",
            Self::Hunyuan => "hunyuan",
            Self::Custom => "custom",
        }
    }

    /// 面向 UI 的显示名。
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Openai => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Deepseek => "DeepSeek",
            Self::Ollama => "Ollama",
            Self::Google => "Google",
            Self::Azure => "Azure OpenAI",
            Self::Zhipu => "智谱开放平台",
            Self::Openrouter => "OpenRouter",
            Self::Bailian => "阿里百炼",
            Self::Nvidia => "NVIDIA",
            Self::Moonshot => "月之暗面",
            Self::Volcengine => "火山引擎",
            Self::Minimax => "MiniMax",
            Self::Hunyuan => "腾讯混元",
            Self::Custom => "Custom",
        }
    }

    /// 映射到 providers crate 的后端 id。
    pub fn backend_id(&self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Anthropic => "claude",
            Self::Deepseek => "deepseek",
            Self::Ollama => "ollama",
            Self::Google => "google",
            Self::Azure => "azure",
            Self::Zhipu => "zhipu",
            Self::Openrouter => "openrouter",
            Self::Bailian => "bailian",
            Self::Nvidia => "nvidia",
            Self::Moonshot => "moonshot",
            Self::Volcengine => "volcengine",
            Self::Minimax => "minimax",
            Self::Hunyuan => "hunyuan",
            Self::Custom => "openai",
        }
    }

    /// 该供应商的默认 API 基址。
    pub fn default_endpoint(&self) -> &'static str {
        match self {
            Self::Openai => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com",
            Self::Deepseek => "https://api.deepseek.com/v1",
            Self::Ollama => "http://localhost:11434/v1",
            Self::Google => "https://generativelanguage.googleapis.com",
            Self::Azure => "https://YOUR_RESOURCE.services.ai.azure.com/openai/v1",
            Self::Zhipu => "https://open.bigmodel.cn/api/paas/v4",
            Self::Openrouter => "https://openrouter.ai/api/v1",
            Self::Bailian => "https://dashscope.aliyuncs.com/compatible-mode/v1",
            Self::Nvidia => "https://integrate.api.nvidia.com/v1",
            Self::Moonshot => "https://api.moonshot.cn/v1",
            Self::Volcengine => "https://ark.cn-beijing.volces.com/api/v3",
            Self::Minimax => "https://api.minimaxi.com/v1",
            Self::Hunyuan => "https://api.hunyuan.cloud.tencent.com/v1",
            Self::Custom => "http://localhost:11434/v1",
        }
    }

    /// 该供应商的默认模型 id。
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Openai => "gpt-5.6-sol",
            Self::Anthropic => "claude-opus-4-8",
            Self::Deepseek => "deepseek-chat",
            Self::Ollama => "llama3.3",
            Self::Google => "gemini-3.1-ultra",
            Self::Azure => "gpt-5.6-sol",
            Self::Zhipu => "glm-5.2-plus",
            Self::Openrouter => "openai/gpt-5.6",
            Self::Bailian => "qwen3.8-max",
            Self::Nvidia => "meta/llama-3.3-70b-instruct",
            Self::Moonshot => "kimi-k2.5",
            Self::Volcengine => "doubao-seed-2.1-pro",
            Self::Minimax => "MiniMax-M3",
            Self::Hunyuan => "hunyuan-hy3",
            Self::Custom => "custom-model",
        }
    }

    /// 是否必须配置 API Key。
    pub fn requires_api_key(&self) -> bool {
        !matches!(self, Self::Ollama)
    }

    /// 官方获取 API Key 的控制台链接。
    pub fn official_key_url(&self) -> Option<&'static str> {
        match self {
            Self::Openai => Some("https://platform.openai.com/api-keys"),
            Self::Anthropic => Some("https://console.anthropic.com/settings/keys"),
            Self::Deepseek => Some("https://platform.deepseek.com/api_keys"),
            Self::Ollama => None,
            Self::Google => Some("https://aistudio.google.com/apikey"),
            Self::Azure => Some(
                "https://portal.azure.com/#view/Microsoft_Azure_ProjectOxford/CognitiveServicesHub/~/OpenAI",
            ),
            Self::Zhipu => Some("https://open.bigmodel.cn/usercenter/proj-mgmt/apikeys"),
            Self::Openrouter => Some("https://openrouter.ai/keys"),
            Self::Bailian => Some("https://bailian.console.aliyun.com/"),
            Self::Nvidia => Some("https://build.nvidia.com/settings/api-keys"),
            Self::Moonshot => Some("https://platform.moonshot.cn/console/api-keys"),
            Self::Volcengine => {
                Some("https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey")
            }
            Self::Minimax => Some(
                "https://platform.minimaxi.com/user-center/basic-information/interface-key",
            ),
            Self::Hunyuan => Some("https://console.cloud.tencent.com/hunyuan/api-key"),
            Self::Custom => None,
        }
    }

    /// 按优先级读取该提供商常见的环境变量名。
    pub fn env_api_key_names(&self) -> &'static [&'static str] {
        match self {
            Self::Openai => &["OPENAI_API_KEY"],
            Self::Anthropic => &["ANTHROPIC_API_KEY", "CLAUDE_API_KEY"],
            Self::Deepseek => &["DEEPSEEK_API_KEY"],
            Self::Ollama => &[],
            Self::Google => &["GOOGLE_API_KEY", "GEMINI_API_KEY", "GOOGLE_AI_API_KEY"],
            Self::Azure => &["AZURE_OPENAI_API_KEY", "AZURE_API_KEY"],
            Self::Zhipu => &["ZHIPU_API_KEY", "BIGMODEL_API_KEY"],
            Self::Openrouter => &["OPENROUTER_API_KEY"],
            Self::Bailian => &["DASHSCOPE_API_KEY", "BAILIAN_API_KEY"],
            Self::Nvidia => &["NVIDIA_API_KEY"],
            Self::Moonshot => &["MOONSHOT_API_KEY", "KIMI_API_KEY"],
            Self::Volcengine => &["ARK_API_KEY", "VOLCENGINE_API_KEY"],
            Self::Minimax => &["MINIMAX_API_KEY"],
            Self::Hunyuan => &["HUNYUAN_API_KEY", "TENCENT_API_KEY"],
            Self::Custom => &["CUSTOM_API_KEY", "OPENAI_API_KEY"],
        }
    }

    /// 从环境变量读取该供应商 API Key。
    pub fn read_env_api_key(&self) -> Option<(String, String)> {
        for name in self.env_api_key_names() {
            if let Ok(value) = std::env::var(name) {
                let trimmed = value.trim().to_string();
                if !trimmed.is_empty() {
                    return Some((name.to_string(), trimmed));
                }
            }
        }
        None
    }
}

/// 单条聊天后备引用（写入 `config.toml [desktop.providers]`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProviderFallbackEntry {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Explicit user configuration; None is only for one-time legacy classification.
    #[serde(default)]
    pub added: Option<bool>,
    pub id: String,
    pub kind: ProviderKind,
    pub display_name: String,
    pub endpoint: String,
    pub model: String,
    pub enabled: bool,
    /// 显式聊天后备链（最多 3；老配置无此字段时默认空）。
    #[serde(default)]
    pub fallback: Vec<ProviderFallbackEntry>,
    /// 生图模型（空=内置默认）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub image_model: String,
    /// 生视频模型（空=内置默认；主要 Google）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub video_model: String,
    /// 生音频 / TTS 模型（空=内置默认）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tts_model: String,
    /// 视觉（图片理解）模型（空=内置默认）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub vision_model: String,
    /// 音乐生成模型（空=内置默认；主要 Google）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub music_model: String,
    /// 文本嵌入模型（空=内置默认）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub embedding_model: String,
}

impl ProviderConfig {
    fn is_added(&self) -> bool {
        self.added == Some(true) || self.id.starts_with("toml:")
    }

    /// 创建默认实例。
    pub fn new(kind: ProviderKind) -> Self {
        Self {
            display_name: kind.display_name().to_string(),
            endpoint: kind.default_endpoint().to_string(),
            model: resolve_latest_chat_model(&kind),
            enabled: true,
            id: format!("prov-{}", uuid::Uuid::new_v4().simple()),
            kind,
            fallback: Vec::new(),
            image_model: String::new(),
            video_model: String::new(),
            tts_model: String::new(),
            vision_model: String::new(),
            music_model: String::new(),
            embedding_model: String::new(),
            added: Some(true),
        }
    }

    /// 创建默认禁用的 Provider 条目。
    #[cfg(test)]
    pub fn new_disabled(kind: ProviderKind) -> Self {
        let mut p = Self::new(kind);
        p.enabled = false;
        p
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvidersState {
    pub providers: Vec<ProviderConfig>,
    pub active_provider_id: Option<String>,
    #[serde(default)]
    pub active_image_provider_id: Option<String>,
}

impl ProvidersState {
    fn add_provider(&mut self, kind: ProviderKind) {
        let mut provider = if let Some(index) = self
            .providers
            .iter()
            .position(|p| p.kind == kind && !p.is_added())
        {
            // Promote, rather than replace, a catalog row: preserve its ID and stored fields.
            self.providers.remove(index)
        } else {
            let mut provider = ProviderConfig::new(kind);
            provider.enabled = !matches!(
                kind,
                ProviderKind::Openrouter
                    | ProviderKind::Bailian
                    | ProviderKind::Nvidia
                    | ProviderKind::Moonshot
                    | ProviderKind::Volcengine
                    | ProviderKind::Minimax
                    | ProviderKind::Hunyuan
            );
            provider
        };
        provider.added = Some(true);
        self.providers.push(provider);
    }

    fn reorder_added(&mut self, ids: Vec<String>) -> Result<(), String> {
        if ids.len()
            != self
                .providers
                .iter()
                .filter(|provider| provider.is_added())
                .count()
        {
            return Err("排序列表长度不匹配".to_string());
        }
        let (added, templates): (Vec<_>, Vec<_>) = self
            .providers
            .iter()
            .cloned()
            .partition(ProviderConfig::is_added);
        let mut by_id: std::collections::HashMap<String, ProviderConfig> = added
            .into_iter()
            .map(|provider| (provider.id.clone(), provider))
            .collect();
        let mut next = Vec::with_capacity(ids.len());
        for id in ids {
            let p = by_id
                .remove(&id)
                .ok_or_else(|| format!("提供商不存在: {id}"))?;
            next.push(p);
        }
        if !by_id.is_empty() {
            return Err("排序列表缺少部分提供商".to_string());
        }
        next.extend(templates);
        self.providers = next;
        Ok(())
    }

    fn remove_provider(&mut self, id: &str) -> Result<(), String> {
        let matches: Vec<_> = self
            .providers
            .iter()
            .enumerate()
            .filter(|(_, p)| p.id == id)
            .map(|(i, _)| i)
            .collect();
        if matches.is_empty() {
            return Err("提供商不存在".to_string());
        }
        // 若历史数据存在重复 id，一次删干净，避免「删了还在」
        for idx in matches.into_iter().rev() {
            self.providers.remove(idx);
        }
        if self.active_provider_id.as_deref() == Some(id)
            || self
                .active_provider_id
                .as_ref()
                .is_some_and(|aid| !self.providers.iter().any(|p| &p.id == aid))
        {
            self.active_provider_id = self
                .providers
                .iter()
                .find(|p| {
                    p.is_added()
                        && p.enabled
                        && providers::dispatch::supports_agent_responses(&toml_backend_id(p))
                })
                .map(|p| p.id.clone());
        }
        if self.active_image_provider_id.as_deref() == Some(id) {
            self.active_image_provider_id = None;
        }
        self.ensure_builtin_kinds();
        Ok(())
    }

    /// Built-in rows are a catalog, not user-added configuration.
    pub fn with_defaults() -> Self {
        let mut state = Self::default();
        state.ensure_builtin_kinds();
        state
    }

    pub fn ensure_builtin_kinds(&mut self) -> bool {
        let mut changed = false;
        for kind in [
            ProviderKind::Anthropic,
            ProviderKind::Openai,
            ProviderKind::Google,
            ProviderKind::Deepseek,
            ProviderKind::Azure,
            ProviderKind::Zhipu,
            ProviderKind::Ollama,
            ProviderKind::Openrouter,
            ProviderKind::Bailian,
            ProviderKind::Nvidia,
            ProviderKind::Moonshot,
            ProviderKind::Volcengine,
            ProviderKind::Minimax,
            ProviderKind::Hunyuan,
        ] {
            if !self.providers.iter().any(|provider| provider.kind == kind) {
                let mut provider = ProviderConfig::new(kind);
                provider.enabled = matches!(
                    kind,
                    ProviderKind::Anthropic
                        | ProviderKind::Openai
                        | ProviderKind::Google
                        | ProviderKind::Deepseek
                        | ProviderKind::Azure
                        | ProviderKind::Zhipu
                        | ProviderKind::Ollama
                );
                provider.added = Some(false);
                self.providers.push(provider);
                changed = true;
            }
        }
        changed
    }

    /// Keep legacy configured accounts and explicit custom rows without deleting any data.
    fn migrate_added_flags(&mut self, has_credentials: impl Fn(&ProviderConfig) -> bool) -> bool {
        let mut changed = false;
        let mut seen_kinds = std::collections::HashSet::new();
        for provider in &mut self.providers {
            let duplicate_kind = !seen_kinds.insert(provider.kind.as_str());
            if provider.added.is_some() {
                continue;
            }
            let customized = provider.kind == ProviderKind::Custom
                || provider.id.starts_with("toml:")
                || duplicate_kind
                || (self.active_provider_id.as_deref() == Some(provider.id.as_str())
                    && provider.kind != ProviderKind::Anthropic)
                || self.active_image_provider_id.as_deref() == Some(provider.id.as_str())
                || provider.display_name != provider.kind.display_name()
                || provider.endpoint.trim_end_matches('/')
                    != provider.kind.default_endpoint().trim_end_matches('/')
                || (!provider.model.trim().is_empty()
                    && provider.model != provider.kind.default_model()
                    && provider.model != resolve_latest_chat_model(&provider.kind))
                || !provider.fallback.is_empty()
                || [
                    &provider.image_model,
                    &provider.video_model,
                    &provider.tts_model,
                    &provider.vision_model,
                    &provider.music_model,
                    &provider.embedding_model,
                ]
                .iter()
                .any(|model| !model.is_empty());
            provider.added =
                Some(customized || (provider.kind.requires_api_key() && has_credentials(provider)));
            changed = true;
        }
        changed
    }

    /// 把已知过期/不安全的默认值迁到现行默认（仅当用户仍停留在旧默认时）。
    pub fn migrate_stale_defaults(&mut self) -> bool {
        let mut changed = false;
        for p in &mut self.providers {
            match p.kind {
                ProviderKind::Moonshot if p.model == "kimi-k2-0711-preview" => {
                    p.model = "kimi-k2.5".to_string();
                    changed = true;
                }
                ProviderKind::Volcengine if p.model == "doubao-pro-32k" => {
                    p.model = ProviderKind::Volcengine.default_model().to_string();
                    changed = true;
                }
                ProviderKind::Minimax => {
                    let ep = p.endpoint.trim_end_matches('/');
                    if ep == "https://api.minimax.chat/v1" {
                        p.endpoint = "https://api.minimaxi.com/v1".to_string();
                        changed = true;
                    }
                    if p.model == "MiniMax-M2.5" {
                        p.model = ProviderKind::Minimax.default_model().to_string();
                        changed = true;
                    }
                }
                ProviderKind::Google => {
                    // 旧默认走 OpenAI 兼容；聊天已迁 Interactions，回写原生 host。
                    let ep = p.endpoint.trim_end_matches('/');
                    if ep.contains("/v1beta/openai")
                        || (ep.ends_with("/openai")
                            && ep.contains("generativelanguage.googleapis.com"))
                    {
                        p.endpoint = ProviderKind::Google.default_endpoint().to_string();
                        changed = true;
                    }
                    // 仍停在旧时代默认模型名时升级到现行默认（不改 2.5 / 用户自定义）。
                    match p.model.as_str() {
                        "gemini-1.5-flash"
                        | "gemini-1.5-pro"
                        | "gemini-2.0-flash"
                        | "gemini-2.0-flash-001" => {
                            p.model = ProviderKind::Google.default_model().to_string();
                            changed = true;
                        }
                        _ => {}
                    }
                }
                ProviderKind::Azure => {
                    let ep = p.endpoint.trim_end_matches('/');
                    if ep == "https://YOUR_RESOURCE.openai.azure.com" {
                        p.endpoint = ProviderKind::Azure.default_endpoint().to_string();
                        changed = true;
                    }
                    if p.model == "gpt-5.6" {
                        p.model = ProviderKind::Azure.default_model().to_string();
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        changed
    }

    /// 修复历史数据中因纳秒 ID 碰撞产生的重复 id，避免删除/选中错乱。
    pub fn ensure_unique_ids(&mut self) -> bool {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        let mut changed = false;
        let old_active = self.active_provider_id.clone();
        for p in &mut self.providers {
            if !seen.insert(p.id.clone()) {
                let old_id = p.id.clone();
                p.id = format!("prov-{}", uuid::Uuid::new_v4().simple());
                if old_active.as_deref() == Some(&old_id) {
                    // 保留第一个同名 id 作为 active；后续碰撞项换新 id
                }
                changed = true;
            }
        }
        // active 指向已不存在的 id 时回退
        if let Some(active) = &self.active_provider_id {
            if !self.providers.iter().any(|p| &p.id == active) {
                self.active_provider_id = self.providers.first().map(|p| p.id.clone());
                changed = true;
            }
        }
        if self
            .active_image_provider_id
            .as_ref()
            .is_some_and(|active| {
                !self.providers.iter().any(|provider| {
                    &provider.id == active
                        && provider.enabled
                        && providers::dispatch::supports_image_gen(&toml_backend_id(provider))
                })
            })
        {
            self.active_image_provider_id = None;
            changed = true;
        }
        changed
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderConfigDto {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub endpoint: String,
    pub model: String,
    pub enabled: bool,
    pub has_api_key: bool,
    /// keyring | env | none | not_required
    pub key_source: String,
    /// 命中的环境变量名（若有）
    pub env_key_name: Option<String>,
    pub backend_id: String,
    /// 官方获取 API Key 的链接（若有）
    pub official_key_url: Option<String>,
    /// 聊天后备链（显式配置）
    pub fallback: Vec<ProviderFallbackEntry>,
    pub image_model: String,
    pub video_model: String,
    pub tts_model: String,
    pub vision_model: String,
    pub music_model: String,
    pub asr_model: String,
    pub embedding_model: String,
    pub supports_image: bool,
    pub supports_video: bool,
    pub supports_tts: bool,
    pub supports_music: bool,
    pub supports_asr: bool,
    pub supports_embedding: bool,
    /// 是否可用于 Agent Responses API。
    pub supports_responses_api: bool,
    /// 配置来源：`"builtin"` / `"toml"` / `"user"`。
    pub config_source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProvidersStateDto {
    pub providers: Vec<ProviderConfigDto>,
    pub provider_templates: Vec<ProviderConfigDto>,
    pub active_provider_id: Option<String>,
    pub active_image_provider_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfigInput {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub endpoint: String,
    pub model: String,
    pub enabled: bool,
    #[serde(default)]
    pub fallback: Vec<ProviderFallbackEntry>,
    #[serde(default)]
    pub image_model: String,
    #[serde(default)]
    pub video_model: String,
    #[serde(default)]
    pub tts_model: String,
    #[serde(default)]
    pub vision_model: String,
    #[serde(default)]
    pub music_model: String,
    #[serde(default)]
    pub embedding_model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderModelsResult {
    pub models: Vec<crate::meta::model_meta::ModelInfo>,
    pub latency_ms: u64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub model: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ModelsCacheFile {
    #[serde(default)]
    providers: HashMap<String, CachedProviderModels>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedProviderModels {
    #[serde(default)]
    configuration_fingerprint: String,
    provider_id: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    models: Vec<crate::meta::model_meta::ModelEntryCompat>,
    #[serde(default)]
    source: String,
    #[serde(default)]
    latency_ms: u64,
    updated_at: String,
}

/// 从磁盘加载模型列表缓存。
fn load_models_cache() -> ModelsCacheFile {
    let base = home::default_memory_dir();
    match home::cache::read::<ModelsCacheFile>(
        &base,
        home::cache::Domain::Models,
        &"provider-catalog",
    ) {
        Ok(Some(mut cache)) => {
            let ttl = home::cache::policy(&base, home::cache::Domain::Models)
                .map(|p| p.ttl_seconds)
                .unwrap_or(0);
            cache.providers.retain(|_, entry| {
                chrono::DateTime::parse_from_rfc3339(&entry.updated_at).is_ok_and(|time| {
                    let age = chrono::Utc::now().signed_duration_since(time).num_seconds();
                    age >= 0 && (age as u64) <= ttl
                })
            });
            cache
        }
        Ok(None) => ModelsCacheFile::default(),
        Err(error) => {
            tracing::warn!(%error, "model cache unavailable");
            ModelsCacheFile::default()
        }
    }
}

/// 串行化 models.json 的读改写，避免并发 list_provider_models 互相覆盖 / 撞临时文件。
static MODELS_CACHE_LOCK: Mutex<()> = Mutex::new(());

/// 将模型列表缓存原子写入磁盘（调用方须已持有 `MODELS_CACHE_LOCK`）。
fn save_models_cache(cache: &ModelsCacheFile) -> Result<(), String> {
    home::cache::write(
        &home::default_memory_dir(),
        home::cache::Domain::Models,
        &"provider-catalog",
        cache,
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

/// 持久化某一 Provider 的模型列表。
fn persist_provider_models(
    provider: &ProviderConfig,
    models: &[crate::meta::model_meta::ModelInfo],
    source: &str,
    latency_ms: u64,
) -> Result<(), String> {
    let _guard = MODELS_CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut cache = load_models_cache();
    cache.providers.insert(
        provider.id.clone(),
        CachedProviderModels {
            configuration_fingerprint: provider_cache_identity(provider).unwrap_or_default(),
            provider_id: provider.id.clone(),
            display_name: provider.display_name.clone(),
            kind: provider.kind.as_str().to_string(),
            models: models
                .iter()
                .cloned()
                .map(|info| crate::meta::model_meta::ModelEntryCompat::Full(Box::new(info)))
                .collect(),
            source: source.to_string(),
            latency_ms,
            updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        },
    );
    save_models_cache(&cache)
}

/// 将 TOML 自定义 provider 声明的模型注入 models.json 缓存。
fn sync_custom_provider_models() {
    let config_path = home::config_path(&home::default_memory_dir());
    let custom = providers::custom::load_custom_providers(&config_path);
    if custom.is_empty() {
        return;
    }
    let _guard = MODELS_CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut cache = load_models_cache();
    for (id, cfg) in &custom {
        if cfg.models.is_empty() {
            continue;
        }
        let models: Vec<crate::meta::model_meta::ModelEntryCompat> = cfg
            .models
            .iter()
            .map(|m| {
                let reasoning = if m.reasoning {
                    Some(crate::meta::model_meta::ModelReasoningMeta {
                        supported_efforts: m.supported_efforts.clone(),
                        default_effort: m.default_effort.clone(),
                        default_enabled: Some(true),
                        mandatory: Some(false),
                        supports_max_tokens: None,
                        persistent_instructions: None,
                    })
                } else {
                    None
                };
                crate::meta::model_meta::ModelEntryCompat::Full(Box::new(
                    crate::meta::model_meta::ModelInfo {
                        id: m.id.clone(),
                        profile: types::ModelProfile::default(),
                        tool_mode: m.tool_mode,
                        display_name: m.display_name.clone(),
                        description: None,
                        canonical_slug: None,
                        knowledge_cutoff: None,
                        expiration_date: None,
                        created: None,
                        hugging_face_id: None,
                        is_moderated: None,
                        context_window: m.context_window,
                        max_output_tokens: m.max_output_tokens,
                        capabilities: crate::meta::model_meta::ModelCapabilities {
                            tools: m.tools.unwrap_or(true),
                            vision: m.vision.unwrap_or(false),
                            reasoning: m.reasoning,
                            ..Default::default()
                        },
                        reasoning,
                        pricing: None,
                        default_parameters: None,
                        meta_source: "toml".to_string(),
                    },
                ))
            })
            .collect();
        cache.providers.insert(
            format!("toml:{id}"),
            CachedProviderModels {
                configuration_fingerprint: configured_provider_for_cache(&format!("toml:{id}"))
                    .and_then(|p| provider_cache_identity(&p))
                    .unwrap_or_default(),
                provider_id: format!("toml:{id}"),
                display_name: cfg.name.clone(),
                kind: "custom".to_string(),
                models,
                source: "config.toml".to_string(),
                latency_ms: 0,
                updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            },
        );
    }
    if let Err(e) = save_models_cache(&cache) {
        tracing::warn!(error = %e, "sync_custom_provider_models 写入失败");
    }
}

/// 加载全部 Provider 配置状态。
fn load_state() -> Result<ProvidersState, String> {
    let state = home::settings::read::<ProvidersState>(
        &home::default_memory_dir(),
        &["desktop", "providers"],
    )
    .map_err(|error| error.to_string())?;
    let Some(mut state) = state else {
        let mut state = ProvidersState::with_defaults();
        merge_toml_custom_providers(&mut state);
        return Ok(state);
    };
    let mut changed = state.ensure_unique_ids();
    if state.ensure_builtin_kinds() {
        changed = true;
    }
    if state.migrate_stale_defaults() {
        changed = true;
    }
    if state.migrate_added_flags(|provider| resolve_api_key(provider).0) {
        changed = true;
    }
    if changed {
        save_state(&state)?;
    }
    sync_custom_provider_models();
    merge_toml_custom_providers(&mut state);
    Ok(state)
}

/// 将 TOML 自定义 provider 合并进 ProvidersState（UI 可见）。
///
/// 以 `toml:<id>` 作为 provider ID，避免与 UI 手动添加的 `prov-*` ID 碰撞。
/// 已存在同 ID 的条目时跳过（用户可能在 UI 中修改过）。
fn merge_toml_custom_providers(state: &mut ProvidersState) {
    let config_path = home::config_path(&home::default_memory_dir());
    let custom = providers::custom::load_custom_providers(&config_path);
    for (id, cfg) in &custom {
        let toml_id = format!("toml:{id}");
        if state.providers.iter().any(|p| p.id == toml_id) {
            continue;
        }
        let api_key_available = providers::custom::read_env_key(&cfg.env_keys).is_some();
        state.providers.push(ProviderConfig {
            id: toml_id,
            kind: ProviderKind::Custom,
            display_name: if cfg.name.is_empty() {
                id.to_string()
            } else {
                cfg.name.clone()
            },
            endpoint: cfg.base_url.clone(),
            model: cfg.default_model.clone(),
            enabled: api_key_available,
            fallback: Vec::new(),
            image_model: String::new(),
            video_model: String::new(),
            tts_model: String::new(),
            vision_model: String::new(),
            music_model: String::new(),
            embedding_model: String::new(),
            added: Some(true),
        });
    }
}

/// 保存 Provider 配置状态。
fn save_state(state: &ProvidersState) -> Result<(), String> {
    home::settings::write(
        &home::default_memory_dir(),
        &["desktop", "providers"],
        state,
    )
    .map_err(|error| error.to_string())
}

/// `resolve_api_key`。
pub(crate) fn resolve_api_key(
    p: &ProviderConfig,
) -> (bool, String, Option<String>, Option<String>) {
    if !p.kind.requires_api_key() {
        return (true, "not_required".into(), None, None);
    }
    let service = keyring_service_for_provider(&p.id);
    if let Some(key) = load_api_key(&service) {
        return (true, "keyring".into(), None, Some(key));
    }
    if let Some((env_name, key)) = p.kind.read_env_api_key() {
        return (true, "env".into(), Some(env_name), Some(key));
    }
    (false, "none".into(), None, None)
}

/// 支持 Responses API 切换的厂商（表驱动）。
fn supports_responses_toggle(kind: ProviderKind) -> bool {
    providers::profile::resolve(kind.backend_id())
        .is_some_and(providers::profile::ProviderProfile::supports_agent_responses)
}

/// TOML 自定义 provider 用 TOML key 作为 backend_id（dispatch 需要此 ID 命中 custom provider）。
fn toml_backend_id(p: &ProviderConfig) -> String {
    if let Some(toml_key) = p.id.strip_prefix("toml:") {
        toml_key.to_string()
    } else {
        p.kind.backend_id().to_string()
    }
}

/// 单条 Provider → 前端 DTO。
fn to_dto(p: &ProviderConfig) -> ProviderConfigDto {
    let (has_api_key, key_source, env_key_name, _) = resolve_api_key(p);
    let bid = toml_backend_id(p);
    let profile = providers::profile::resolve_or_openai_compat(&bid);
    ProviderConfigDto {
        id: p.id.clone(),
        kind: p.kind.as_str().to_string(),
        display_name: p.display_name.clone(),
        endpoint: p.endpoint.clone(),
        model: p.model.clone(),
        enabled: p.enabled,
        has_api_key,
        key_source,
        env_key_name,
        backend_id: bid.to_string(),
        official_key_url: p.kind.official_key_url().map(str::to_string),
        fallback: p.fallback.clone(),
        image_model: if p.image_model.is_empty()
            && !providers::image_gen::supports_scene_image_model(&bid)
        {
            profile.default_image_model.to_string()
        } else {
            p.image_model.clone()
        },
        video_model: if p.video_model.is_empty() {
            profile.default_video_model.to_string()
        } else {
            p.video_model.clone()
        },
        tts_model: if p.tts_model.is_empty() {
            profile.default_tts_model.to_string()
        } else {
            p.tts_model.clone()
        },
        vision_model: if p.vision_model.is_empty() {
            profile.default_vision_model.to_string()
        } else {
            p.vision_model.clone()
        },
        music_model: if p.music_model.is_empty() {
            profile.default_music_model.to_string()
        } else {
            p.music_model.clone()
        },
        asr_model: profile.default_asr_model.to_string(),
        embedding_model: if p.embedding_model.is_empty() {
            profile.default_embedding_model.to_string()
        } else {
            p.embedding_model.clone()
        },
        supports_image: profile.supports_image_gen(),
        supports_video: profile.supports_video(),
        supports_tts: profile.supports_tts(),
        supports_music: profile.supports_music(),
        supports_asr: profile.supports_asr(),
        supports_embedding: profile.supports_embedding(),
        supports_responses_api: supports_responses_toggle(p.kind) || p.id.starts_with("toml:"),
        config_source: if p.id.starts_with("toml:") {
            "toml".to_string()
        } else if p.is_added() {
            "user".to_string()
        } else {
            "builtin".to_string()
        },
    }
}

/// 整体状态 → 前端 DTO。
fn to_state_dto(state: &ProvidersState) -> ProvidersStateDto {
    ProvidersStateDto {
        providers: state
            .providers
            .iter()
            .filter(|provider| provider.is_added())
            .map(to_dto)
            .collect(),
        provider_templates: state
            .providers
            .iter()
            .filter(|provider| !provider.is_added())
            .map(to_dto)
            .collect(),
        active_provider_id: state.active_provider_id.clone().filter(|id| {
            state
                .providers
                .iter()
                .any(|provider| provider.id == *id && provider.is_added())
        }),
        active_image_provider_id: state.active_image_provider_id.clone().filter(|id| {
            state
                .providers
                .iter()
                .any(|provider| provider.id == *id && provider.is_added())
        }),
    }
}

static STATE: Mutex<Option<ProvidersState>> = Mutex::new(None);

/// 可变借用状态并执行闭包后写回。
fn with_state_mut<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce(&mut ProvidersState) -> Result<T, String>,
{
    let mut guard = STATE.lock().map_err(|_| "状态锁失败".to_string())?;
    if guard.is_none() {
        *guard = Some(load_state()?);
    }
    let mut next = guard.as_ref().expect("provider state initialized").clone();
    let result = f(&mut next)?;
    save_state(&next)?;
    *guard = Some(next);
    Ok(result)
}

/// 只读借用状态执行闭包。
fn with_state<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce(&ProvidersState) -> Result<T, String>,
{
    let mut guard = STATE.lock().map_err(|_| "状态锁失败".to_string())?;
    if guard.is_none() {
        *guard = Some(load_state()?);
    }
    let state = guard.as_ref().unwrap();
    f(state)
}

/// Tauri 命令：get_providers_state。
#[tauri::command]
pub fn get_providers_state() -> Result<ProvidersStateDto, String> {
    with_state_mut(|s| {
        let _ = s.ensure_unique_ids();
        s.ensure_builtin_kinds();
        Ok(to_state_dto(s))
    })
}

/// 列出供应商配置状态。
#[tauri::command]
pub fn list_providers() -> Result<Vec<ProviderConfigDto>, String> {
    with_state(|s| {
        let list: Vec<_> = s
            .providers
            .iter()
            .filter(|p| p.enabled && p.is_added())
            .map(to_dto)
            .collect();
        Ok(list)
    })
}

/// Tauri 命令：add_provider。
#[tauri::command]
pub fn add_provider(kind: String) -> Result<ProvidersStateDto, String> {
    let kind = ProviderKind::from_str(&kind).ok_or_else(|| format!("未知提供商类型: {kind}"))?;
    with_state_mut(|s| {
        s.add_provider(kind);
        Ok(to_state_dto(s))
    })
}

/// 保存单个供应商配置。
#[tauri::command]
pub fn save_provider(provider: ProviderConfigInput) -> Result<ProvidersStateDto, String> {
    let kind = ProviderKind::from_str(&provider.kind)
        .ok_or_else(|| format!("未知提供商类型: {}", provider.kind))?;
    validate_http_endpoint(&provider.endpoint)?;
    with_state_mut(|s| {
        let idx = s
            .providers
            .iter()
            .position(|p| p.id == provider.id)
            .ok_or_else(|| "提供商不存在".to_string())?;
        let fallback = provider
            .fallback
            .into_iter()
            .take(types::MAX_MODEL_FALLBACKS)
            .filter(|e| !e.provider_id.trim().is_empty())
            .collect();
        s.providers[idx] = ProviderConfig {
            id: provider.id,
            kind,
            display_name: provider.display_name,
            endpoint: provider.endpoint,
            model: provider.model,
            enabled: provider.enabled,
            fallback,
            image_model: provider.image_model.trim().to_string(),
            video_model: provider.video_model.trim().to_string(),
            tts_model: provider.tts_model.trim().to_string(),
            vision_model: provider.vision_model.trim().to_string(),
            music_model: provider.music_model.trim().to_string(),
            embedding_model: provider.embedding_model.trim().to_string(),
            added: Some(true),
        };
        if s.active_image_provider_id.as_deref() == Some(s.providers[idx].id.as_str())
            && (!s.providers[idx].enabled
                || !providers::dispatch::supports_image_gen(&toml_backend_id(&s.providers[idx])))
        {
            s.active_image_provider_id = None;
        }
        Ok(to_state_dto(s))
    })
}

/// Tauri 命令：reorder_providers。
#[tauri::command]
pub fn reorder_providers(ids: Vec<String>) -> Result<ProvidersStateDto, String> {
    with_state_mut(|s| {
        s.reorder_added(ids)?;
        Ok(to_state_dto(s))
    })
}

/// Tauri 命令：delete_provider。
#[tauri::command]
pub fn delete_provider(id: String) -> Result<ProvidersStateDto, String> {
    with_state_mut(|s| {
        s.remove_provider(&id)?;
        let _ = delete_api_key(&keyring_service_for_provider(&id));
        Ok(to_state_dto(s))
    })
}

/// Tauri 命令：set_active_provider。
#[tauri::command]
pub fn set_active_provider(id: String) -> Result<ProvidersStateDto, String> {
    with_state_mut(|s| {
        let provider = s
            .providers
            .iter_mut()
            .find(|p| p.id == id && p.enabled)
            .ok_or_else(|| "提供商不存在或未启用".to_string())?;
        if !providers::dispatch::supports_agent_responses(&toml_backend_id(provider)) {
            return Err("该提供商不支持 Agent Responses API".to_string());
        }
        provider.added = Some(true);
        s.active_provider_id = Some(id);
        Ok(to_state_dto(s))
    })
}

/// 原子更新聊天模型并设为当前 Provider，避免前端用不完整配置覆盖高级字段。
#[tauri::command]
pub fn set_active_provider_model(id: String, model: String) -> Result<ProvidersStateDto, String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("模型 ID 不能为空".to_string());
    }
    with_state_mut(|state| set_active_provider_model_in_state(state, &id, model))
}

fn set_active_provider_model_in_state(
    state: &mut ProvidersState,
    id: &str,
    model: &str,
) -> Result<ProvidersStateDto, String> {
    let provider = state
        .providers
        .iter_mut()
        .find(|provider| provider.id == id && provider.enabled)
        .ok_or_else(|| "提供商不存在或未启用".to_string())?;
    if !providers::dispatch::supports_agent_responses(&toml_backend_id(provider)) {
        return Err("该提供商不支持 Agent Responses API".to_string());
    }
    provider.model = model.to_string();
    provider.added = Some(true);
    state.active_provider_id = Some(id.to_string());
    Ok(to_state_dto(state))
}

/// 设置独立的默认图片生成 Provider。
#[tauri::command]
pub fn set_active_image_provider(id: String) -> Result<ProvidersStateDto, String> {
    with_state_mut(|state| {
        let provider = state
            .providers
            .iter_mut()
            .find(|provider| provider.id == id && provider.enabled)
            .ok_or_else(|| "提供商不存在或未启用".to_string())?;
        if !providers::dispatch::supports_image_gen(&toml_backend_id(provider)) {
            return Err("该提供商不支持图片生成".to_string());
        }
        provider.added = Some(true);
        state.active_image_provider_id = Some(id);
        Ok(to_state_dto(state))
    })
}

/// Tauri 命令：get_provider_api_key。
#[tauri::command]
pub fn get_provider_api_key(id: String) -> Result<Option<String>, String> {
    with_state(|s| {
        let p = s
            .providers
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| "提供商不存在".to_string())?;
        let (has, _, _, _) = resolve_api_key(p);
        // 不向渲染进程下发明文 Key；前端用 has_api_key + 重新输入改 Key
        Ok(if has {
            Some("__configured__".to_string())
        } else {
            None
        })
    })
}

/// Tauri 命令：set_provider_api_key。
#[tauri::command]
pub fn set_provider_api_key(id: String, api_key: String) -> Result<ProvidersStateDto, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("API Key 不能为空".to_string());
    }
    with_state_mut(|s| {
        if !s.providers.iter().any(|p| p.id == id) {
            return Err("提供商不存在".to_string());
        }
        save_api_key(&keyring_service_for_provider(&id), key).map_err(|e| e.to_string())?;
        if let Some(provider) = s.providers.iter_mut().find(|provider| provider.id == id) {
            provider.added = Some(true);
        }
        Ok(to_state_dto(s))
    })
}

/// Tauri 命令：clear_provider_api_key。
#[tauri::command]
pub fn clear_provider_api_key(id: String) -> Result<ProvidersStateDto, String> {
    with_state_mut(|s| {
        if !s.providers.iter().any(|p| p.id == id) {
            return Err("提供商不存在".to_string());
        }
        let service = keyring_service_for_provider(&id);
        if has_api_key(&service) {
            delete_api_key(&service).map_err(|e| e.to_string())?;
        }
        Ok(to_state_dto(s))
    })
}

/// `find_provider`。
pub(crate) fn find_provider(id: &str) -> Result<ProviderConfig, String> {
    with_state(|s| {
        s.providers
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| "提供商不存在".to_string())
    })
}

/// `find_provider_by_backend`。
pub(crate) fn find_provider_by_backend(backend_id: &str) -> Result<ProviderConfig, String> {
    with_state(|s| {
        s.providers
            .iter()
            .find(|p| p.is_added() && p.enabled && toml_backend_id(p) == backend_id)
            .cloned()
            .or_else(|| {
                s.providers
                    .iter()
                    .find(|p| p.is_added() && toml_backend_id(p) == backend_id)
                    .cloned()
            })
            .ok_or_else(|| format!("未找到 backend_id={backend_id} 的提供商"))
    })
}

/// 从 config.toml 的 desktop.providers + keyring 展开主目标与聊天后备链（含 primary）。
///
/// - primary：与 `resolve_chat_credentials` 相同的查找规则；`model` 空则用条目默认模型
/// - fallback：跳过禁用、无 Key（ollama 除外）、缺失条目；去重与上限由 `expand_model_targets` 负责
pub fn resolve_model_targets(
    primary_provider_id: Option<&str>,
    backend_hint: &str,
    model: &str,
) -> Result<Vec<types::ModelTarget>, String> {
    let cfg: ProviderConfig = if let Some(id) = primary_provider_id.filter(|s| !s.is_empty()) {
        find_provider(id)?
    } else {
        find_provider_by_backend(backend_hint)?
    };
    let primary_backend = toml_backend_id(&cfg);
    if !providers::dispatch::supports_agent_responses(&primary_backend) {
        return Err(format!("{} 不支持 Agent Responses API", cfg.display_name));
    }

    let (has, _source, _env, key) = resolve_api_key(&cfg);
    if cfg.kind.requires_api_key() && !has {
        return Err(format!(
            "未配置 API Key。请在「模型提供商」中为 {} 保存密钥。",
            cfg.display_name
        ));
    }

    let model = {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            cfg.model.clone()
        } else {
            trimmed.to_string()
        }
    };

    let primary = types::ModelTarget {
        provider_id: cfg.id.clone(),
        backend_id: primary_backend,
        model,
        api_key: key.unwrap_or_default(),
        base_url: cfg.endpoint.clone(),
    };

    let refs: Vec<types::FallbackRef> = cfg
        .fallback
        .iter()
        .map(|e| types::FallbackRef {
            provider_id: e.provider_id.clone(),
            model: e.model.clone(),
        })
        .collect();

    let chain = types::expand_model_targets(&primary, &refs, |id| {
        let Ok(p) = find_provider(id) else {
            return None;
        };
        if !p.enabled {
            return None;
        }
        let (_has, _source, _env, key) = resolve_api_key(&p);
        let api_key = key.unwrap_or_default();
        let bid = toml_backend_id(&p);
        if !providers::dispatch::supports_agent_responses(&bid) {
            return None;
        }
        let allow_empty_key = bid == "ollama";
        if api_key.trim().is_empty() && !allow_empty_key {
            return None;
        }
        Some(types::ModelTarget {
            provider_id: p.id.clone(),
            backend_id: bid,
            model: p.model.clone(),
            api_key,
            base_url: p.endpoint.clone(),
        })
    });

    Ok(chain)
}

/// 图片生成目标（已开启 + 有 API Key 的 Google / OpenAI / Azure / MiniMax）
#[derive(Debug, Clone, Serialize)]
pub struct ImageGenTarget {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub display_name: String,
    pub video_model: String,
    pub tts_model: String,
    pub vision_model: String,
    pub music_model: String,
}

const IMAGE_GEN_NO_PROVIDER_MSG: &str =
    "未找到可用的图片生成提供商。请在「模型提供商」中开启 Google、OpenAI、Azure 或 MiniMax，并配置 API Key。";

/// 按供应商类型选择默认图片模型（动态优先，硬编码 fallback）。
fn default_image_model_for_kind(kind: &ProviderKind) -> Option<&'static str> {
    // 硬编码 fallback — 仅在缓存为空时使用
    match kind {
        ProviderKind::Google => Some("nano-banana-pro-preview"),
        ProviderKind::Openai | ProviderKind::Azure => {
            Some(providers::image_gen::IMAGE_MODEL_GENERAL)
        }
        ProviderKind::Minimax => Some("image-01"),
        _ => None,
    }
}

fn default_video_model_for_kind(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Google => "veo-3.1",
        ProviderKind::Minimax => "MiniMax-H3",
        _ => "",
    }
}

fn default_music_model_for_kind(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Google => "lyria-3-pro",
        ProviderKind::Minimax => "music-3.0",
        _ => "",
    }
}

fn default_tts_model_for_kind(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Google => "gemini-3.1-flash-tts",
        ProviderKind::Openai => "openai-tts-v3",
        ProviderKind::Minimax => "speech-2.8-hd",
        _ => "",
    }
}

fn default_vision_model_for_kind(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Google => "gemini-3.1-ultra",
        ProviderKind::Openai => "gpt-4o",
        ProviderKind::Minimax => "MiniMax-M3",
        _ => "",
    }
}

fn resolve_media_model(configured: &str, default: &str) -> String {
    let t = configured.trim();
    if t.is_empty() {
        default.to_string()
    } else {
        t.to_string()
    }
}

/// Read definitions without load_state's model-cache synchronization side effects.
fn configured_provider_for_cache(id: &str) -> Option<ProviderConfig> {
    let mut state: ProvidersState =
        home::settings::read(&home::default_memory_dir(), &["desktop", "providers"])
            .ok()
            .flatten()
            .unwrap_or_default();
    merge_toml_custom_providers(&mut state);
    state
        .providers
        .into_iter()
        .find(|provider| provider.id == id)
}

fn provider_cache_identity(provider: &ProviderConfig) -> Option<String> {
    let (_, _, _, credential) = resolve_api_key(provider);
    let custom: Option<serde_json::Value> = if let Some(id) = provider.id.strip_prefix("toml:") {
        home::settings::read(&home::default_memory_dir(), &["custom_providers", id]).ok()?
    } else {
        None
    };
    home::cache::fingerprint(&(provider, credential, custom)).ok()
}

fn cache_entry_is_current(entry: &CachedProviderModels) -> bool {
    configured_provider_for_cache(&entry.provider_id)
        .and_then(|p| provider_cache_identity(&p))
        .is_some_and(|identity| identity == entry.configuration_fingerprint)
}

/// 从模型缓存中选取该 provider 最新的聊天模型 id。
/// 缓存为空时回退到 `ProviderKind::default_model()` 硬编码值。
pub fn resolve_latest_chat_model(kind: &ProviderKind) -> String {
    let kind_str = kind.as_str();
    let cache = load_models_cache();
    // 遍历缓存中该 kind 的所有 provider entry
    let mut best: Option<(u64, String)> = None;
    for entry in cache.providers.values() {
        if !cache_entry_is_current(entry) {
            continue;
        }
        if entry.kind != kind_str {
            continue;
        }
        for compat in &entry.models {
            let info = match compat {
                crate::meta::model_meta::ModelEntryCompat::Full(boxed) => boxed.as_ref(),
                _ => continue,
            };
            // 过滤非聊天模型
            let id_lower = info.id.to_ascii_lowercase();
            if id_lower.contains("embed")
                || id_lower.contains("tts")
                || id_lower.contains("whisper")
                || id_lower.contains("veo")
                || id_lower.contains("lyria")
                || id_lower.contains("imagen")
                || id_lower.contains("dall-e")
                || id_lower.contains("sora")
                || id_lower.contains("robotics")
            {
                continue;
            }
            // 纯媒体模型跳过
            if info.capabilities.image_gen && !info.capabilities.tools && !info.capabilities.vision
            {
                continue;
            }
            let created = info.created.unwrap_or(0);
            if best.as_ref().is_none_or(|(c, _)| created > *c) {
                best = Some((created, info.id.clone()));
            }
        }
    }
    best.map(|(_, id)| id)
        .unwrap_or_else(|| kind.default_model().to_string())
}

/// 从 providers 面板解析图片生成候选：独立生图默认优先，未配置时回退聊天默认。
pub fn resolve_image_gen_targets() -> Result<Vec<ImageGenTarget>, String> {
    with_state(|s| {
        let preferred_id = s
            .active_image_provider_id
            .as_deref()
            .or(s.active_provider_id.as_deref());
        let mut active: Option<ImageGenTarget> = None;
        let mut google: Option<ImageGenTarget> = None;
        let mut openai: Option<ImageGenTarget> = None;
        let mut azure: Option<ImageGenTarget> = None;
        let mut minimax: Option<ImageGenTarget> = None;

        for p in &s.providers {
            if !p.is_added() || !p.enabled {
                continue;
            }
            let Some(default_image) = default_image_model_for_kind(&p.kind) else {
                continue;
            };
            let (has, _source, _env, key) = resolve_api_key(p);
            if !has {
                continue;
            }
            let Some(api_key) = key.filter(|k| !k.trim().is_empty()) else {
                continue;
            };
            let target = ImageGenTarget {
                provider: p.kind.backend_id().to_string(),
                model: if providers::image_gen::supports_scene_image_model(p.kind.backend_id()) {
                    p.image_model.trim().to_owned()
                } else {
                    resolve_media_model(&p.image_model, default_image)
                },
                api_key,
                base_url: p.endpoint.clone(),
                display_name: p.display_name.clone(),
                video_model: resolve_media_model(
                    &p.video_model,
                    default_video_model_for_kind(&p.kind),
                ),
                tts_model: resolve_media_model(&p.tts_model, default_tts_model_for_kind(&p.kind)),
                vision_model: resolve_media_model(
                    &p.vision_model,
                    default_vision_model_for_kind(&p.kind),
                ),
                music_model: resolve_media_model(
                    &p.music_model,
                    default_music_model_for_kind(&p.kind),
                ),
            };
            if preferred_id == Some(p.id.as_str()) {
                active = Some(target);
                continue;
            }
            match p.kind {
                ProviderKind::Google if google.is_none() => google = Some(target),
                ProviderKind::Openai if openai.is_none() => openai = Some(target),
                ProviderKind::Azure if azure.is_none() => azure = Some(target),
                ProviderKind::Minimax if minimax.is_none() => minimax = Some(target),
                _ => {}
            }
        }

        let mut targets = Vec::new();
        if let Some(a) = active {
            targets.push(a);
        }
        if let Some(g) = google {
            targets.push(g);
        }
        if let Some(o) = openai {
            targets.push(o);
        }
        if let Some(a) = azure {
            targets.push(a);
        }
        if let Some(m) = minimax {
            targets.push(m);
        }
        if targets.is_empty() {
            return Err(IMAGE_GEN_NO_PROVIDER_MSG.to_string());
        }
        Ok(targets)
    })
}

/// 构造用于探测/拉模型的 reqwest 客户端。
fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

/// 去掉 URL 末尾斜杠。
fn trim_slash(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

/// 从各厂商 API 错误响应中提取人类可读消息。
fn extract_api_error(body: &serde_json::Value) -> &str {
    body.pointer("/error/message")
        .and_then(|m| m.as_str())
        .or_else(|| {
            body.pointer("/base_resp/status_msg")
                .and_then(|m| m.as_str())
        })
        .or_else(|| body.get("error").and_then(|e| e.as_str()))
        .or_else(|| body.get("message").and_then(|m| m.as_str()))
        .filter(|s| !s.is_empty())
        .unwrap_or("未知错误")
}

/// 仅允许 http(s) Endpoint，拒绝 file:// 等危险 scheme（防 SSRF），
/// 并校验 URL 语法和主机名存在性。
fn validate_http_endpoint(endpoint: &str) -> Result<(), String> {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() {
        return Err("Endpoint 不能为空".to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("Endpoint 仅支持 http:// 或 https://".to_string());
    }
    let parsed = url::Url::parse(trimmed).map_err(|e| format!("Endpoint 格式无效: {e}"))?;
    if parsed.host_str().is_none_or(|h| h.is_empty()) {
        return Err("Endpoint 缺少主机名".to_string());
    }
    Ok(())
}

/// 规范化 OpenAI 兼容 API 基址（非 Google；Google 已迁 Interactions）。
fn openai_compatible_base(endpoint: &str) -> String {
    let base = trim_slash(endpoint);
    if base.ends_with("/v1")
        || base.ends_with("/v3")
        || base.ends_with("/v4")
        || base.ends_with("/openai")
        || base.contains("/paas/v4")
        || base.contains("/v1beta/openai")
    {
        base
    } else {
        format!("{base}/v1")
    }
}

/// 规范化 Google 原生基址：剥离旧配置中的 `/v1beta/openai` 后缀。
fn google_native_endpoint(endpoint: &str) -> String {
    let base = trim_slash(endpoint);
    if base.ends_with("/openai") {
        return base
            .trim_end_matches("/openai")
            .trim_end_matches("/v1beta")
            .to_string();
    }
    base
}

/// 规范化 Anthropic endpoint：剥离用户误加的 `/v1` 后缀，避免拼出 `/v1/v1/messages`。
fn anthropic_base(endpoint: &str) -> String {
    trim_slash(endpoint).trim_end_matches("/v1").to_string()
}

/// 缺少 API Key 时返回可读错误。
fn require_api_key(p: &ProviderConfig) -> Result<String, String> {
    let (has, source, env_name, key) = resolve_api_key(p);
    if !p.kind.requires_api_key() {
        return Ok(String::new());
    }
    key.filter(|_| has).ok_or_else(|| {
        let hint = p
            .kind
            .env_api_key_names()
            .first()
            .copied()
            .unwrap_or("API_KEY");
        format!(
            "未找到 API Key（密钥链或环境变量 {hint}）。当前来源: {source}{}",
            env_name.map(|n| format!(" / {n}")).unwrap_or_default()
        )
    })
}

/// Tauri 命令：list_provider_models。
#[tauri::command]
pub async fn list_provider_models(
    id: String,
    refresh: Option<bool>,
) -> Result<ProviderModelsResult, String> {
    let provider = find_provider(&id)?;
    validate_http_endpoint(&provider.endpoint)?;
    let api_key = require_api_key(&provider)?;
    if !refresh.unwrap_or(false) {
        // TTL 缓存：10 分钟内用缓存，不请求 API
        let cache = load_models_cache();
        if let Some(entry) = cache
            .providers
            .get(&id)
            .filter(|entry| cache_entry_is_current(entry))
        {
            if let Ok(updated) = chrono::DateTime::parse_from_rfc3339(&entry.updated_at) {
                let age = chrono::Utc::now().signed_duration_since(updated);
                let ttl =
                    home::cache::policy(&home::default_memory_dir(), home::cache::Domain::Models)
                        .map_err(|e| e.to_string())?
                        .ttl_seconds;
                if age.num_seconds() >= 0 && (age.num_seconds() as u64) <= ttl {
                    let kind = if entry.kind.is_empty() {
                        "custom"
                    } else {
                        entry.kind.as_str()
                    };
                    return Ok(ProviderModelsResult {
                        models: entry
                            .models
                            .iter()
                            .cloned()
                            .map(|e| e.into_info(kind))
                            .collect(),
                        latency_ms: entry.latency_ms,
                        source: format!("{} (cached)", entry.source),
                    });
                }
            }
        }
        drop(cache);
    }
    // 刷新模型前确保 OpenRouter 上下文/能力表可用（失败不阻断，走回落）
    if let Err(err) = crate::meta::openrouter_meta::ensure_cache(false).await {
        tracing::warn!(error = %err, "OpenRouter 模型表不可用，将使用 API/回落");
    }
    let client = http_client()?;
    let started = std::time::Instant::now();
    let kind = provider.kind.as_str();

    let (models, source) = match provider.kind {
        ProviderKind::Ollama => {
            // chat 默认走 /v1；模型列表仍用原生 /api/tags（需去掉 /v1 后缀）
            let root = trim_slash(&provider.endpoint)
                .trim_end_matches("/v1")
                .to_string();
            let url = format!("{root}/api/tags");
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("连接 Ollama 失败: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Ollama 返回 {}", resp.status()));
            }
            let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let models = body["models"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    let id = m["name"]
                        .as_str()
                        .or_else(|| m["model"].as_str())?
                        .to_string();
                    Some(crate::meta::model_meta::enrich_from_id(&id, kind, None))
                })
                .collect::<Vec<_>>();
            (models, "ollama:/api/tags".to_string())
        }
        ProviderKind::Anthropic => {
            let url = format!("{}/v1/models", anthropic_base(&provider.endpoint));
            let resp = client
                .get(&url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .send()
                .await
                .map_err(|e| format!("连接 Anthropic 失败: {e}"))?;
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            if !status.is_success() {
                return Err(format!(
                    "Anthropic 列表失败 ({status}): {}",
                    extract_api_error(&body)
                ));
            }
            let models = body["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    let hints = crate::meta::model_meta::ApiModelHints {
                        display_name: m["display_name"].as_str().map(str::to_string),
                        ..Default::default()
                    };
                    Some(crate::meta::model_meta::enrich_from_id(
                        &id,
                        kind,
                        Some(hints),
                    ))
                })
                .collect::<Vec<_>>();
            (models, "anthropic:/v1/models".to_string())
        }
        ProviderKind::Google => {
            // Interactions 原生基址：`/v1beta/models?key=`（自动剥离旧 /v1beta/openai）
            let base = google_native_endpoint(&provider.endpoint);
            let url = if base.contains("/v1beta") {
                format!("{base}/models?key={api_key}")
            } else {
                format!("{base}/v1beta/models?key={api_key}")
            };
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("连接 Google 失败: {e}"))?;
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            if !status.is_success() {
                return Err(format!(
                    "Google 列表失败 ({status}): {}",
                    extract_api_error(&body)
                ));
            }
            let models = body["models"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    let name = m["name"].as_str()?;
                    let id = name.strip_prefix("models/").unwrap_or(name).to_string();
                    let methods = m["supportedGenerationMethods"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect::<Vec<_>>();
                    let hints = crate::meta::model_meta::ApiModelHints {
                        display_name: m["displayName"].as_str().map(str::to_string),
                        context_window: m["inputTokenLimit"].as_u64(),
                        max_output_tokens: m["outputTokenLimit"].as_u64(),
                        supported_methods: methods,
                    };
                    Some(crate::meta::model_meta::enrich_from_id(
                        &id,
                        kind,
                        Some(hints),
                    ))
                })
                .collect::<Vec<_>>();
            (models, "google:/v1beta/models".to_string())
        }
        ProviderKind::Openai
        | ProviderKind::Deepseek
        | ProviderKind::Zhipu
        | ProviderKind::Openrouter
        | ProviderKind::Bailian
        | ProviderKind::Nvidia
        | ProviderKind::Moonshot
        | ProviderKind::Volcengine
        | ProviderKind::Minimax
        | ProviderKind::Hunyuan
        | ProviderKind::Custom => {
            let url = format!("{}/models", openai_compatible_base(&provider.endpoint));
            let mut req = client.get(&url);
            if !api_key.is_empty() {
                req = req.bearer_auth(&api_key);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| format!("连接模型服务失败: {e}"))?;
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            if !status.is_success() {
                return Err(format!(
                    "模型列表失败 ({status}): {}",
                    extract_api_error(&body)
                ));
            }
            let models = body["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    // 少数兼容网关会带 context_length / max_model_len
                    let ctx = m["context_length"]
                        .as_u64()
                        .or_else(|| m["context_window"].as_u64())
                        .or_else(|| m["max_model_len"].as_u64())
                        .or_else(|| m["max_tokens"].as_u64());
                    let hints = crate::meta::model_meta::ApiModelHints {
                        display_name: m["name"].as_str().map(str::to_string),
                        context_window: ctx,
                        max_output_tokens: m["max_output_tokens"].as_u64(),
                        supported_methods: Vec::new(),
                    };
                    let mut info = crate::meta::model_meta::enrich_from_id(
                        &id,
                        kind,
                        if ctx.is_some() { Some(hints) } else { None },
                    );
                    if kind == "openai" {
                        let persistent_instructions = m
                            .pointer("/model_messages/persistent_instructions")
                            .or_else(|| m.pointer("/reasoning/persistent_instructions"))
                            .or_else(|| m.get("persistent_instructions"))
                            .and_then(serde_json::Value::as_str)
                            .map(str::trim)
                            .filter(|instructions| !instructions.is_empty());
                        if let Some(instructions) = persistent_instructions {
                            info.capabilities.reasoning = true;
                            let reasoning = info.reasoning.get_or_insert_with(Default::default);
                            if !reasoning
                                .supported_efforts
                                .iter()
                                .any(|effort| effort == "persistent")
                            {
                                reasoning.supported_efforts.push("persistent".into());
                            }
                            reasoning.persistent_instructions = Some(instructions.into());
                        }
                    }
                    Some(info)
                })
                .collect::<Vec<_>>();
            (models, format!("{}:/models", provider.kind.as_str()))
        }
        ProviderKind::Azure => {
            let base = providers::impls::azure::azure_openai_v1_base(&provider.endpoint);
            let url = format!("{base}/models");
            let resp = client
                .get(&url)
                .bearer_auth(&api_key)
                .send()
                .await
                .map_err(|e| format!("连接 Azure OpenAI 失败: {e}"))?;
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            if !status.is_success() {
                return Err(format!(
                    "Azure 模型列表失败 ({status}): {}",
                    extract_api_error(&body)
                ));
            }
            let models = body["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    let id = m["id"]
                        .as_str()
                        .or_else(|| m["model"].as_str())?
                        .to_string();
                    Some(crate::meta::model_meta::enrich_from_id(&id, kind, None))
                })
                .collect::<Vec<_>>();
            (models, "azure:/openai/v1/models".to_string())
        }
    };

    let latency_ms = started.elapsed().as_millis() as u64;
    if let Err(err) = persist_provider_models(&provider, &models, &source, latency_ms) {
        tracing::warn!(error = %err, "failed to persist models.json");
    }

    Ok(ProviderModelsResult {
        models,
        latency_ms,
        source,
    })
}

/// 从 models.json 缓存读取完整 [`ModelInfo`]（与前端展示同源）。
pub fn cached_model_info(
    provider_id: &str,
    model_id: &str,
) -> Option<crate::meta::model_meta::ModelInfo> {
    let cache = load_models_cache();
    let entry = cache.providers.get(provider_id)?;
    if !cache_entry_is_current(entry) {
        return None;
    }
    let kind = if entry.kind.is_empty() {
        "custom"
    } else {
        entry.kind.as_str()
    };
    for m in &entry.models {
        match m {
            crate::meta::model_meta::ModelEntryCompat::Full(info) if info.id == model_id => {
                let mut info = info.as_ref().clone();
                crate::meta::model_meta::enrich_model_info(&mut info, kind, None);
                return Some(info);
            }
            crate::meta::model_meta::ModelEntryCompat::Id(id) if id == model_id => {
                return Some(crate::meta::model_meta::enrich_from_id(id, kind, None));
            }
            _ => {}
        }
    }
    // 缓存未命中时仍尝试 OpenRouter 表
    let info = crate::meta::model_meta::enrich_from_id(model_id, kind, None);
    if info.meta_source.is_empty() {
        None
    } else {
        Some(info)
    }
}

/// Tauri 命令：get_cached_provider_models。
#[tauri::command]
pub async fn get_cached_provider_models(
    id: String,
) -> Result<Option<ProviderModelsResult>, String> {
    let cache = load_models_cache();
    Ok(cache
        .providers
        .get(&id)
        .filter(|entry| cache_entry_is_current(entry))
        .map(|c| {
            let kind = if c.kind.is_empty() {
                "custom"
            } else {
                c.kind.as_str()
            };
            ProviderModelsResult {
                models: c
                    .models
                    .iter()
                    .cloned()
                    .map(|e| e.into_info(kind))
                    .collect(),
                latency_ms: c.latency_ms,
                source: c.source.clone(),
            }
        }))
}

/// 单模型连通性探测；协议实现已下沉到 `providers::verify`。
async fn probe_one_model(
    provider: &ProviderConfig,
    api_key: &str,
    _client: &reqwest::Client,
    model: String,
) -> ProviderTestResult {
    let bid = toml_backend_id(provider);
    let probe_id = bid.as_str();
    let config = providers::ProviderConfig {
        api_key: api_key.to_string(),
        base_url: Some(provider.endpoint.clone()),
        model: model.clone(),
        temperature: 0.0,
        max_tokens: 1,
        thinking_enabled: false,
        reasoning_effort: "high".to_string(),
        additional_params: serde_json::Value::Null,
        previous_interaction_id: None,
        api_mode: if providers::dispatch::supports_agent_responses(probe_id) {
            "responses".to_string()
        } else {
            "chat_completions".to_string()
        },
    };
    let result = providers::dispatch::verify(probe_id, &model, &config).await;
    ProviderTestResult {
        ok: result.ok,
        latency_ms: result.latency_ms,
        model: result.model,
        message: result.message,
    }
}

/// Tauri 命令：test_provider。
#[tauri::command]
pub async fn test_provider(
    id: String,
    model: Option<String>,
) -> Result<ProviderTestResult, String> {
    let provider = find_provider(&id)?;
    validate_http_endpoint(&provider.endpoint)?;
    let api_key = require_api_key(&provider)?;
    let model = model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| provider.model.clone());
    let client = http_client()?;
    Ok(probe_one_model(&provider, &api_key, &client, model).await)
}

/// 批量健康检查：对同一提供商下多个模型并发探测（上限 8 路），避免串行等待。
const BATCH_PROBE_CONCURRENCY: usize = 8;

/// Tauri 命令：test_provider_models。
#[tauri::command]
pub async fn test_provider_models(
    id: String,
    models: Vec<String>,
) -> Result<Vec<ProviderTestResult>, String> {
    let provider = find_provider(&id)?;
    let api_key = require_api_key(&provider)?;
    let client = http_client()?;
    let models: Vec<String> = models
        .into_iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    if models.is_empty() {
        return Ok(Vec::new());
    }

    use futures::stream::{self, StreamExt};
    let provider = std::sync::Arc::new(provider);
    let api_key = std::sync::Arc::new(api_key);
    let client = std::sync::Arc::new(client);

    let results = stream::iter(models)
        .map(|model| {
            let provider = provider.clone();
            let api_key = api_key.clone();
            let client = client.clone();
            async move { probe_one_model(&provider, &api_key, &client, model).await }
        })
        .buffer_unordered(BATCH_PROBE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "explicit isolated cross-process fixture; run tools/verify-settings-restart.mjs"]
    fn settings_restart_write_fixture() {
        let root = home::test_env::settings_restart_fixture_root();
        let _env = home::test_env::AstroMemoryDirGuard::set(&root);
        fs::write(
            home::config_path(&root),
            "# preserved foreign section\n[foreign]\nvalue = 42\n",
        )
        .unwrap();
        let mut provider = ProviderConfig::new(ProviderKind::Openai);
        provider.id = "restart-local".into();
        provider.model = "restart-model".into();
        provider.endpoint = "http://127.0.0.1:19999/v1".into();
        save_state(&ProvidersState {
            providers: vec![provider],
            active_provider_id: Some("restart-local".into()),
            active_image_provider_id: None,
        })
        .unwrap();
        home::save_tools_enabled(&std::collections::HashMap::from([(
            "exec_command".into(),
            false,
        )]))
        .unwrap();
        let skill = root.join("skills/restart-fixture");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: restart-fixture\ndescription: isolated test\n---\nfixture",
        )
        .unwrap();
        skills::installed::set_enabled_for_agent(Some("default"), &skill.to_string_lossy(), false)
            .unwrap();
        let agent = home::AgentRuntimeConfig {
            id: "default".into(),
            name: "Restart fixture".into(),
            inherit_from: None,
            provider_id: Some("restart-local".into()),
            model: Some("restart-model".into()),
            temperature: Some(0.3),
            max_turns: Some(17),
            additional_params: None,
            tools_enabled: None,
            created_at: "fixture".into(),
        };
        agent.save(&root).unwrap();
    }

    #[test]
    #[ignore = "explicit isolated cross-process fixture; run tools/verify-settings-restart.mjs"]
    fn settings_restart_read_fixture() {
        let root = home::test_env::settings_restart_fixture_root();
        let _env = home::test_env::AstroMemoryDirGuard::set(&root);
        let state = load_state().unwrap();
        let provider = state
            .providers
            .iter()
            .find(|p| p.id == "restart-local")
            .unwrap();
        assert_eq!(provider.model, "restart-model");
        assert_eq!(provider.endpoint, "http://127.0.0.1:19999/v1");
        assert_eq!(state.active_provider_id.as_deref(), Some("restart-local"));
        assert_eq!(
            home::load_tools_enabled().unwrap().get("exec_command"),
            Some(&false)
        );
        let skill = skills::installed::list_installed_for_agent(Some("default"), Some("astro"))
            .into_iter()
            .find(|s| s.name == "restart-fixture")
            .unwrap();
        assert!(!skill.enabled);
        let agent = home::AgentRuntimeConfig::load(&root, "default").unwrap();
        assert_eq!(agent.name, "Restart fixture");
        assert_eq!(agent.max_turns, Some(17));
        assert_eq!(agent.model.as_deref(), Some("restart-model"));
        let raw = fs::read_to_string(home::config_path(&root)).unwrap();
        assert!(raw.contains("# preserved foreign section"));
        assert!(raw.contains("value = 42"));
        for old in [
            "models/providers.json",
            "tools/enabled.json",
            "skills/enabled.json",
            "agents/default/config.json",
        ] {
            assert!(!root.join(old).exists(), "recreated {old}");
        }
    }

    #[test]
    fn custom_provider_configuration_respects_astro_memory_dir() {
        let dir = tempfile::tempdir().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        fs::write(home::settings::path(dir.path()), "[custom_providers.config_path_test]\nbase_url = 'http://127.0.0.1:4321/v1'\ndefault_model = 'local-model'\n").unwrap();
        let state = load_state().unwrap();
        let provider = state
            .providers
            .iter()
            .find(|p| p.id == "toml:config_path_test")
            .unwrap();
        assert_eq!(provider.model, "local-model");
    }

    #[test]
    fn provider_preferences_round_trip_through_toml_without_json_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        fs::write(home::settings::path(dir.path()), "# keep\n[mcp_servers]\n").unwrap();
        let mut state = ProvidersState::with_defaults();
        state.active_provider_id = Some(state.providers[0].id.clone());
        state.providers[0].endpoint = "http://127.0.0.1:4321/v1".into();
        save_state(&state).unwrap();
        let restored: ProvidersState = home::settings::read(dir.path(), &["desktop", "providers"])
            .unwrap()
            .unwrap();
        assert_eq!(restored.active_provider_id, state.active_provider_id);
        assert_eq!(restored.providers[0].endpoint, state.providers[0].endpoint);
        assert!(fs::read_to_string(home::settings::path(dir.path()))
            .unwrap()
            .contains("# keep"));
        assert!(!dir.path().join("providers.json").exists());
        assert!(!dir.path().join("models/providers.json").exists());
    }

    #[test]
    fn parses_new_kinds() {
        assert_eq!(
            ProviderKind::from_str("openrouter"),
            Some(ProviderKind::Openrouter)
        );
        assert_eq!(
            ProviderKind::from_str("bailian"),
            Some(ProviderKind::Bailian)
        );
        assert_eq!(ProviderKind::from_str("nvidia"), Some(ProviderKind::Nvidia));
        assert_eq!(
            ProviderKind::from_str("moonshot"),
            Some(ProviderKind::Moonshot)
        );
        assert_eq!(
            ProviderKind::from_str("volcengine"),
            Some(ProviderKind::Volcengine)
        );
        assert_eq!(
            ProviderKind::from_str("minimax"),
            Some(ProviderKind::Minimax)
        );
        assert_eq!(ProviderKind::from_str("minmax"), None);
        assert_eq!(ProviderKind::Minimax.as_str(), "minimax");
        assert_eq!(ProviderKind::Bailian.backend_id(), "bailian");
    }

    #[test]
    fn new_disabled_sets_enabled_false() {
        let p = ProviderConfig::new_disabled(ProviderKind::Openrouter);
        assert!(!p.enabled);
        assert_eq!(p.kind, ProviderKind::Openrouter);
        assert_eq!(p.endpoint, "https://openrouter.ai/api/v1");
    }

    #[test]
    fn agent_provider_capability_is_responses_only() {
        assert!(providers::dispatch::supports_agent_responses("deepseek"));
        assert!(providers::dispatch::supports_agent_responses("openai"));
        assert!(providers::dispatch::supports_agent_responses("openrouter"));
        assert!(!providers::dispatch::supports_agent_responses("ollama"));
    }

    #[test]
    fn active_model_update_preserves_advanced_provider_fields() {
        let mut provider = ProviderConfig::new(ProviderKind::Openai);
        provider.id = "provider-1".into();
        provider.fallback = vec![ProviderFallbackEntry {
            provider_id: "provider-2".into(),
            model: Some("fallback-model".into()),
        }];
        provider.image_model = "image-model".into();
        provider.video_model = "video-model".into();
        provider.tts_model = "tts-model".into();
        provider.vision_model = "vision-model".into();
        provider.music_model = "music-model".into();
        provider.embedding_model = "embedding-model".into();
        let mut state = ProvidersState {
            providers: vec![provider.clone()],
            active_provider_id: None,
            active_image_provider_id: None,
        };

        set_active_provider_model_in_state(&mut state, "provider-1", "next-model").unwrap();

        let updated = &state.providers[0];
        assert_eq!(updated.model, "next-model");
        assert_eq!(updated.fallback, provider.fallback);
        assert_eq!(updated.image_model, provider.image_model);
        assert_eq!(updated.video_model, provider.video_model);
        assert_eq!(updated.tts_model, provider.tts_model);
        assert_eq!(updated.vision_model, provider.vision_model);
        assert_eq!(updated.music_model, provider.music_model);
        assert_eq!(updated.embedding_model, provider.embedding_model);
        assert_eq!(state.active_provider_id.as_deref(), Some("provider-1"));
    }

    #[test]
    fn fresh_catalog_is_not_user_configuration() {
        let state = ProvidersState::with_defaults();
        assert_eq!(state.providers.len(), 14);
        let dto = to_state_dto(&state);
        assert!(dto.providers.is_empty());
        assert_eq!(dto.provider_templates.len(), 14);
        assert!(dto.active_provider_id.is_none());
        assert!(dto
            .provider_templates
            .iter()
            .all(|p| p.config_source == "builtin"));
    }

    #[test]
    fn explicit_add_survives_reload_without_a_key() {
        let mut state = ProvidersState::with_defaults();
        let id = state
            .providers
            .iter()
            .find(|p| p.kind == ProviderKind::Openai)
            .unwrap()
            .id
            .clone();
        state.add_provider(ProviderKind::Openai);
        assert_eq!(state.providers.len(), 14);
        assert_eq!(state.providers.last().unwrap().id, id);
        let mut restored: ProvidersState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert!(!restored.migrate_added_flags(|_| false));
        let dto = to_state_dto(&restored);
        assert_eq!(dto.providers.len(), 1);
        assert_eq!(dto.providers[0].id, id);
        assert_eq!(dto.providers[0].config_source, "user");
        restored.add_provider(ProviderKind::Openai);
        assert_eq!(to_state_dto(&restored).providers.len(), 2);
        assert_ne!(restored.providers.last().unwrap().id, id);
    }

    #[test]
    fn legacy_accounts_are_classified_once_without_losing_rows() {
        let mut state = ProvidersState::with_defaults();
        for provider in &mut state.providers {
            provider.added = None;
        }
        let keyed = state.providers[1].id.clone();
        state.providers[2].endpoint = "https://custom.example/v1".into();
        let mut duplicate = ProviderConfig::new(ProviderKind::Openai);
        duplicate.added = None;
        state.providers.push(duplicate);
        assert!(state.migrate_added_flags(|p| p.id == keyed));
        assert_eq!(state.providers.len(), 15);
        assert_eq!(state.providers.iter().filter(|p| p.is_added()).count(), 3);
        // A later environment key must not silently add every template again.
        assert!(!state.migrate_added_flags(|_| true));
        assert_eq!(state.providers.iter().filter(|p| p.is_added()).count(), 3);
    }

    #[test]
    fn reorder_only_requires_added_ids_and_delete_last_stays_empty() {
        let mut state = ProvidersState::with_defaults();
        let first = ProviderConfig::new(ProviderKind::Openai);
        let second = ProviderConfig::new(ProviderKind::Google);
        let first_id = first.id.clone();
        let second_id = second.id.clone();
        state.providers.extend([first, second]);
        state.active_provider_id = Some(first_id.clone());
        let before = serde_json::to_value(&state).unwrap();
        assert!(state
            .reorder_added(vec![first_id.clone(), first_id.clone()])
            .is_err());
        assert_eq!(serde_json::to_value(&state).unwrap(), before);
        state
            .reorder_added(vec![second_id.clone(), first_id.clone()])
            .unwrap();
        assert_eq!(state.providers[0].id, second_id);
        state.remove_provider(&first_id).unwrap();
        state.remove_provider(&second_id).unwrap();
        let mut restored: ProvidersState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        restored.ensure_builtin_kinds();
        restored.migrate_added_flags(|_| true);
        assert_eq!(restored.providers.len(), 14);
        assert!(to_state_dto(&restored).providers.is_empty());
        assert!(restored.active_provider_id.is_none());
    }

    #[test]
    fn legacy_selected_local_provider_is_retained_without_credentials() {
        let mut state = ProvidersState::with_defaults();
        let local = state
            .providers
            .iter_mut()
            .find(|p| p.kind == ProviderKind::Ollama)
            .unwrap();
        local.added = None;
        let id = local.id.clone();
        state.active_provider_id = Some(id.clone());
        state.migrate_added_flags(|_| false);
        assert_eq!(to_state_dto(&state).providers[0].id, id);
    }

    #[test]
    fn selecting_a_template_model_promotes_it_to_added() {
        let mut state = ProvidersState::with_defaults();
        let id = state
            .providers
            .iter()
            .find(|p| p.kind == ProviderKind::Openai)
            .unwrap()
            .id
            .clone();
        let dto = set_active_provider_model_in_state(&mut state, &id, "chosen-model").unwrap();
        assert_eq!(dto.providers.len(), 1);
        assert_eq!(dto.providers[0].model, "chosen-model");
        assert!(!dto.provider_templates.iter().any(|p| p.id == id));
        assert_eq!(dto.active_provider_id.as_deref(), Some(id.as_str()));
    }

    #[test]
    fn ensure_builtin_adds_six_disabled() {
        let mut s = ProvidersState::default();
        assert!(s.ensure_builtin_kinds());
        for kind in [
            ProviderKind::Openrouter,
            ProviderKind::Bailian,
            ProviderKind::Nvidia,
            ProviderKind::Moonshot,
            ProviderKind::Volcengine,
            ProviderKind::Minimax,
        ] {
            let p = s
                .providers
                .iter()
                .find(|p| p.kind == kind)
                .expect("missing kind");
            assert!(!p.enabled, "{:?} should be disabled", kind);
        }
        assert!(!s.ensure_builtin_kinds()); // idempotent
    }

    #[test]
    fn with_defaults_six_new_are_disabled() {
        let s = ProvidersState::with_defaults();
        for kind in [
            ProviderKind::Openrouter,
            ProviderKind::Bailian,
            ProviderKind::Nvidia,
            ProviderKind::Moonshot,
            ProviderKind::Volcengine,
            ProviderKind::Minimax,
        ] {
            let p = s
                .providers
                .iter()
                .find(|p| p.kind == kind)
                .expect("missing kind");
            assert!(!p.enabled, "{:?} should be disabled in with_defaults", kind);
        }
    }

    #[test]
    fn six_provider_default_endpoint_and_model() {
        assert_eq!(
            ProviderKind::Openrouter.default_endpoint(),
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(ProviderKind::Openrouter.default_model(), "openai/gpt-5.6");
        assert_eq!(
            ProviderKind::Bailian.default_endpoint(),
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        );
        assert_eq!(ProviderKind::Bailian.default_model(), "qwen3.8-max");
        assert_eq!(
            ProviderKind::Nvidia.default_endpoint(),
            "https://integrate.api.nvidia.com/v1"
        );
        assert_eq!(
            ProviderKind::Nvidia.default_model(),
            "meta/llama-3.3-70b-instruct"
        );
        assert_eq!(
            ProviderKind::Moonshot.default_endpoint(),
            "https://api.moonshot.cn/v1"
        );
        assert_eq!(ProviderKind::Moonshot.default_model(), "kimi-k2.5");
        assert_eq!(
            ProviderKind::Volcengine.default_endpoint(),
            "https://ark.cn-beijing.volces.com/api/v3"
        );
        assert_eq!(
            ProviderKind::Volcengine.default_model(),
            "doubao-seed-2.1-pro"
        );
        assert_eq!(
            ProviderKind::Minimax.default_endpoint(),
            "https://api.minimaxi.com/v1"
        );
        assert_eq!(ProviderKind::Minimax.default_model(), "MiniMax-M3");
    }

    #[test]
    fn serde_uses_minimax_id() {
        let raw = r#"{"id":"x","kind":"minimax","display_name":"M","endpoint":"https://api.minimaxi.com/v1","model":"MiniMax-M2.5","enabled":false}"#;
        let p: ProviderConfig = serde_json::from_str(raw).expect("minimax provider id");
        assert_eq!(p.kind, ProviderKind::Minimax);
        assert!(p.fallback.is_empty());
        let out = serde_json::to_value(&p).unwrap();
        assert_eq!(out["kind"], "minimax");
    }

    #[test]
    fn serde_rejects_minmax_typo() {
        let raw = r#"{"id":"x","kind":"minmax","display_name":"M","endpoint":"https://api.minimaxi.com/v1","model":"MiniMax-M2.5","enabled":false}"#;
        assert!(serde_json::from_str::<ProviderConfig>(raw).is_err());
    }

    #[test]
    fn serde_providers_without_fallback_loads() {
        let raw = r#"{"id":"p1","kind":"openai","display_name":"O","endpoint":"https://api.openai.com/v1","model":"gpt","enabled":true}"#;
        let p: ProviderConfig = serde_json::from_str(raw).expect("no fallback field");
        assert!(p.fallback.is_empty());

        let with_fb = r#"{"id":"p1","kind":"openai","display_name":"O","endpoint":"https://api.openai.com/v1","model":"gpt","enabled":true,"fallback":[{"provider_id":"p2","model":"m"}]}"#;
        let p2: ProviderConfig = serde_json::from_str(with_fb).expect("with fallback");
        assert_eq!(p2.fallback.len(), 1);
        assert_eq!(p2.fallback[0].provider_id, "p2");
        assert_eq!(p2.fallback[0].model.as_deref(), Some("m"));
    }

    #[test]
    fn provider_without_music_model_deserializes_to_empty() {
        let json = r#"{
          "id":"google-1","kind":"google","display_name":"Google",
          "endpoint":"https://generativelanguage.googleapis.com",
          "model":"gemini-3.5-flash","enabled":true
        }"#;
        let provider: ProviderConfig = serde_json::from_str(json).unwrap();
        assert_eq!(provider.music_model, "");
        assert_eq!(provider.embedding_model, "");
    }

    #[test]
    fn google_music_model_defaults_to_lyria_pro() {
        assert_eq!(
            default_music_model_for_kind(&ProviderKind::Google),
            "lyria-3-pro"
        );
        assert_eq!(default_music_model_for_kind(&ProviderKind::Openai), "");
    }

    #[test]
    fn azure_image_model_defaults_to_flare() {
        assert_eq!(
            default_image_model_for_kind(&ProviderKind::Azure),
            Some("gpt-image-2.5-flare")
        );
    }

    #[test]
    fn image_dto_keeps_auto_blank_and_preserves_manual_selection() {
        for kind in [ProviderKind::Openai, ProviderKind::Azure] {
            let mut provider = ProviderConfig::new_disabled(kind);
            assert_eq!(to_dto(&provider).image_model, "");
            provider.image_model = "my-sunburst-deployment".into();
            assert_eq!(to_dto(&provider).image_model, "my-sunburst-deployment");
        }
        let google = ProviderConfig::new_disabled(ProviderKind::Google);
        assert!(!to_dto(&google).image_model.is_empty());
    }

    #[test]
    fn azure_defaults_to_latest_openai_v1_endpoint() {
        assert_eq!(
            ProviderKind::Azure.default_endpoint(),
            "https://YOUR_RESOURCE.services.ai.azure.com/openai/v1"
        );
        assert_eq!(ProviderKind::Azure.default_model(), "gpt-5.6-sol");
        let provider = ProviderConfig::new(ProviderKind::Azure);
        let dto = to_dto(&provider);
        assert!(dto.supports_embedding);
        assert_eq!(dto.embedding_model, "text-embedding-3-small");
    }

    #[test]
    fn providers_state_without_image_default_is_backward_compatible() {
        let state: ProvidersState =
            serde_json::from_str(r#"{"providers":[],"active_provider_id":null}"#)
                .expect("legacy state");
        assert_eq!(state.active_image_provider_id, None);
    }

    #[test]
    fn invalid_image_default_is_cleared() {
        let mut provider = ProviderConfig::new(ProviderKind::Azure);
        provider.id = "azure-image".to_string();
        provider.enabled = false;
        let mut state = ProvidersState {
            providers: vec![provider],
            active_provider_id: None,
            active_image_provider_id: Some("azure-image".to_string()),
        };
        assert!(state.ensure_unique_ids());
        assert_eq!(state.active_image_provider_id, None);
    }

    #[test]
    fn validate_http_endpoint_rejects_non_http() {
        assert!(validate_http_endpoint("https://ok.example/v1").is_ok());
        assert!(validate_http_endpoint("http://localhost:11434").is_ok());
        assert!(validate_http_endpoint("file:///etc/passwd").is_err());
        assert!(validate_http_endpoint("").is_err());
        assert!(validate_http_endpoint("ftp://x").is_err());
        // 仅有 scheme 无主机名
        assert!(validate_http_endpoint("https://").is_err());
        assert!(validate_http_endpoint("http://").is_err());
    }

    #[test]
    fn anthropic_base_strips_v1() {
        assert_eq!(
            anthropic_base("https://api.anthropic.com/v1"),
            "https://api.anthropic.com"
        );
        assert_eq!(
            anthropic_base("https://api.anthropic.com/v1/"),
            "https://api.anthropic.com"
        );
        assert_eq!(
            anthropic_base("https://my-proxy.com"),
            "https://my-proxy.com"
        );
        assert_eq!(
            anthropic_base("https://my-proxy.com/"),
            "https://my-proxy.com"
        );
    }

    #[test]
    fn migrate_stale_defaults_updates_known_old_values() {
        let mut s = ProvidersState {
            active_provider_id: None,
            active_image_provider_id: None,
            providers: vec![
                ProviderConfig {
                    id: "m1".into(),
                    kind: ProviderKind::Moonshot,
                    display_name: "月之暗面".into(),
                    endpoint: ProviderKind::Moonshot.default_endpoint().into(),
                    model: "kimi-k2-0711-preview".into(),
                    enabled: false,
                    fallback: vec![],
                    image_model: String::new(),
                    video_model: String::new(),
                    tts_model: String::new(),
                    vision_model: String::new(),
                    music_model: String::new(),
                    embedding_model: String::new(),
                    added: Some(true),
                },
                ProviderConfig {
                    id: "v1".into(),
                    kind: ProviderKind::Volcengine,
                    display_name: "火山".into(),
                    endpoint: ProviderKind::Volcengine.default_endpoint().into(),
                    model: "doubao-pro-32k".into(),
                    enabled: false,
                    fallback: vec![],
                    image_model: String::new(),
                    video_model: String::new(),
                    tts_model: String::new(),
                    vision_model: String::new(),
                    music_model: String::new(),
                    embedding_model: String::new(),
                    added: Some(true),
                },
                ProviderConfig {
                    id: "x1".into(),
                    kind: ProviderKind::Minimax,
                    display_name: "MiniMax".into(),
                    endpoint: "https://api.minimax.chat/v1".into(),
                    model: "MiniMax-M2.5".into(),
                    enabled: false,
                    fallback: vec![],
                    image_model: String::new(),
                    video_model: String::new(),
                    tts_model: String::new(),
                    vision_model: String::new(),
                    music_model: String::new(),
                    embedding_model: String::new(),
                    added: Some(true),
                },
                ProviderConfig {
                    id: "g1".into(),
                    kind: ProviderKind::Google,
                    display_name: "Google".into(),
                    endpoint: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
                    model: "gemini-1.5-flash".into(),
                    enabled: false,
                    fallback: vec![],
                    image_model: String::new(),
                    video_model: String::new(),
                    tts_model: String::new(),
                    vision_model: String::new(),
                    music_model: String::new(),
                    embedding_model: String::new(),
                    added: Some(true),
                },
            ],
        };
        assert!(s.migrate_stale_defaults());
        assert_eq!(s.providers[0].model, "kimi-k2.5");
        assert_eq!(s.providers[1].model, "doubao-seed-2.1-pro");
        assert_eq!(s.providers[2].endpoint, "https://api.minimaxi.com/v1");
        assert_eq!(s.providers[2].model, "MiniMax-M3");
        assert_eq!(
            s.providers[3].endpoint,
            "https://generativelanguage.googleapis.com"
        );
        assert_eq!(s.providers[3].model, "gemini-3.1-ultra");
        assert!(!s.migrate_stale_defaults());
    }

    #[test]
    fn persist_provider_models_concurrent_writers_do_not_enoent() {
        let dir = std::env::temp_dir().join(format!(
            "astro-models-cache-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(&dir);

        let n = 16usize;
        let mut handles = Vec::new();
        for i in 0..n {
            handles.push(std::thread::spawn(move || {
                let mut p = ProviderConfig::new(ProviderKind::Openai);
                p.id = format!("p{i}");
                p.display_name = format!("P{i}");
                // 拉开一点窗口，模拟多路 list_provider_models 同时回写
                std::thread::sleep(std::time::Duration::from_micros(80 * (i as u64 % 4 + 1)));
                persist_provider_models(&p, &[], "test", 1)
            }));
        }

        let mut errs = Vec::new();
        for h in handles {
            match h.join().expect("thread") {
                Ok(()) => {}
                Err(e) => errs.push(e),
            }
        }
        let cache = load_models_cache();
        let _ = fs::remove_dir_all(&dir);
        assert!(
            errs.is_empty(),
            "concurrent persist failed (fixed tmp race?): {errs:?}"
        );
        assert_eq!(
            cache.providers.len(),
            n,
            "lost providers under concurrent RMW: {:?}",
            cache.providers.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn model_cache_invalidates_changed_provider_definition() {
        let dir = tempfile::tempdir().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        let mut provider = ProviderConfig::new(ProviderKind::Ollama);
        provider.id = "cache-fixture".into();
        let state = ProvidersState {
            providers: vec![provider.clone()],
            ..Default::default()
        };
        home::settings::write(dir.path(), &["desktop", "providers"], &state).unwrap();
        let before = fs::read(home::config_path(dir.path())).unwrap();
        persist_provider_models(&provider, &[], "test", 1).unwrap();
        let cache = load_models_cache();
        let entry = cache.providers.get(&provider.id).unwrap();
        assert!(cache_entry_is_current(entry));
        assert_eq!(fs::read(home::config_path(dir.path())).unwrap(), before);
        provider.endpoint = "http://127.0.0.1:11435".into();
        let state = ProvidersState {
            providers: vec![provider],
            ..Default::default()
        };
        home::settings::write(dir.path(), &["desktop", "providers"], &state).unwrap();
        assert!(!cache_entry_is_current(entry));
    }
}
