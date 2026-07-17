//! LiteLLM `model_prices_and_context_window.json` 本地缓存与查找。
//!
//! 源：https://github.com/BerriAI/litellm — 刷新模型列表时按需拉取，落盘 `~/.astro/litellm-model-meta.json`。

use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

/// 缓存超过该时长则在下次刷新模型时重新拉取
const CACHE_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, Default)]
pub struct LiteLlmEntry {
    pub max_input_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub supports_vision: bool,
    pub supports_function_calling: bool,
    pub supports_reasoning: bool,
    pub supports_web_search: bool,
    pub supports_image_generation: bool,
    pub supports_video_generation: bool,
    pub supports_audio_output: bool,
    pub supported_output_modalities: Vec<String>,
    pub mode: Option<String>,
    pub litellm_provider: Option<String>,
    pub input_cost_per_token: Option<f64>,
    pub output_cost_per_token: Option<f64>,
    /// 命中的 LiteLLM 键名（调试 / 单测用）
    #[allow(dead_code)]
    pub matched_key: String,
}

#[derive(Debug, Deserialize)]
struct RawEntry {
    #[serde(default)]
    max_input_tokens: Option<u64>,
    #[serde(default)]
    max_output_tokens: Option<u64>,
    #[serde(default)]
    max_tokens: Option<u64>,
    #[serde(default)]
    supports_vision: Option<bool>,
    #[serde(default)]
    supports_function_calling: Option<bool>,
    #[serde(default)]
    supports_reasoning: Option<bool>,
    #[serde(default)]
    supports_web_search: Option<bool>,
    #[serde(default)]
    supports_image_generation: Option<bool>,
    #[serde(default)]
    supports_video_generation: Option<bool>,
    #[serde(default)]
    supports_audio_output: Option<bool>,
    #[serde(default)]
    supported_output_modalities: Vec<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    litellm_provider: Option<String>,
    #[serde(default)]
    input_cost_per_token: Option<f64>,
    #[serde(default)]
    output_cost_per_token: Option<f64>,
}

impl RawEntry {
    /// 将原始条目转为缓存 Entry。
    fn into_entry(self, key: &str) -> LiteLlmEntry {
        LiteLlmEntry {
            max_input_tokens: self.max_input_tokens,
            max_output_tokens: self.max_output_tokens.or(self.max_tokens),
            supports_vision: self.supports_vision.unwrap_or(false),
            supports_function_calling: self.supports_function_calling.unwrap_or(false),
            supports_reasoning: self.supports_reasoning.unwrap_or(false),
            supports_web_search: self.supports_web_search.unwrap_or(false),
            supports_image_generation: self.supports_image_generation.unwrap_or(false),
            supports_video_generation: self.supports_video_generation.unwrap_or(false),
            supports_audio_output: self.supports_audio_output.unwrap_or(false),
            supported_output_modalities: self.supported_output_modalities,
            mode: self.mode,
            litellm_provider: self.litellm_provider,
            input_cost_per_token: self.input_cost_per_token,
            output_cost_per_token: self.output_cost_per_token,
            matched_key: key.to_string(),
        }
    }
}

/// LiteLLM 元数据缓存文件路径。
fn cache_path() -> PathBuf {
    home::default_memory_dir().join("litellm-model-meta.json")
}

/// 获取进程内缓存 Map 锁。
fn map_lock() -> &'static RwLock<HashMap<String, LiteLlmEntry>> {
    static MAP: OnceLock<RwLock<HashMap<String, LiteLlmEntry>>> = OnceLock::new();
    MAP.get_or_init(|| RwLock::new(HashMap::new()))
}

/// 已尝试加载（含空 fixture）；避免测试清空后再次从磁盘灌回
fn map_ready() -> &'static AtomicBool {
    static READY: AtomicBool = AtomicBool::new(false);
    &READY
}

/// 标记内存缓存已就绪。
fn mark_ready() {
    map_ready().store(true, Ordering::SeqCst);
}

/// 内存缓存是否已加载。
fn memory_loaded() -> bool {
    map_lock().read().map(|g| !g.is_empty()).unwrap_or(false)
}

/// 解析磁盘 JSON 为内存 Map。
fn parse_map(value: serde_json::Value) -> HashMap<String, LiteLlmEntry> {
    let mut out = HashMap::new();
    let Some(obj) = value.as_object() else {
        return out;
    };
    for (key, raw) in obj {
        if key == "sample_spec" || !raw.is_object() {
            continue;
        }
        if let Ok(entry) = serde_json::from_value::<RawEntry>(raw.clone()) {
            let e = entry.into_entry(key);
            // 保留有上下文、能力标记、mode 或单价的条目（跳过无信息占位）
            let has_mode = e.mode.as_ref().is_some_and(|m| !m.trim().is_empty());
            if e.max_input_tokens.is_some()
                || e.supports_vision
                || e.supports_function_calling
                || e.supports_reasoning
                || e.supports_web_search
                || e.supports_image_generation
                || e.supports_video_generation
                || e.supports_audio_output
                || !e.supported_output_modalities.is_empty()
                || has_mode
                || e.input_cost_per_token.is_some()
                || e.output_cost_per_token.is_some()
            {
                out.insert(key.to_lowercase(), e);
            }
        }
    }
    out
}

/// 从磁盘灌入内存缓存。
fn load_disk_into_memory() -> bool {
    let path = cache_path();
    let Ok(bytes) = std::fs::read(&path) else {
        mark_ready();
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        mark_ready();
        return false;
    };
    let map = parse_map(value);
    if map.is_empty() {
        mark_ready();
        return false;
    }
    if let Ok(mut guard) = map_lock().write() {
        *guard = map;
        mark_ready();
        true
    } else {
        false
    }
}

/// 判断缓存是否仍在有效期内。
fn cache_is_fresh() -> bool {
    let path = cache_path();
    let Ok(meta) = std::fs::metadata(&path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    modified
        .elapsed()
        .map(|d| d < CACHE_MAX_AGE)
        .unwrap_or(false)
}

/// 确保内存中有 LiteLLM 表：优先磁盘，过期或缺失则联网拉取。
pub async fn ensure_cache(force: bool) -> Result<usize, String> {
    if !memory_loaded() {
        let _ = load_disk_into_memory();
    }
    if !force && memory_loaded() && cache_is_fresh() {
        return Ok(map_lock().read().map(|g| g.len()).unwrap_or(0));
    }

    match fetch_and_store().await {
        Ok(n) => Ok(n),
        Err(err) => {
            if memory_loaded() || load_disk_into_memory() {
                tracing::warn!(error = %err, "LiteLLM 刷新失败，使用本地缓存");
                Ok(map_lock().read().map(|g| g.len()).unwrap_or(0))
            } else {
                Err(err)
            }
        }
    }
}

/// 拉取远端模型元数据并写入缓存。
async fn fetch_and_store() -> Result<usize, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(LITELLM_URL)
        .send()
        .await
        .map_err(|e| format!("下载 LiteLLM 模型表失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("LiteLLM 模型表 HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("读取 LiteLLM 响应失败: {e}"))?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("解析 LiteLLM JSON 失败: {e}"))?;
    let map = parse_map(value.clone());
    if map.is_empty() {
        return Err("LiteLLM 模型表为空".into());
    }
    let dir = home::default_memory_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = cache_path();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    let n = map.len();
    if let Ok(mut guard) = map_lock().write() {
        *guard = map;
    }
    mark_ready();
    tracing::info!(count = n, path = %path.display(), "LiteLLM 模型表已更新");
    Ok(n)
}

/// 各供应商模型 id 前缀规则。
fn kind_prefixes(kind: &str) -> &'static [&'static str] {
    match kind {
        "openai" => &["", "openai/"],
        "deepseek" => &["", "deepseek/"],
        "google" => &["", "gemini/", "vertex_ai/", "vertex_ai-language-models/"],
        "anthropic" => &["", "anthropic/"],
        "azure" => &["", "azure/", "azure_ai/"],
        "zhipu" => &["", "zai/", "zhipu/"],
        "ollama" => &["", "ollama/"],
        "openrouter" => &["", "openrouter/"],
        "bailian" => &["", "dashscope/", "qwen/", "alibaba/"],
        "nvidia" => &["", "nvidia_nim/", "nvidia/"],
        "moonshot" => &["", "moonshot/"],
        "volcengine" => &["", "volcengine/", "doubao/"],
        "minimax" => &["", "minimax/"],
        _ => &[""],
    }
}

/// 去掉模型名上的日期后缀以便匹配。
fn strip_date_suffix(id: &str) -> String {
    let bytes = id.as_bytes();
    if bytes.len() > 9 && bytes[bytes.len() - 9] == b'-' {
        let suffix = &id[id.len() - 8..];
        if suffix.chars().all(|c| c.is_ascii_digit()) {
            return id[..id.len() - 9].to_string();
        }
    }
    id.to_string()
}

/// 判断模型条目是否属于某 Provider 类型。
fn provider_matches_kind(provider: &str, kind: &str) -> bool {
    let p = provider.to_lowercase();
    match kind {
        "openai" => p.contains("openai"),
        "deepseek" => p.contains("deepseek"),
        "google" => p.contains("gemini") || p.contains("vertex") || p.contains("google"),
        "anthropic" => p.contains("anthropic") || p.contains("claude"),
        "azure" => p.contains("azure"),
        "zhipu" => p.contains("zhipu") || p.contains("zai") || p.contains("glm"),
        "ollama" => p.contains("ollama"),
        "openrouter" => p.contains("openrouter"),
        "bailian" => {
            p.contains("dashscope")
                || p.contains("qwen")
                || p.contains("alibaba")
                || p.contains("bailian")
        }
        "nvidia" => p.contains("nvidia") || p.contains("nim"),
        "moonshot" => p.contains("moonshot") || p.contains("kimi"),
        "volcengine" => p.contains("volc") || p.contains("doubao") || p.contains("ark"),
        "minimax" => p.contains("minimax") || p.contains("minmax"),
        _ => true,
    }
}

/// 按模型 id + 提供商 kind 查找 LiteLLM 条目。
pub fn lookup(id: &str, kind: &str) -> Option<LiteLlmEntry> {
    if !map_ready().load(Ordering::SeqCst) {
        let _ = load_disk_into_memory();
    }
    let guard = map_lock().read().ok()?;
    if guard.is_empty() {
        return None;
    }

    let mut bare = id.trim().to_lowercase();
    if let Some(rest) = bare.strip_prefix("models/") {
        bare = rest.to_string();
    }
    let kind = kind.to_lowercase();
    let stripped = strip_date_suffix(&bare);

    let mut candidates: Vec<String> = Vec::new();
    for base in [&bare, &stripped] {
        for prefix in kind_prefixes(&kind) {
            let key = format!("{prefix}{base}");
            if !candidates.contains(&key) {
                candidates.push(key);
            }
        }
    }

    for key in &candidates {
        if let Some(entry) = guard.get(key) {
            return Some(entry.clone());
        }
    }

    // 后缀匹配：优先无斜杠的短键，且 provider 与 kind 相符
    let mut best: Option<&LiteLlmEntry> = None;
    let mut best_score: i32 = i32::MIN;
    for (key, entry) in guard.iter() {
        let hit = key == &bare
            || key == &stripped
            || key.ends_with(&format!("/{bare}"))
            || key.ends_with(&format!("/{stripped}"));
        if !hit {
            continue;
        }
        let mut score = 0i32;
        if !key.contains('/') {
            score += 100;
        }
        if key == &bare || key == &stripped {
            score += 50;
        }
        if let Some(p) = entry.litellm_provider.as_deref() {
            if provider_matches_kind(p, &kind) {
                score += 40;
            } else {
                score -= 20;
            }
        }
        // 惩罚网关/代理前缀
        if key.contains("gateway") || key.contains("openrouter") || key.contains("bedrock") {
            score -= 30;
        }
        score -= key.len() as i32 / 10;
        if score > best_score {
            best_score = score;
            best = Some(entry);
        }
    }
    best.cloned()
}

#[cfg(test)]
/// `load_fixture_json`。
pub fn load_fixture_json(json: &str) {
    let value: serde_json::Value = serde_json::from_str(json).expect("fixture json");
    let map = parse_map(value);
    *map_lock().write().unwrap() = map;
    mark_ready();
}

/// 测试用：串行加载 fixture，避免并行用例互相覆盖全局表
#[cfg(test)]
pub fn with_fixture<R>(json: &str, f: impl FnOnce() -> R) -> R {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    load_fixture_json(json);
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_prefers_bare_key() {
        with_fixture(
            r#"{
              "deepseek-chat": {
                "max_input_tokens": 131072,
                "max_output_tokens": 8192,
                "supports_function_calling": true,
                "mode": "chat",
                "litellm_provider": "deepseek"
              },
              "vercel_ai_gateway/deepseek/deepseek-chat": {
                "max_input_tokens": 64000,
                "supports_function_calling": true,
                "litellm_provider": "vercel_ai_gateway"
              }
            }"#,
            || {
                let e = lookup("deepseek-chat", "deepseek").unwrap();
                assert_eq!(e.max_input_tokens, Some(131072));
                assert!(e.supports_function_calling);
                assert_eq!(e.matched_key, "deepseek-chat");
            },
        );
    }
}
