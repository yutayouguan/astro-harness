//! OpenRouter 排行数据的双源读取、缓存与降级。
//!
//! 有 OpenRouter API Key 时，文档化 Data API 是首选；没有对应官方数据集
//! 或官方请求失败时，降级到 OpenRouter 公开页面使用的 frontend API。

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use super::providers::{find_provider_by_backend, resolve_api_key};

const OPENROUTER_ORIGIN: &str = "https://openrouter.ai";
const OFFICIAL_CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const FRONTEND_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RankingSource {
    Official,
    Frontend,
}

impl RankingSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Frontend => "frontend",
        }
    }

    fn ttl(self) -> Duration {
        match self {
            Self::Official => OFFICIAL_CACHE_TTL,
            Self::Frontend => FRONTEND_CACHE_TTL,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedRankingResponse {
    fetched_at: DateTime<Utc>,
    data_source: String,
    #[serde(default)]
    as_of: Option<String>,
    payload: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterRankingsResponse {
    pub dataset: String,
    pub modality: Option<String>,
    pub data_source: String,
    pub freshness: String,
    pub cache_hit: bool,
    pub used_fallback: bool,
    pub fetched_at: String,
    pub as_of: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
struct RequestSpec {
    dataset: String,
    modality: Option<String>,
}

impl RequestSpec {
    fn parse(dataset: &str, modality: Option<&str>) -> Result<Self, String> {
        let dataset = dataset.trim().to_ascii_lowercase();
        let modality = modality.map(|value| value.trim().to_ascii_lowercase());
        match dataset.as_str() {
            "text" | "tools" | "performance" | "tasks" | "benchmarks" | "apps" | "session_cost"
            | "batch" => {
                if modality.is_some() {
                    return Err(format!("{dataset} 不接受 modality 参数"));
                }
            }
            "modality" => {
                let Some(value) = modality.as_deref() else {
                    return Err("modality 数据集缺少 modality 参数".into());
                };
                if !matches!(
                    value,
                    "image" | "embeddings" | "rerank" | "video" | "speech" | "transcription"
                ) {
                    return Err(format!("不支持的 OpenRouter 排行模态: {value}"));
                }
            }
            _ => return Err(format!("不支持的 OpenRouter 排行数据集: {dataset}")),
        }
        Ok(Self { dataset, modality })
    }

    fn cache_stem(&self, source: RankingSource) -> String {
        let suffix = self
            .modality
            .as_deref()
            .map(|value| format!("-{value}"))
            .unwrap_or_default();
        format!(
            "openrouter-rankings-{}{}-{}",
            self.dataset,
            suffix,
            source.as_str()
        )
    }
}

fn request_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn cache_path(spec: &RequestSpec, source: RankingSource) -> PathBuf {
    let dir = home::default_memory_dir().join("cache");
    let _ = fs::create_dir_all(&dir);
    dir.join(format!("{}.json", spec.cache_stem(source)))
}

fn load_cached(spec: &RequestSpec, source: RankingSource) -> Option<CachedRankingResponse> {
    let bytes = fs::read(cache_path(spec, source)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn is_fresh(cached: &CachedRankingResponse, source: RankingSource) -> bool {
    Utc::now()
        .signed_duration_since(cached.fetched_at)
        .to_std()
        .map(|age| age <= source.ttl())
        .unwrap_or(false)
}

fn save_cached(
    spec: &RequestSpec,
    source: RankingSource,
    payload: Value,
) -> Result<CachedRankingResponse, String> {
    let cached = CachedRankingResponse {
        fetched_at: Utc::now(),
        data_source: source.as_str().to_string(),
        as_of: extract_as_of(&payload),
        payload,
    };
    let path = cache_path(spec, source);
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(&cached).map_err(|error| error.to_string())?;
    fs::write(&tmp, bytes).map_err(|error| error.to_string())?;
    fs::rename(&tmp, &path).map_err(|error| error.to_string())?;
    Ok(cached)
}

fn into_response(
    spec: &RequestSpec,
    cached: CachedRankingResponse,
    freshness: &str,
    cache_hit: bool,
    used_fallback: bool,
) -> OpenRouterRankingsResponse {
    OpenRouterRankingsResponse {
        dataset: spec.dataset.clone(),
        modality: spec.modality.clone(),
        data_source: cached.data_source,
        freshness: freshness.to_string(),
        cache_hit,
        used_fallback,
        fetched_at: cached.fetched_at.to_rfc3339(),
        as_of: cached.as_of,
        payload: cached.payload,
    }
}

fn openrouter_api_key() -> Option<String> {
    let provider = find_provider_by_backend("openrouter").ok()?;
    let (available, _, _, key) = resolve_api_key(&provider);
    available
        .then_some(key)
        .flatten()
        .filter(|key| !key.trim().is_empty())
}

fn official_url(spec: &RequestSpec) -> Option<reqwest::Url> {
    let mut url = match spec.dataset.as_str() {
        "text" => reqwest::Url::parse(&format!(
            "{OPENROUTER_ORIGIN}/api/v1/datasets/rankings-daily"
        ))
        .ok()?,
        "apps" => reqwest::Url::parse(&format!(
            "{OPENROUTER_ORIGIN}/api/v1/datasets/app-rankings"
        ))
        .ok()?,
        "benchmarks" => {
            reqwest::Url::parse(&format!("{OPENROUTER_ORIGIN}/api/v1/benchmarks")).ok()?
        }
        _ => return None,
    };

    match spec.dataset.as_str() {
        "text" | "apps" => {
            let end = Utc::now().date_naive() - ChronoDuration::days(1);
            let start = end - ChronoDuration::days(6);
            url.query_pairs_mut()
                .append_pair("start_date", &start.format("%Y-%m-%d").to_string())
                .append_pair("end_date", &end.format("%Y-%m-%d").to_string());
            if spec.dataset == "apps" {
                url.query_pairs_mut()
                    .append_pair("sort", "popular")
                    .append_pair("limit", "50");
            }
        }
        "benchmarks" => {
            url.query_pairs_mut()
                .append_pair("source", "artificial-analysis")
                .append_pair("max_results", "100");
        }
        _ => {}
    }
    Some(url)
}

fn frontend_url(spec: &RequestSpec) -> Result<reqwest::Url, String> {
    let path = match spec.dataset.as_str() {
        "text" | "batch" => "models",
        "tools" => "tools",
        "performance" => "performance",
        "tasks" => "task-spend",
        "benchmarks" => "benchmarks",
        "apps" => "apps",
        "session_cost" => "session-cost",
        "modality" => "modality-chart",
        _ => return Err(format!("无法解析 frontend 数据集: {}", spec.dataset)),
    };
    let mut url = reqwest::Url::parse(&format!(
        "{OPENROUTER_ORIGIN}/api/frontend/v1/rankings/{path}"
    ))
    .map_err(|error| error.to_string())?;
    match spec.dataset.as_str() {
        "text" => {
            url.query_pairs_mut()
                .append_pair("modality", "text")
                .append_pair("view", "week");
        }
        "batch" => {
            url.query_pairs_mut()
                .append_pair("traffic", "batch")
                .append_pair("modality", "all")
                .append_pair("metric", "requests")
                .append_pair("view", "week");
        }
        "modality" => {
            url.query_pairs_mut()
                .append_pair("routeSegment", spec.modality.as_deref().unwrap_or("text"));
        }
        _ => {}
    }
    Ok(url)
}

async fn fetch_json(url: reqwest::Url, api_key: Option<&str>) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client
        .get(url.clone())
        .header("Accept", "application/json")
        .header("HTTP-Referer", "https://github.com/astro-agent")
        .header("X-Title", "Astro Agent");
    if let Some(key) = api_key {
        request = request.bearer_auth(key);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("OpenRouter 排行数据请求失败: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "OpenRouter 排行数据 HTTP {} ({})",
            response.status(),
            url.path()
        ));
    }
    response
        .json::<Value>()
        .await
        .map_err(|error| format!("OpenRouter 排行数据解析失败: {error}"))
}

fn extract_as_of(payload: &Value) -> Option<String> {
    for pointer in [
        "/meta/as_of",
        "/data/as_of",
        "/data/windowEnd",
        "/data/generatedAt",
    ] {
        if let Some(value) = payload.pointer(pointer).and_then(Value::as_str) {
            return Some(value.to_string());
        }
    }
    for pointer in ["/data/cachedAt", "/cachedAt"] {
        if let Some(value) = payload.pointer(pointer).and_then(Value::as_i64) {
            if let Some(timestamp) = DateTime::<Utc>::from_timestamp_millis(value) {
                return Some(timestamp.to_rfc3339());
            }
        }
    }

    let rows = payload.get("data").and_then(Value::as_array)?;
    rows.iter()
        .filter_map(|row| {
            row.get("date")
                .or_else(|| row.get("x"))
                .and_then(Value::as_str)
        })
        .max()
        .map(str::to_string)
}

async fn fetch_and_cache(
    spec: &RequestSpec,
    source: RankingSource,
    url: reqwest::Url,
    api_key: Option<&str>,
) -> Result<CachedRankingResponse, String> {
    let payload = fetch_json(url, api_key).await?;
    save_cached(spec, source, payload)
}

/// 返回 OpenRouter 排行数据。官方 Data API 优先，frontend API 补位。
#[tauri::command]
pub async fn get_openrouter_rankings(
    dataset: String,
    modality: Option<String>,
    force_refresh: bool,
) -> Result<OpenRouterRankingsResponse, String> {
    let spec = RequestSpec::parse(&dataset, modality.as_deref())?;
    let _guard = request_lock().lock().await;
    let api_key = openrouter_api_key();
    let has_official = api_key.is_some() && official_url(&spec).is_some();

    let mut errors = Vec::new();
    if let (Some(key), Some(url)) = (api_key.as_deref(), official_url(&spec)) {
        if !force_refresh {
            if let Some(cached) = load_cached(&spec, RankingSource::Official)
                .filter(|item| is_fresh(item, RankingSource::Official))
            {
                return Ok(into_response(&spec, cached, "fresh", true, false));
            }
        }
        match fetch_and_cache(&spec, RankingSource::Official, url, Some(key)).await {
            Ok(cached) => {
                return Ok(into_response(&spec, cached, "fresh", false, false));
            }
            Err(error) => errors.push(error),
        }
    }

    if !force_refresh {
        if let Some(cached) = load_cached(&spec, RankingSource::Frontend)
            .filter(|item| is_fresh(item, RankingSource::Frontend))
        {
            return Ok(into_response(&spec, cached, "fresh", true, has_official));
        }
    }

    match frontend_url(&spec) {
        Ok(url) => match fetch_and_cache(&spec, RankingSource::Frontend, url, None).await {
            Ok(cached) => {
                return Ok(into_response(&spec, cached, "fresh", false, has_official));
            }
            Err(error) => errors.push(error),
        },
        Err(error) => errors.push(error),
    }

    let stale_sources = if has_official {
        vec![RankingSource::Official, RankingSource::Frontend]
    } else {
        vec![RankingSource::Frontend]
    };
    for source in stale_sources {
        if let Some(cached) = load_cached(&spec, source) {
            return Ok(into_response(
                &spec,
                cached,
                "stale",
                true,
                has_official && source == RankingSource::Frontend,
            ));
        }
    }

    Err(errors.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_modality_names() {
        assert!(RequestSpec::parse("modality", Some("embeddings")).is_ok());
        assert!(RequestSpec::parse("modality", Some("music")).is_err());
        assert!(RequestSpec::parse("text", Some("image")).is_err());
    }

    #[test]
    fn maps_official_and_frontend_endpoints() {
        let text = RequestSpec::parse("text", None).unwrap();
        assert!(official_url(&text)
            .unwrap()
            .path()
            .ends_with("/datasets/rankings-daily"));
        assert_eq!(
            frontend_url(&text)
                .unwrap()
                .query_pairs()
                .find(|(key, _)| key == "view")
                .map(|(_, value)| value.into_owned()),
            Some("week".into())
        );
        assert_eq!(
            frontend_url(&text)
                .unwrap()
                .query_pairs()
                .find(|(key, _)| key == "modality")
                .map(|(_, value)| value.into_owned()),
            Some("text".into())
        );

        let tools = RequestSpec::parse("tools", None).unwrap();
        assert!(official_url(&tools).is_none());
        assert!(frontend_url(&tools)
            .unwrap()
            .path()
            .ends_with("/rankings/tools"));

        let tasks = RequestSpec::parse("tasks", None).unwrap();
        assert!(official_url(&tasks).is_none());
        assert!(frontend_url(&tasks)
            .unwrap()
            .path()
            .ends_with("/rankings/task-spend"));

    }

    #[test]
    fn extracts_upstream_freshness_metadata() {
        let official = serde_json::json!({"meta": {"as_of": "2026-09-03T00:00:00Z"}});
        assert_eq!(
            extract_as_of(&official).as_deref(),
            Some("2026-09-03T00:00:00Z")
        );

        let frontend = serde_json::json!({"data": [{"x": "2026-08-25"}, {"x": "2026-09-01"}]});
        assert_eq!(extract_as_of(&frontend).as_deref(), Some("2026-09-01"));
    }
}
