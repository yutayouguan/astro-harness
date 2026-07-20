//! OpenRouter Models API 本地缓存与查找。
//!
//! 源：`GET https://openrouter.ai/api/v1/models?output_modalities=all`
//! 刷新模型列表时按需拉取，落盘 `~/.astro/openrouter-model-meta.json`。

use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

const OPENROUTER_MODELS_URL: &str =
    "https://openrouter.ai/api/v1/models?output_modalities=all";

/// 缓存超过该时长则在下次刷新模型时重新拉取
const CACHE_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// 归一化后的模型能力条目（供 enrich 使用）。
#[derive(Debug, Clone, Default)]
pub struct OpenRouterEntry {
    pub max_input_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub supports_vision: bool,
    pub supports_function_calling: bool,
    pub supports_reasoning: bool,
    pub supports_web_search: bool,
    pub supports_image_generation: bool,
    pub supports_video_generation: bool,
    pub supports_audio_output: bool,
    pub supports_music_generation: bool,
    pub display_name: Option<String>,
    /// 命中的 OpenRouter 模型 id（调试 / 单测用）
    #[allow(dead_code)]
    pub matched_key: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<RawModel>,
}

#[derive(Debug, Deserialize)]
struct RawModel {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    architecture: Option<RawArchitecture>,
    #[serde(default)]
    top_provider: Option<RawTopProvider>,
    #[serde(default)]
    supported_parameters: Vec<String>,
    /// 非空对象即表示支持推理（含 mandatory / effort 等）
    #[serde(default)]
    reasoning: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
struct RawArchitecture {
    #[serde(default)]
    input_modalities: Vec<String>,
    #[serde(default)]
    output_modalities: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawTopProvider {
    #[serde(default)]
    max_completion_tokens: Option<u64>,
}

impl RawModel {
    fn into_entry(self) -> OpenRouterEntry {
        let id_lower = self.id.to_lowercase();
        let name_lower = self.name.as_deref().unwrap_or("").to_lowercase();
        let arch = self.architecture.unwrap_or_default();
        let inns: Vec<String> = arch
            .input_modalities
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        let outs: Vec<String> = arch
            .output_modalities
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        let params: Vec<String> = self
            .supported_parameters
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        let has_mod = |list: &[String], needle: &str| list.iter().any(|m| m == needle);
        let has_param = |needle: &str| params.iter().any(|p| p == needle);

        let image_out = has_mod(&outs, "image");
        let video_out = has_mod(&outs, "video");
        let audio_out = has_mod(&outs, "audio");
        let music_hint = id_lower.contains("lyria")
            || id_lower.contains("music")
            || name_lower.contains("lyria")
            || name_lower.contains("music");
        let supports_music = audio_out && music_hint;

        let reasoning_obj = match self.reasoning.as_ref() {
            Some(v) if v.is_null() => false,
            Some(v) if v.is_object() => v.as_object().is_some_and(|o| !o.is_empty()),
            Some(_) => true,
            None => false,
        };
        let supports_reasoning = reasoning_obj
            || has_param("reasoning")
            || has_param("include_reasoning")
            || has_param("reasoning_effort");

        OpenRouterEntry {
            max_input_tokens: self.context_length,
            max_output_tokens: self
                .top_provider
                .and_then(|t| t.max_completion_tokens),
            supports_vision: has_mod(&inns, "image") || has_mod(&inns, "video"),
            supports_function_calling: has_param("tools") || has_param("tool_choice"),
            supports_reasoning,
            supports_web_search: has_param("web_search_options"),
            supports_image_generation: image_out,
            supports_video_generation: video_out,
            supports_audio_output: audio_out && !supports_music,
            supports_music_generation: supports_music,
            display_name: self.name,
            matched_key: self.id,
        }
    }
}

/// OpenRouter 元数据缓存文件路径。
fn cache_path() -> PathBuf {
    home::default_memory_dir().join("openrouter-model-meta.json")
}

/// 获取进程内缓存 Map 锁。
fn map_lock() -> &'static RwLock<HashMap<String, OpenRouterEntry>> {
    static MAP: OnceLock<RwLock<HashMap<String, OpenRouterEntry>>> = OnceLock::new();
    MAP.get_or_init(|| RwLock::new(HashMap::new()))
}

/// 已尝试加载（含空 fixture）；避免测试清空后再次从磁盘灌回。
fn map_ready() -> &'static AtomicBool {
    static READY: AtomicBool = AtomicBool::new(false);
    &READY
}

fn mark_ready() {
    map_ready().store(true, Ordering::SeqCst);
}

fn memory_loaded() -> bool {
    map_lock().read().map(|g| !g.is_empty()).unwrap_or(false)
}

/// 解析 API / 磁盘 JSON 为内存 Map（key = 小写 id）。
fn parse_map(value: serde_json::Value) -> HashMap<String, OpenRouterEntry> {
    let mut out = HashMap::new();
    let Ok(resp) = serde_json::from_value::<ModelsResponse>(value) else {
        return out;
    };
    for raw in resp.data {
        if raw.id.trim().is_empty() {
            continue;
        }
        let key = raw.id.to_lowercase();
        let entry = raw.into_entry();
        out.insert(key, entry);
    }
    out
}

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

/// 确保内存中有 OpenRouter 表：优先磁盘，过期或缺失则联网拉取。
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
                tracing::warn!(error = %err, "OpenRouter 模型表刷新失败，使用本地缓存");
                Ok(map_lock().read().map(|g| g.len()).unwrap_or(0))
            } else {
                Err(err)
            }
        }
    }
}

async fn fetch_and_store() -> Result<usize, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(OPENROUTER_MODELS_URL)
        .header("HTTP-Referer", "https://github.com/astro-agent")
        .header("X-Title", "Astro Agent")
        .send()
        .await
        .map_err(|e| format!("下载 OpenRouter 模型表失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("OpenRouter 模型表 HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("读取 OpenRouter 响应失败: {e}"))?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("解析 OpenRouter JSON 失败: {e}"))?;
    let map = parse_map(value.clone());
    if map.is_empty() {
        return Err("OpenRouter 模型表为空".into());
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
    tracing::info!(count = n, path = %path.display(), "OpenRouter 模型表已更新");
    Ok(n)
}

/// 各供应商在 OpenRouter 上的常见 author 前缀。
fn kind_authors(kind: &str) -> &'static [&'static str] {
    match kind {
        "openai" => &["openai"],
        "deepseek" => &["deepseek"],
        "google" => &["google"],
        "anthropic" => &["anthropic"],
        "azure" => &["openai", "azure"],
        "zhipu" => &["z-ai", "zhipu", "thudm"],
        "ollama" => &["ollama"],
        "openrouter" => &["openrouter"],
        "bailian" => &["qwen", "alibaba"],
        "nvidia" => &["nvidia"],
        "moonshot" => &["moonshotai", "moonshot"],
        "volcengine" => &["bytedance", "doubao"],
        "minimax" => &["minimax"],
        "mistral" => &["mistralai"],
        "xai" | "x-ai" => &["x-ai"],
        "meta" => &["meta-llama", "meta"],
        _ => &[],
    }
}

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

/// 去掉 `:free` / `:nitro` 等路由后缀。
fn strip_variant_suffix(id: &str) -> &str {
    id.split(':').next().unwrap_or(id)
}

/// 按模型 id + 提供商 kind 查找 OpenRouter 条目。
pub fn lookup(id: &str, kind: &str) -> Option<OpenRouterEntry> {
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
    let bare = strip_variant_suffix(&bare).to_string();
    let kind = kind.to_lowercase();
    let stripped = strip_date_suffix(&bare);
    let slug = bare
        .rsplit('/')
        .next()
        .unwrap_or(&bare)
        .to_string();
    let slug_stripped = strip_date_suffix(&slug);

    let mut candidates: Vec<String> = Vec::new();
    let push = |cands: &mut Vec<String>, key: String| {
        if !cands.contains(&key) {
            cands.push(key);
        }
    };
    push(&mut candidates, bare.clone());
    push(&mut candidates, stripped.clone());
    for author in kind_authors(&kind) {
        push(&mut candidates, format!("{author}/{slug}"));
        push(&mut candidates, format!("{author}/{slug_stripped}"));
        push(&mut candidates, format!("{author}/{bare}"));
        push(&mut candidates, format!("{author}/{stripped}"));
    }

    for key in &candidates {
        if let Some(entry) = guard.get(key) {
            return Some(entry.clone());
        }
    }

    // 后缀匹配：`author/slug` 的 slug 部分
    let mut best: Option<&OpenRouterEntry> = None;
    let mut best_score: i32 = i32::MIN;
    let authors = kind_authors(&kind);
    for (key, entry) in guard.iter() {
        let key_slug = key.rsplit('/').next().unwrap_or(key);
        let key_slug = strip_variant_suffix(key_slug);
        let hit = key == &bare
            || key == &stripped
            || key_slug == slug
            || key_slug == slug_stripped
            || key.ends_with(&format!("/{bare}"))
            || key.ends_with(&format!("/{stripped}"))
            || key.ends_with(&format!("/{slug}"))
            || key.ends_with(&format!("/{slug_stripped}"));
        if !hit {
            continue;
        }
        let mut score = 0i32;
        if key == &bare || key == &stripped {
            score += 80;
        }
        if let Some((author, _)) = key.split_once('/') {
            if authors.iter().any(|a| *a == author) {
                score += 50;
            } else if !authors.is_empty() {
                score -= 15;
            }
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
pub fn load_fixture_json(json: &str) {
    let value: serde_json::Value = serde_json::from_str(json).expect("fixture json");
    let map = parse_map(value);
    *map_lock().write().unwrap() = map;
    mark_ready();
}

/// 测试用：串行加载 fixture，避免并行用例互相覆盖全局表。
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
    fn lookup_prefers_author_slug() {
        with_fixture(
            r#"{
              "data": [
                {
                  "id": "deepseek/deepseek-chat",
                  "name": "DeepSeek Chat",
                  "context_length": 131072,
                  "architecture": {
                    "input_modalities": ["text"],
                    "output_modalities": ["text"]
                  },
                  "supported_parameters": ["tools", "tool_choice"],
                  "reasoning": null
                },
                {
                  "id": "openrouter/deepseek-chat",
                  "name": "OR DeepSeek",
                  "context_length": 64000,
                  "architecture": {
                    "input_modalities": ["text"],
                    "output_modalities": ["text"]
                  },
                  "supported_parameters": ["tools"],
                  "reasoning": null
                }
              ]
            }"#,
            || {
                let e = lookup("deepseek-chat", "deepseek").unwrap();
                assert_eq!(e.max_input_tokens, Some(131072));
                assert!(e.supports_function_calling);
                assert_eq!(e.matched_key, "deepseek/deepseek-chat");
            },
        );
    }

    #[test]
    fn maps_reasoning_and_vision() {
        with_fixture(
            r#"{
              "data": [{
                "id": "deepseek/deepseek-v4-flash",
                "name": "DeepSeek V4 Flash",
                "context_length": 1048576,
                "architecture": {
                  "input_modalities": ["text"],
                  "output_modalities": ["text"]
                },
                "supported_parameters": ["tools", "reasoning", "reasoning_effort"],
                "reasoning": { "mandatory": false, "default_effort": "high" },
                "top_provider": { "max_completion_tokens": 8192 }
              }]
            }"#,
            || {
                let e = lookup("deepseek-v4-flash", "deepseek").unwrap();
                assert!(e.supports_reasoning);
                assert!(e.supports_function_calling);
                assert!(!e.supports_vision);
                assert_eq!(e.max_input_tokens, Some(1_048_576));
                assert_eq!(e.max_output_tokens, Some(8192));
            },
        );
    }
}
