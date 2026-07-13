//! 路由感知用量费用估算：官方价快照 + 兼容端 models API（Task 3）+ 订阅 included。
//!
//! 费用路径 **禁止** 读取 `litellm-model-meta.json`。

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
    _api_key: Option<&str>,
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
        BillingRoute::ProviderModelsApi { .. } => {
            // Task 3: OpenRouter /models API + 本地缓存
            unknown_result()
        }
        BillingRoute::Unknown => unknown_result(),
    }
}
