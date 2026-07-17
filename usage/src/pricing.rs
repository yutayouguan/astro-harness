//! 路由感知用量费用估算：官方价快照 + 兼容端 models API（Task 3）+ 订阅 included。
//!
//! 费用路径 **禁止** 读取 `litellm-model-meta.json`。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use home::default_memory_dir;

const OPENROUTER_PRICING_CACHE_FILE: &str = "openrouter-model-pricing.json";
const PRICING_CACHE_MAX_AGE: Duration = Duration::hours(24);

/// 四桶 token 用量（memory 侧类型，避免 memory→providers 依赖）。
#[derive(Debug, Clone, Copy, Default)]
pub struct UsageTokens {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_write_tokens: u32,
    pub request_count: u32,
}

/// 费用估算状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostStatus {
    Estimated,
    Included,
    Unknown,
}

/// 费用估算结果。
#[derive(Debug, Clone)]
pub struct CostResult {
    pub amount_usd: Option<f64>,
    pub status: CostStatus,
    /// e.g. `official_docs_snapshot` | `provider_models_api` | `none`
    pub source: String,
    pub pricing_version: Option<String>,
    pub label: String,
}

/// 计费路由判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BillingRoute {
    Included,
    OfficialSnapshot { provider: String },
    ProviderModelsApi { provider: String, base_url: String },
    Unknown,
}

#[derive(Debug, Clone, Copy)]
struct PricingEntry {
    input_per_million: f64,
    output_per_million: f64,
    cache_read_per_million: Option<f64>,
    cache_write_per_million: Option<f64>,
    request_fee: Option<f64>,
    pricing_version: &'static str,
}

/// 官方文档价快照（per-million USD）。覆盖项目常用模型，可增量扩充。
const OFFICIAL_DOCS_PRICING: &[(&str, &str, PricingEntry)] = &[
    (
        "openai",
        "gpt-4o-mini",
        PricingEntry {
            input_per_million: 0.15,
            output_per_million: 0.60,
            cache_read_per_million: None,
            cache_write_per_million: None,
            request_fee: None,
            pricing_version: "openai-2025-07",
        },
    ),
    (
        "anthropic",
        "claude-3-5-haiku-20241022",
        PricingEntry {
            input_per_million: 0.80,
            output_per_million: 4.00,
            cache_read_per_million: Some(0.08),
            cache_write_per_million: Some(1.00),
            request_fee: None,
            pricing_version: "anthropic-2025-07",
        },
    ),
    (
        "anthropic",
        "claude-3-5-sonnet-20241022",
        PricingEntry {
            input_per_million: 3.00,
            output_per_million: 15.00,
            cache_read_per_million: Some(0.30),
            cache_write_per_million: Some(3.75),
            request_fee: None,
            pricing_version: "anthropic-2025-07",
        },
    ),
];

fn is_local_host(base_url: &str) -> bool {
    let lower = base_url.to_ascii_lowercase();
    lower.contains("localhost") || lower.contains("127.0.0.1") || lower.contains("[::1]")
}

fn is_openrouter_host(base_url: &str) -> bool {
    base_url.to_ascii_lowercase().contains("openrouter.ai")
}

fn is_official_snapshot_provider(provider: &str) -> bool {
    matches!(
        provider,
        "openai" | "anthropic" | "google" | "gemini" | "bedrock" | "minimax"
    )
}

fn infer_provider_from_base_url(base_url: &str) -> Option<String> {
    let lower = base_url.to_ascii_lowercase();
    if lower.contains("api.openai.com") || lower.contains("openai.azure.com") {
        return Some("openai".into());
    }
    if lower.contains("api.anthropic.com") {
        return Some("anthropic".into());
    }
    if lower.contains("generativelanguage.googleapis.com") {
        return Some("google".into());
    }
    None
}

fn normalize_model_key(model: &str) -> String {
    let model = model.trim().to_ascii_lowercase();
    model
        .rsplit_once('/')
        .map(|(_, tail)| tail.to_string())
        .unwrap_or(model)
}

fn lookup_official_snapshot(provider: &str, model: &str) -> Option<&'static PricingEntry> {
    let provider = provider.trim().to_ascii_lowercase();
    let model_key = normalize_model_key(model);
    OFFICIAL_DOCS_PRICING
        .iter()
        .find(|(p, m, _)| *p == provider && *m == model_key)
        .map(|(_, _, entry)| entry)
}

fn compute_cost(usage: &UsageTokens, entry: &PricingEntry) -> Result<f64, ()> {
    let mut cost = 0.0;
    if usage.input_tokens > 0 {
        cost += f64::from(usage.input_tokens) * entry.input_per_million / 1_000_000.0;
    }
    if usage.output_tokens > 0 {
        cost += f64::from(usage.output_tokens) * entry.output_per_million / 1_000_000.0;
    }
    if usage.cache_read_tokens > 0 {
        let rate = entry.cache_read_per_million.ok_or(())?;
        cost += f64::from(usage.cache_read_tokens) * rate / 1_000_000.0;
    }
    if usage.cache_write_tokens > 0 {
        let rate = entry.cache_write_per_million.ok_or(())?;
        cost += f64::from(usage.cache_write_tokens) * rate / 1_000_000.0;
    }
    if usage.request_count > 0 {
        if let Some(fee) = entry.request_fee {
            cost += f64::from(usage.request_count) * fee;
        }
    }
    Ok(cost)
}

fn format_label(status: CostStatus, amount: Option<f64>) -> String {
    match status {
        CostStatus::Included => "included".to_string(),
        CostStatus::Unknown => "n/a".to_string(),
        CostStatus::Estimated => {
            let amount = amount.unwrap_or(0.0);
            format!("~${amount:.2}")
        }
    }
}

fn unknown_result() -> CostResult {
    CostResult {
        amount_usd: None,
        status: CostStatus::Unknown,
        source: "none".to_string(),
        pricing_version: None,
        label: format_label(CostStatus::Unknown, None),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedModelPricing {
    prompt: f64,
    completion: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache_read: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache_write: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PricingCacheFile {
    fetched_at: String,
    models: HashMap<String, CachedModelPricing>,
}

fn openrouter_pricing_cache_path() -> PathBuf {
    default_memory_dir().join(OPENROUTER_PRICING_CACHE_FILE)
}

fn is_cache_fetched_at_valid(fetched_at: DateTime<Utc>) -> bool {
    let now = Utc::now();
    if fetched_at > now {
        return true;
    }
    now - fetched_at < PRICING_CACHE_MAX_AGE
}

fn parse_price_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn cached_model_to_entry(pricing: &CachedModelPricing) -> PricingEntry {
    PricingEntry {
        input_per_million: pricing.prompt * 1_000_000.0,
        output_per_million: pricing.completion * 1_000_000.0,
        cache_read_per_million: pricing.cache_read.map(|rate| rate * 1_000_000.0),
        cache_write_per_million: pricing.cache_write.map(|rate| rate * 1_000_000.0),
        request_fee: pricing.request,
        pricing_version: "provider_models_api",
    }
}

fn lookup_model_in_cache<'a>(
    models: &'a HashMap<String, CachedModelPricing>,
    model: &str,
) -> Option<&'a CachedModelPricing> {
    let model = model.trim();
    if let Some(pricing) = models.get(model) {
        return Some(pricing);
    }
    let lower = model.to_ascii_lowercase();
    if let Some(pricing) = models.get(&lower) {
        return Some(pricing);
    }
    let norm = normalize_model_key(model);
    models
        .iter()
        .find(|(id, _)| normalize_model_key(id) == norm)
        .map(|(_, pricing)| pricing)
}

fn read_pricing_cache_file() -> Option<PricingCacheFile> {
    let path = openrouter_pricing_cache_path();
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn load_valid_pricing_cache() -> Option<PricingCacheFile> {
    let cache = read_pricing_cache_file()?;
    let fetched_at = DateTime::parse_from_rfc3339(&cache.fetched_at)
        .ok()?
        .with_timezone(&Utc);
    if !is_cache_fetched_at_valid(fetched_at) {
        return None;
    }
    Some(cache)
}

fn write_pricing_cache(cache: &PricingCacheFile) {
    let path = openrouter_pricing_cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cache) {
        let _ = fs::write(path, json);
    }
}

fn parse_models_api_response(json: Value) -> Option<PricingCacheFile> {
    let data = json.get("data")?.as_array()?;
    let mut models = HashMap::new();
    for item in data {
        let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(pricing) = item.get("pricing") else {
            continue;
        };
        let Some(prompt) = pricing.get("prompt").and_then(parse_price_value) else {
            continue;
        };
        let Some(completion) = pricing.get("completion").and_then(parse_price_value) else {
            continue;
        };
        models.insert(
            id.to_string(),
            CachedModelPricing {
                prompt,
                completion,
                cache_read: pricing.get("cache_read").and_then(parse_price_value),
                cache_write: pricing.get("cache_write").and_then(parse_price_value),
                request: pricing.get("request").and_then(parse_price_value),
            },
        );
    }
    if models.is_empty() {
        return None;
    }
    Some(PricingCacheFile {
        fetched_at: Utc::now().to_rfc3339(),
        models,
    })
}

fn fetch_models_pricing_cache(base_url: &str, api_key: &str) -> Option<PricingCacheFile> {
    let base = base_url.trim().trim_end_matches('/');
    let url = format!("{base}/models");
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .ok()?;
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let json: Value = response.json().ok()?;
    parse_models_api_response(json)
}

fn estimate_from_provider_models_api(
    model: &str,
    usage: &UsageTokens,
    base_url: &str,
    api_key: Option<&str>,
) -> CostResult {
    if let Some(cache) = load_valid_pricing_cache() {
        if let Some(pricing) = lookup_model_in_cache(&cache.models, model) {
            return cost_from_cached_pricing(usage, pricing, &cache.fetched_at);
        }
    }

    if let Some(key) = api_key.map(str::trim).filter(|k| !k.is_empty()) {
        if let Some(cache) = fetch_models_pricing_cache(base_url, key) {
            write_pricing_cache(&cache);
            if let Some(pricing) = lookup_model_in_cache(&cache.models, model) {
                return cost_from_cached_pricing(usage, pricing, &cache.fetched_at);
            }
        }
    }

    unknown_result()
}

fn cost_from_cached_pricing(
    usage: &UsageTokens,
    pricing: &CachedModelPricing,
    fetched_at: &str,
) -> CostResult {
    let entry = cached_model_to_entry(pricing);
    match compute_cost(usage, &entry) {
        Ok(amount) => CostResult {
            amount_usd: Some(amount),
            status: CostStatus::Estimated,
            source: "provider_models_api".to_string(),
            pricing_version: Some(fetched_at.to_string()),
            label: format_label(CostStatus::Estimated, Some(amount)),
        },
        Err(()) => unknown_result(),
    }
}

/// 根据 model / provider / base_url 判定计费路由。
pub fn resolve_billing_route(
    _model: &str,
    provider: Option<&str>,
    base_url: Option<&str>,
) -> BillingRoute {
    let provider = provider
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase());
    let base_url = base_url.map(str::trim).filter(|s| !s.is_empty());

    if let Some(url) = base_url {
        if is_local_host(url) {
            return BillingRoute::Unknown;
        }
        if is_openrouter_host(url) {
            return BillingRoute::ProviderModelsApi {
                provider: provider.unwrap_or_else(|| "openrouter".to_string()),
                base_url: url.to_string(),
            };
        }
    }

    if let Some(ref p) = provider {
        if p == "custom" {
            return BillingRoute::Unknown;
        }
        if p == "openrouter" {
            if let Some(url) = base_url {
                return BillingRoute::ProviderModelsApi {
                    provider: p.clone(),
                    base_url: url.to_string(),
                };
            }
            return BillingRoute::Unknown;
        }
        if is_official_snapshot_provider(p) {
            return BillingRoute::OfficialSnapshot {
                provider: p.clone(),
            };
        }
    }

    if let Some(url) = base_url {
        if let Some(p) = infer_provider_from_base_url(url) {
            return BillingRoute::OfficialSnapshot { provider: p };
        }
    }

    BillingRoute::Unknown
}

/// 按路由与官方快照估算用量费用（USD）。
pub fn estimate_usage_cost(
    model: &str,
    usage: &UsageTokens,
    provider: Option<&str>,
    base_url: Option<&str>,
    api_key: Option<&str>,
) -> CostResult {
    let route = resolve_billing_route(model, provider, base_url);

    match route {
        BillingRoute::Included => CostResult {
            amount_usd: Some(0.0),
            status: CostStatus::Included,
            source: "subscription_included".to_string(),
            pricing_version: None,
            label: format_label(CostStatus::Included, Some(0.0)),
        },
        BillingRoute::OfficialSnapshot { provider } => {
            let Some(entry) = lookup_official_snapshot(&provider, model) else {
                return unknown_result();
            };
            match compute_cost(usage, entry) {
                Ok(amount) => CostResult {
                    amount_usd: Some(amount),
                    status: CostStatus::Estimated,
                    source: "official_docs_snapshot".to_string(),
                    pricing_version: Some(entry.pricing_version.to_string()),
                    label: format_label(CostStatus::Estimated, Some(amount)),
                },
                Err(()) => unknown_result(),
            }
        }
        BillingRoute::ProviderModelsApi { base_url, .. } => {
            estimate_from_provider_models_api(model, usage, &base_url, api_key)
        }
        BillingRoute::Unknown => unknown_result(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_models_api_response_skips_invalid_items() {
        let json = json!({
            "data": [
                { "id": "bad-no-pricing" },
                { "id": "bad-missing-completion", "pricing": { "prompt": "0.000001" } },
                { "id": "good/model", "pricing": { "prompt": "0.000001", "completion": "0.000002" } }
            ]
        });
        let cache = parse_models_api_response(json).expect("should parse with one valid model");
        assert_eq!(cache.models.len(), 1);
        let pricing = cache.models.get("good/model").expect("valid model present");
        assert!((pricing.prompt - 0.000001).abs() < 1e-12);
        assert!((pricing.completion - 0.000002).abs() < 1e-12);
    }
}
