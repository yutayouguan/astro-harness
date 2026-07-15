//! 模型供应商配置、校验与 LiteLLM/元数据相关 Tauri 命令。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::keystore::{
    delete_api_key, has_api_key, keyring_service_for_provider, load_api_key, save_api_key,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    #[serde(alias = "minmax")]
    Minimax,
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
            "minimax" | "minmax" => Some(Self::Minimax),
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
            Self::Google => "https://generativelanguage.googleapis.com/v1beta/openai",
            Self::Azure => "https://YOUR_RESOURCE.openai.azure.com",
            Self::Zhipu => "https://open.bigmodel.cn/api/paas/v4",
            Self::Openrouter => "https://openrouter.ai/api/v1",
            Self::Bailian => "https://dashscope.aliyuncs.com/compatible-mode/v1",
            Self::Nvidia => "https://integrate.api.nvidia.com/v1",
            Self::Moonshot => "https://api.moonshot.cn/v1",
            Self::Volcengine => "https://ark.cn-beijing.volces.com/api/v3",
            Self::Minimax => "https://api.minimax.io/v1",
            Self::Custom => "http://localhost:11434/v1",
        }
    }

    /// 该供应商的默认模型 id。
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Openai => "gpt-5.6",
            Self::Anthropic => "claude-opus-4-8",
            Self::Deepseek => "deepseek-chat",
            Self::Ollama => "llama3.3",
            Self::Google => "gemini-3.5-flash",
            Self::Azure => "gpt-5.6",
            Self::Zhipu => "glm-4.7-flash",
            Self::Openrouter => "openai/gpt-5.6",
            Self::Bailian => "qwen3.6-plus",
            Self::Nvidia => "meta/llama-3.3-70b-instruct",
            Self::Moonshot => "kimi-k2.5",
            Self::Volcengine => "ep-",
            Self::Minimax => "MiniMax-M2.5",
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
            Self::Minimax => &["MINIMAX_API_KEY", "MINMAX_API_KEY"],
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

/// 单条聊天后备引用（写入 `providers.json`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProviderFallbackEntry {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
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
}

impl ProviderConfig {
    /// 创建默认实例。
    pub fn new(kind: ProviderKind) -> Self {
        Self {
            display_name: kind.display_name().to_string(),
            endpoint: kind.default_endpoint().to_string(),
            model: kind.default_model().to_string(),
            enabled: true,
            id: format!("prov-{}", uuid::Uuid::new_v4().simple()),
            kind,
            fallback: Vec::new(),
            image_model: String::new(),
            video_model: String::new(),
            tts_model: String::new(),
        }
    }

    /// 创建默认禁用的 Provider 条目。
    pub fn new_disabled(kind: ProviderKind) -> Self {
        let mut p = Self::new(kind);
        p.enabled = false;
        p
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProvidersState {
    pub providers: Vec<ProviderConfig>,
    pub active_provider_id: Option<String>,
}

impl ProvidersState {
    /// 用内置默认集填充缺失供应商。
    pub fn with_defaults() -> Self {
        let mut s = Self::default();
        for kind in [
            ProviderKind::Anthropic,
            ProviderKind::Openai,
            ProviderKind::Google,
            ProviderKind::Deepseek,
            ProviderKind::Azure,
            ProviderKind::Zhipu,
            ProviderKind::Ollama,
        ] {
            s.providers.push(ProviderConfig::new(kind));
        }
        for kind in [
            ProviderKind::Openrouter,
            ProviderKind::Bailian,
            ProviderKind::Nvidia,
            ProviderKind::Moonshot,
            ProviderKind::Volcengine,
            ProviderKind::Minimax,
        ] {
            s.providers.push(ProviderConfig::new_disabled(kind));
        }
        if let Some(first) = s.providers.first() {
            s.active_provider_id = Some(first.id.clone());
        }
        s
    }

    /// 为已有配置补齐新增内置提供商（Azure / 智谱 / 六个新提供商）。
    pub fn ensure_builtin_kinds(&mut self) -> bool {
        let mut changed = false;
        for kind in [ProviderKind::Azure, ProviderKind::Zhipu] {
            if !self.providers.iter().any(|p| p.kind == kind) {
                self.providers.push(ProviderConfig::new(kind));
                changed = true;
            }
        }
        for kind in [
            ProviderKind::Openrouter,
            ProviderKind::Bailian,
            ProviderKind::Nvidia,
            ProviderKind::Moonshot,
            ProviderKind::Volcengine,
            ProviderKind::Minimax,
        ] {
            if !self.providers.iter().any(|p| p.kind == kind) {
                self.providers.push(ProviderConfig::new_disabled(kind));
                changed = true;
            }
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
                    p.model = "ep-".to_string();
                    changed = true;
                }
                ProviderKind::Minimax
                    if p.endpoint.trim_end_matches('/') == "https://api.minimax.chat/v1" =>
                {
                    p.endpoint = "https://api.minimax.io/v1".to_string();
                    changed = true;
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
}

#[derive(Debug, Clone, Serialize)]
pub struct ProvidersStateDto {
    pub providers: Vec<ProviderConfigDto>,
    pub active_provider_id: Option<String>,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderModelsResult {
    pub models: Vec<crate::model_meta::ModelInfo>,
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

/// providers.json 路径。
fn providers_path() -> PathBuf {
    memory::default_memory_dir().join("providers.json")
}

/// 模型缓存文件路径。
fn models_path() -> PathBuf {
    memory::default_memory_dir().join("models.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ModelsCacheFile {
    #[serde(default)]
    providers: HashMap<String, CachedProviderModels>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedProviderModels {
    provider_id: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    models: Vec<crate::model_meta::ModelEntryCompat>,
    #[serde(default)]
    source: String,
    #[serde(default)]
    latency_ms: u64,
    updated_at: String,
}

/// 从磁盘加载模型列表缓存。
fn load_models_cache() -> ModelsCacheFile {
    let path = models_path();
    if !path.exists() {
        return ModelsCacheFile::default();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// 将模型列表缓存写入磁盘。
fn save_models_cache(cache: &ModelsCacheFile) -> Result<(), String> {
    let path = models_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(cache).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// 持久化某一 Provider 的模型列表。
fn persist_provider_models(
    provider: &ProviderConfig,
    models: &[crate::model_meta::ModelInfo],
    source: &str,
    latency_ms: u64,
) -> Result<(), String> {
    let mut cache = load_models_cache();
    cache.providers.insert(
        provider.id.clone(),
        CachedProviderModels {
            provider_id: provider.id.clone(),
            display_name: provider.display_name.clone(),
            kind: provider.kind.as_str().to_string(),
            models: models
                .iter()
                .cloned()
                .map(crate::model_meta::ModelEntryCompat::Full)
                .collect(),
            source: source.to_string(),
            latency_ms,
            updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        },
    );
    save_models_cache(&cache)
}

/// 加载全部 Provider 配置状态。
fn load_state() -> Result<ProvidersState, String> {
    let path = providers_path();
    if !path.exists() {
        return Ok(ProvidersState::with_defaults());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut state: ProvidersState = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let mut changed = state.ensure_unique_ids();
    if state.ensure_builtin_kinds() {
        changed = true;
    }
    if state.migrate_stale_defaults() {
        changed = true;
    }
    if changed {
        let _ = save_state(&state);
    }
    Ok(state)
}

/// 保存 Provider 配置状态。
fn save_state(state: &ProvidersState) -> Result<(), String> {
    let path = providers_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| e.to_string())
}

/// `resolve_api_key`。
pub(crate) fn resolve_api_key(p: &ProviderConfig) -> (bool, String, Option<String>, Option<String>) {
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

/// 单条 Provider → 前端 DTO。
fn to_dto(p: &ProviderConfig) -> ProviderConfigDto {
    let (has_api_key, key_source, env_key_name, _) = resolve_api_key(p);
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
        backend_id: p.kind.backend_id().to_string(),
        official_key_url: p.kind.official_key_url().map(str::to_string),
        fallback: p.fallback.clone(),
        image_model: p.image_model.clone(),
        video_model: p.video_model.clone(),
        tts_model: p.tts_model.clone(),
    }
}

/// 整体状态 → 前端 DTO。
fn to_state_dto(state: &ProvidersState) -> ProvidersStateDto {
    ProvidersStateDto {
        providers: state.providers.iter().map(to_dto).collect(),
        active_provider_id: state.active_provider_id.clone(),
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
    let state = guard.as_mut().unwrap();
    let result = f(state)?;
    save_state(state)?;
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
            .filter(|p| p.enabled)
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
        // 新六家与 builtin 一致：添加后默认关闭，避免未配 Key 就占用「可用」
        let cfg = match kind {
            ProviderKind::Openrouter
            | ProviderKind::Bailian
            | ProviderKind::Nvidia
            | ProviderKind::Moonshot
            | ProviderKind::Volcengine
            | ProviderKind::Minimax => ProviderConfig::new_disabled(kind),
            _ => ProviderConfig::new(kind),
        };
        s.providers.push(cfg);
        Ok(to_state_dto(s))
    })
}

/// 保存单个供应商配置。
#[tauri::command]
pub fn save_provider(provider: ProviderConfigInput) -> Result<ProvidersStateDto, String> {
    let kind =
        ProviderKind::from_str(&provider.kind).ok_or_else(|| format!("未知提供商类型: {}", provider.kind))?;
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
            .take(common::MAX_CHAT_FALLBACKS)
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
        };
        Ok(to_state_dto(s))
    })
}

/// Tauri 命令：reorder_providers。
#[tauri::command]
pub fn reorder_providers(ids: Vec<String>) -> Result<ProvidersStateDto, String> {
    with_state_mut(|s| {
        if ids.len() != s.providers.len() {
            return Err("排序列表长度不匹配".to_string());
        }
        let mut by_id: std::collections::HashMap<String, ProviderConfig> = s
            .providers
            .drain(..)
            .map(|p| (p.id.clone(), p))
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
        s.providers = next;
        Ok(to_state_dto(s))
    })
}

/// Tauri 命令：delete_provider。
#[tauri::command]
pub fn delete_provider(id: String) -> Result<ProvidersStateDto, String> {
    with_state_mut(|s| {
        if s.providers.len() <= 1 {
            return Err("至少保留一个提供商".to_string());
        }
        let matches: Vec<_> = s
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
            s.providers.remove(idx);
        }
        if s.providers.is_empty() {
            return Err("至少保留一个提供商".to_string());
        }
        if s.active_provider_id.as_deref() == Some(&id)
            || s.active_provider_id
                .as_ref()
                .is_some_and(|aid| !s.providers.iter().any(|p| &p.id == aid))
        {
            s.active_provider_id = s.providers.first().map(|p| p.id.clone());
        }
        let _ = delete_api_key(&keyring_service_for_provider(&id));
        Ok(to_state_dto(s))
    })
}

/// Tauri 命令：set_active_provider。
#[tauri::command]
pub fn set_active_provider(id: String) -> Result<ProvidersStateDto, String> {
    with_state_mut(|s| {
        if !s.providers.iter().any(|p| p.id == id && p.enabled) {
            return Err("提供商不存在或未启用".to_string());
        }
        s.active_provider_id = Some(id);
        Ok(to_state_dto(s))
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
            .find(|p| p.enabled && p.kind.backend_id() == backend_id)
            .cloned()
            .or_else(|| {
                s.providers
                    .iter()
                    .find(|p| p.kind.backend_id() == backend_id)
                    .cloned()
            })
            .ok_or_else(|| format!("未找到 backend_id={backend_id} 的提供商"))
    })
}

/// 从 providers.json + keyring 展开主目标与聊天后备链（含 primary）。
///
/// - primary：与 `resolve_chat_credentials` 相同的查找规则；`model` 空则用条目默认模型
/// - fallback：跳过禁用、无 Key（ollama 除外）、缺失条目；去重与上限由 `expand_chat_targets` 负责
pub fn resolve_chat_targets(
    primary_provider_id: Option<&str>,
    backend_hint: &str,
    model: &str,
) -> Result<Vec<common::ChatTarget>, String> {
    let cfg: ProviderConfig = if let Some(id) = primary_provider_id.filter(|s| !s.is_empty()) {
        find_provider(id)?
    } else {
        find_provider_by_backend(backend_hint)?
    };

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

    let primary = common::ChatTarget {
        provider_id: cfg.id.clone(),
        backend_id: cfg.kind.backend_id().to_string(),
        model,
        api_key: key.unwrap_or_default(),
        base_url: cfg.endpoint.clone(),
    };

    let refs: Vec<common::FallbackRef> = cfg
        .fallback
        .iter()
        .map(|e| common::FallbackRef {
            provider_id: e.provider_id.clone(),
            model: e.model.clone(),
        })
        .collect();

    let chain = common::expand_chat_targets(&primary, &refs, |id| {
        let Ok(p) = find_provider(id) else {
            return None;
        };
        if !p.enabled {
            return None;
        }
        let (_has, _source, _env, key) = resolve_api_key(&p);
        let api_key = key.unwrap_or_default();
        let allow_empty_key = p.kind.backend_id() == "ollama";
        if api_key.trim().is_empty() && !allow_empty_key {
            return None;
        }
        Some(common::ChatTarget {
            provider_id: p.id.clone(),
            backend_id: p.kind.backend_id().to_string(),
            model: p.model.clone(),
            api_key,
            base_url: p.endpoint.clone(),
        })
    });

    Ok(chain)
}

/// 图片生成目标（已开启 + 有 API Key 的 Google / OpenAI）
#[derive(Debug, Clone, Serialize)]
pub struct ImageGenTarget {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub display_name: String,
    pub video_model: String,
    pub tts_model: String,
}

const IMAGE_GEN_NO_PROVIDER_MSG: &str =
    "未找到可用的图片生成提供商。请在「模型提供商」中开启 Google 或 OpenAI，并配置 API Key。";

/// 按供应商类型选择默认图片模型。
fn default_image_model_for_kind(kind: &ProviderKind) -> Option<&'static str> {
    match kind {
        ProviderKind::Google => Some("gemini-3.1-flash-image"),
        ProviderKind::Openai => Some("gpt-image-2"),
        _ => None,
    }
}

fn default_video_model_for_kind(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Google => "veo-3.1-generate-preview",
        _ => "",
    }
}

fn default_tts_model_for_kind(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Google => "gemini-3.1-flash-tts-preview",
        ProviderKind::Openai => "gpt-4o-mini-tts",
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

/// 从 providers 面板解析图片生成候选：Google 优先，OpenAI 备用。
pub fn resolve_image_gen_targets() -> Result<Vec<ImageGenTarget>, String> {
    with_state(|s| {
        let mut google: Option<ImageGenTarget> = None;
        let mut openai: Option<ImageGenTarget> = None;

        for p in &s.providers {
            if !p.enabled {
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
                model: resolve_media_model(&p.image_model, default_image),
                api_key,
                base_url: p.endpoint.clone(),
                display_name: p.display_name.clone(),
                video_model: resolve_media_model(
                    &p.video_model,
                    default_video_model_for_kind(&p.kind),
                ),
                tts_model: resolve_media_model(&p.tts_model, default_tts_model_for_kind(&p.kind)),
            };
            match p.kind {
                ProviderKind::Google if google.is_none() => google = Some(target),
                ProviderKind::Openai if openai.is_none() => openai = Some(target),
                _ => {}
            }
        }

        let mut targets = Vec::new();
        if let Some(g) = google {
            targets.push(g);
        }
        if let Some(o) = openai {
            targets.push(o);
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

/// 仅允许 http(s) Endpoint，拒绝 file:// 等危险 scheme（防 SSRF）。
fn validate_http_endpoint(endpoint: &str) -> Result<(), String> {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() {
        return Err("Endpoint 不能为空".to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("Endpoint 仅支持 http:// 或 https://".to_string());
    }
    Ok(())
}

/// 规范化 OpenAI 兼容 API 基址。
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

const AZURE_API_VERSION: &str = "2024-06-01";

/// 规范化 Azure OpenAI endpoint 根路径。
fn azure_base(endpoint: &str) -> String {
    trim_slash(endpoint)
        .trim_end_matches("/openai")
        .trim_end_matches("/v1")
        .to_string()
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
            env_name
                .map(|n| format!(" / {n}"))
                .unwrap_or_default()
        )
    })
}

/// Tauri 命令：list_provider_models。
#[tauri::command]
pub async fn list_provider_models(id: String) -> Result<ProviderModelsResult, String> {
    let provider = find_provider(&id)?;
    validate_http_endpoint(&provider.endpoint)?;
    let api_key = require_api_key(&provider)?;
    // 刷新模型前确保 LiteLLM 上下文/能力表可用（失败不阻断，走回落）
    if let Err(err) = crate::litellm_meta::ensure_cache(false).await {
        tracing::warn!(error = %err, "LiteLLM 模型表不可用，将使用 API/回落");
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
                    Some(crate::model_meta::enrich_from_id(&id, kind, None))
                })
                .collect::<Vec<_>>();
            (models, "ollama:/api/tags".to_string())
        }
        ProviderKind::Anthropic => {
            let url = format!("{}/v1/models", trim_slash(&provider.endpoint));
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
                let msg = body["error"]["message"]
                    .as_str()
                    .or_else(|| body["error"].as_str())
                    .unwrap_or("未知错误");
                return Err(format!("Anthropic 列表失败 ({status}): {msg}"));
            }
            let models = body["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    let hints = crate::model_meta::ApiModelHints {
                        display_name: m["display_name"].as_str().map(str::to_string),
                        ..Default::default()
                    };
                    Some(crate::model_meta::enrich_from_id(&id, kind, Some(hints)))
                })
                .collect::<Vec<_>>();
            (models, "anthropic:/v1/models".to_string())
        }
        ProviderKind::Google => {
            // Gemini OpenAI 兼容基址：走 /models + Bearer；否则回退原生 ?key= 列表
            let base = trim_slash(&provider.endpoint);
            let (models, source) = if base.contains("/openai") || base.ends_with("/v1") {
                let url = format!("{}/models", openai_compatible_base(&provider.endpoint));
                let mut req = client.get(&url);
                if !api_key.is_empty() {
                    req = req.bearer_auth(&api_key);
                }
                let resp = req
                    .send()
                    .await
                    .map_err(|e| format!("连接 Google 失败: {e}"))?;
                let status = resp.status();
                let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
                if !status.is_success() {
                    let msg = body["error"]["message"]
                        .as_str()
                        .unwrap_or("未知错误");
                    return Err(format!("Google 列表失败 ({status}): {msg}"));
                }
                let models = body["data"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| {
                        let id = m["id"].as_str()?.to_string();
                        let hints = crate::model_meta::ApiModelHints {
                            display_name: m["display_name"].as_str().map(str::to_string),
                            ..Default::default()
                        };
                        Some(crate::model_meta::enrich_from_id(&id, kind, Some(hints)))
                    })
                    .collect::<Vec<_>>();
                (models, "google:openai/models".to_string())
            } else {
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
                    let msg = body["error"]["message"]
                        .as_str()
                        .unwrap_or("未知错误");
                    return Err(format!("Google 列表失败 ({status}): {msg}"));
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
                        let hints = crate::model_meta::ApiModelHints {
                            display_name: m["displayName"].as_str().map(str::to_string),
                            context_window: m["inputTokenLimit"].as_u64(),
                            max_output_tokens: m["outputTokenLimit"].as_u64(),
                            supported_methods: methods,
                        };
                        Some(crate::model_meta::enrich_from_id(&id, kind, Some(hints)))
                    })
                    .collect::<Vec<_>>();
                (models, "google:/v1beta/models".to_string())
            };
            (models, source)
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
                let msg = body["error"]["message"]
                    .as_str()
                    .or_else(|| body["error"].as_str())
                    .unwrap_or("未知错误");
                return Err(format!("模型列表失败 ({status}): {msg}"));
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
                    let hints = crate::model_meta::ApiModelHints {
                        display_name: m["name"].as_str().map(str::to_string),
                        context_window: ctx,
                        max_output_tokens: m["max_output_tokens"].as_u64(),
                        supported_methods: Vec::new(),
                    };
                    Some(crate::model_meta::enrich_from_id(
                        &id,
                        kind,
                        if ctx.is_some() { Some(hints) } else { None },
                    ))
                })
                .collect::<Vec<_>>();
            (models, format!("{}:/models", provider.kind.as_str()))
        }
        ProviderKind::Azure => {
            let base = azure_base(&provider.endpoint);
            let url = format!("{base}/openai/models?api-version={AZURE_API_VERSION}");
            let resp = client
                .get(&url)
                .header("api-key", &api_key)
                .send()
                .await
                .map_err(|e| format!("连接 Azure OpenAI 失败: {e}"))?;
            let status = resp.status();
            let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            if !status.is_success() {
                let msg = body["error"]["message"]
                    .as_str()
                    .or_else(|| body["error"].as_str())
                    .unwrap_or("未知错误");
                return Err(format!("Azure 模型列表失败 ({status}): {msg}"));
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
                    Some(crate::model_meta::enrich_from_id(&id, kind, None))
                })
                .collect::<Vec<_>>();
            (models, "azure:/openai/models".to_string())
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

/// Tauri 命令：get_cached_provider_models。
#[tauri::command]
pub async fn get_cached_provider_models(id: String) -> Result<Option<ProviderModelsResult>, String> {
    let cache = load_models_cache();
    Ok(cache.providers.get(&id).map(|c| {
        let kind = if c.kind.is_empty() { "custom" } else { c.kind.as_str() };
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
    let probe_id = match provider.kind {
        ProviderKind::Custom => "custom",
        _ => provider.kind.backend_id(),
    };
    let config = providers::ProviderConfig {
        api_key: api_key.to_string(),
        base_url: Some(provider.endpoint.clone()),
        model: model.clone(),
        temperature: 0.0,
        max_tokens: 1,
        thinking_enabled: false,
        reasoning_effort: "high".to_string(),
        additional_params: serde_json::Value::Null,
    };
    let result = providers::registry::ProviderRegistry::default()
        .verify(probe_id, &model, &config)
        .await;
    ProviderTestResult {
        ok: result.ok,
        latency_ms: result.latency_ms,
        model: result.model,
        message: result.message,
    }
}

/// Tauri 命令：test_provider。
#[tauri::command]
pub async fn test_provider(id: String, model: Option<String>) -> Result<ProviderTestResult, String> {
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
    fn parses_new_kinds_and_minimax_alias() {
        assert_eq!(ProviderKind::from_str("openrouter"), Some(ProviderKind::Openrouter));
        assert_eq!(ProviderKind::from_str("bailian"), Some(ProviderKind::Bailian));
        assert_eq!(ProviderKind::from_str("nvidia"), Some(ProviderKind::Nvidia));
        assert_eq!(ProviderKind::from_str("moonshot"), Some(ProviderKind::Moonshot));
        assert_eq!(ProviderKind::from_str("volcengine"), Some(ProviderKind::Volcengine));
        assert_eq!(ProviderKind::from_str("minimax"), Some(ProviderKind::Minimax));
        assert_eq!(ProviderKind::from_str("minmax"), Some(ProviderKind::Minimax));
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
            let p = s.providers.iter().find(|p| p.kind == kind).expect("missing kind");
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
            let p = s.providers.iter().find(|p| p.kind == kind).expect("missing kind");
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
        assert_eq!(ProviderKind::Bailian.default_model(), "qwen3.6-plus");
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
        assert_eq!(ProviderKind::Volcengine.default_model(), "ep-");
        assert_eq!(
            ProviderKind::Minimax.default_endpoint(),
            "https://api.minimax.io/v1"
        );
        assert_eq!(ProviderKind::Minimax.default_model(), "MiniMax-M2.5");
    }

    #[test]
    fn serde_accepts_minmax_alias() {
        let raw = r#"{"id":"x","kind":"minmax","display_name":"M","endpoint":"https://api.minimax.io/v1","model":"MiniMax-M2.5","enabled":false}"#;
        let p: ProviderConfig = serde_json::from_str(raw).expect("minmax alias");
        assert_eq!(p.kind, ProviderKind::Minimax);
        assert!(p.fallback.is_empty());
        let out = serde_json::to_value(&p).unwrap();
        assert_eq!(out["kind"], "minimax");
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
    fn validate_http_endpoint_rejects_non_http() {
        assert!(validate_http_endpoint("https://ok.example/v1").is_ok());
        assert!(validate_http_endpoint("http://localhost:11434").is_ok());
        assert!(validate_http_endpoint("file:///etc/passwd").is_err());
        assert!(validate_http_endpoint("").is_err());
        assert!(validate_http_endpoint("ftp://x").is_err());
    }

    #[test]
    fn migrate_stale_defaults_updates_known_old_values() {
        let mut s = ProvidersState {
            active_provider_id: None,
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
                },
            ],
        };
        assert!(s.migrate_stale_defaults());
        assert_eq!(s.providers[0].model, "kimi-k2.5");
        assert_eq!(s.providers[1].model, "ep-");
        assert_eq!(s.providers[2].endpoint, "https://api.minimax.io/v1");
        assert!(!s.migrate_stale_defaults());
    }
}
