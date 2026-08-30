//! Skill 商店：SkillHub / ClawHub API 搜索/详情，以及 skills.sh 页面爬取。

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;

use crate::models::{SkillStoreFilter, StoreSkill, StoreSkillDetail};

/// SkillHub HTTP API 根地址。
const SKILLHUB_API: &str = "https://api.skillhub.cn";
/// ClawHub HTTP API 根地址。
const CLAWHUB_API: &str = "https://clawhub.ai";
/// skills.sh 首页（用于解析列表）。
const SKILLS_SH_URL: &str = "https://www.skills.sh/";

/// SkillHub 列表接口响应外壳。
#[derive(Debug, Deserialize)]
struct SkillHubResponse {
    code: i32,
    data: Option<SkillHubData>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillHubData {
    skills: Vec<SkillHubSkill>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillHubSkill {
    name: String,
    slug: String,
    description: Option<String>,
    #[serde(rename = "description_zh")]
    description_zh: Option<String>,
    source: Option<String>,
    owner_name: Option<String>,
    installs: Option<u64>,
    downloads: Option<u64>,
    homepage: Option<String>,
    upstream_url: Option<String>,
    category: Option<String>,
    labels: Option<SkillHubLabels>,
}

#[derive(Debug, Deserialize)]
struct SkillHubLabels {
    requires_api_key: Option<String>,
}

fn http_client() -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .context("build HTTP client")
}

fn map_skillhub(s: SkillHubSkill) -> StoreSkill {
    let desc = s
        .description_zh
        .filter(|d| !d.is_empty())
        .or(s.description)
        .unwrap_or_default();
    let owner = s.owner_name.unwrap_or_else(|| "unknown".into());
    let slug = s.slug.clone();
    let requires_api_key = s
        .labels
        .and_then(|labels| labels.requires_api_key)
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        });
    // 勿用 homepage（api.skillhub.cn/...）：那不是可安装引用，会导致 CLI 安装必失败。
    let install_ref = match s.upstream_url.filter(|u| !u.trim().is_empty()) {
        Some(url) if url.contains("github.com/") => url,
        Some(url) if url.contains("clawhub") => {
            clawhub_install_ref(&url).unwrap_or_else(|| format!("skillhub:{owner}/{slug}"))
        }
        _ => format!("skillhub:{owner}/{slug}"),
    };
    StoreSkill {
        id: format!("skillhub:{owner}/{slug}"),
        name: s.name,
        description: desc,
        source: s.source.unwrap_or_else(|| "skillhub".into()),
        store: "skillhub".into(),
        installs: s.installs.or(s.downloads),
        install_ref,
        homepage: s.homepage,
        category: s.category.filter(|value| !value.trim().is_empty()),
        requires_api_key,
    }
}

/// `https://clawhub.ai/owner/skills/slug` 或 `…/owner/slug` → `clawhub:owner--slug`
fn clawhub_install_ref(url: &str) -> Option<String> {
    let path = url
        .split("clawhub.ai/")
        .nth(1)
        .or_else(|| url.split("clawhub.com/").nth(1))?
        .trim_matches('/');
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("skills") {
        return Some(format!("clawhub:{}--{}", parts[0], parts[2]));
    }
    if parts.len() >= 2 && parts[0] != "s" && parts[0] != "skills" {
        return Some(format!("clawhub:{}--{}", parts[0], parts[1]));
    }
    None
}

async fn fetch_skillhub(query: &str, limit: usize, page: usize) -> Result<Vec<StoreSkill>> {
    let client = http_client()?;
    let page = page.max(1);
    let mut url = format!(
        "{SKILLHUB_API}/api/skills?page={page}&pageSize={}&sortBy=score",
        limit.min(50)
    );
    if !query.trim().is_empty() {
        url.push_str(&format!("&keyword={}", urlencoding::encode(query.trim())));
    }

    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;

    let body: SkillHubResponse = resp.json().await.context("parse SkillHub JSON")?;

    if body.code != 0 {
        return Err(anyhow!(
            "{}",
            body.message.unwrap_or_else(|| "SkillHub API 错误".into())
        ));
    }

    let skills = body.data.map(|d| d.skills).unwrap_or_default();
    Ok(skills.into_iter().map(map_skillhub).collect())
}

fn parse_skills_sh_html(html: &str, query: &str) -> Vec<StoreSkill> {
    let re = Regex::new(
        r#"\\"source\\":\\"([^\\"]+)\\",\\"skillId\\":\\"([^\\"]+)\\",\\"name\\":\\"([^\\"]+)\\",\\"installs\\":(\d+)"#,
    )
    .expect("skills.sh regex");
    let q = query.trim().to_lowercase();
    let mut out = Vec::new();
    for cap in re.captures_iter(html) {
        let source = cap[1].to_string();
        let slug = cap[2].to_string();
        let name = cap[3].to_string();
        let installs: u64 = cap[4].parse().unwrap_or(0);
        if !q.is_empty()
            && !name.to_lowercase().contains(&q)
            && !source.to_lowercase().contains(&q)
            && !slug.to_lowercase().contains(&q)
        {
            continue;
        }
        out.push(StoreSkill {
            id: format!("skillsdotsh:{source}/{slug}"),
            name: name.clone(),
            description: format!("{source} · {slug}"),
            source: source.clone(),
            store: "skillsdotsh".into(),
            installs: Some(installs),
            // package 可能含 `/`（如 vercel-labs/skills），附带 skill 名供非交互安装
            install_ref: format!("skillsdotsh:{source}/{slug}"),
            homepage: Some(format!("https://skills.sh/{source}/{slug}")),
            category: None,
            requires_api_key: None,
        });
    }
    out
}

async fn fetch_skills_sh(query: &str, limit: usize, page: usize) -> Result<Vec<StoreSkill>> {
    let client = http_client()?;
    let resp = client
        .get(SKILLS_SH_URL)
        .header("User-Agent", "Astro/0.1 (+skills catalog crawler)")
        .send()
        .await
        .context("GET skills.sh")?;
    let html = resp.text().await.context("read skills.sh body")?;
    let all = parse_skills_sh_html(&html, query);
    let page = page.max(1);
    let start = (page - 1).saturating_mul(limit);
    Ok(all.into_iter().skip(start).take(limit).collect())
}

/// ClawHub 列表项。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawHubListSkill {
    slug: String,
    display_name: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    stats: Option<ClawHubStats>,
    topics: Option<Vec<String>>,
    categories: Option<Vec<String>>,
    metadata: Option<ClawHubMetadata>,
    updated_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ClawHubMetadata {
    setup: Option<Vec<ClawHubSetupItem>>,
}

#[derive(Debug, Deserialize)]
struct ClawHubSetupItem {
    key: Option<String>,
    required: Option<bool>,
}

fn normalize_clawhub_category(values: impl IntoIterator<Item = String>) -> Option<String> {
    for value in values {
        let normalized = value.trim().to_ascii_lowercase();
        let category = match normalized.as_str() {
            "productivity" | "office" => "office-efficiency",
            "writing" | "content" | "content-creation" => "content-creation",
            "development" | "developer-tools" | "programming" | "coding" => "dev-programming",
            "analytics" | "data" | "data-analysis" => "data-analysis",
            "design" | "media" | "multimedia" => "design-media",
            "agents" | "ai" | "automation" => "ai-agent",
            "knowledge" | "research" => "knowledge-management",
            "business" | "finance" | "marketing" => "business-ops",
            "education" | "learning" => "education",
            "security" | "devops" | "operations" => "it-ops-security",
            "lifestyle" | "health" | "travel" => "life-service",
            _ => continue,
        };
        return Some(category.into());
    }
    None
}

fn clawhub_requires_api_key(metadata: Option<ClawHubMetadata>) -> Option<bool> {
    let setup = metadata?.setup?;
    let has_required_key = setup.into_iter().any(|item| {
        item.required.unwrap_or(false)
            && item.key.is_some_and(|key| {
                let key = key.to_ascii_uppercase();
                key.contains("API_KEY") || key.contains("TOKEN") || key.contains("SECRET")
            })
    });
    Some(has_required_key)
}

#[derive(Debug, Deserialize)]
struct ClawHubStats {
    downloads: Option<u64>,
    installs: Option<u64>,
    stars: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawHubListResponse {
    items: Vec<ClawHubListSkill>,
    next_cursor: Option<String>,
}

fn map_clawhub(s: ClawHubListSkill) -> StoreSkill {
    let slug = s.slug;
    let name = s
        .display_name
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| slug.clone());
    let desc = s
        .summary
        .filter(|d| !d.is_empty())
        .or(s.description)
        .unwrap_or_default();
    let installs = s.stats.as_ref().and_then(|st| st.downloads.or(st.installs));
    let category = normalize_clawhub_category(
        s.categories
            .unwrap_or_default()
            .into_iter()
            .chain(s.topics.unwrap_or_default()),
    );
    let requires_api_key = clawhub_requires_api_key(s.metadata);
    let _ = s.updated_at;
    StoreSkill {
        id: format!("clawhub:{slug}"),
        name,
        description: desc,
        source: "clawhub".into(),
        store: "clawhub".into(),
        installs,
        install_ref: format!("clawhub:{slug}"),
        // 无 owner 时用官网短链；详情接口会补全 /{handle}/skills/{slug}
        homepage: Some(format!("https://clawhub.ai/s/skills/{slug}")),
        category,
        requires_api_key,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawHubSearchResponse {
    results: Vec<ClawHubSearchHit>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawHubSearchHit {
    slug: String,
    display_name: Option<String>,
    summary: Option<String>,
    downloads: Option<u64>,
    owner_handle: Option<String>,
    native: Option<ClawHubSearchNative>,
}

#[derive(Debug, Deserialize)]
struct ClawHubSearchNative {
    skill: Option<ClawHubSearchNativeSkill>,
}

#[derive(Debug, Deserialize)]
struct ClawHubSearchNativeSkill {
    categories: Option<Vec<String>>,
    topics: Option<Vec<String>>,
}

fn map_clawhub_search_hit(s: ClawHubSearchHit) -> StoreSkill {
    let slug = s.slug;
    let handle = s.owner_handle.filter(|h| !h.is_empty());
    let name = s
        .display_name
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| slug.clone());
    let desc = s.summary.unwrap_or_default();
    let category = s.native.and_then(|native| native.skill).and_then(|skill| {
        normalize_clawhub_category(
            skill
                .categories
                .unwrap_or_default()
                .into_iter()
                .chain(skill.topics.unwrap_or_default()),
        )
    });
    let (id, install_ref, homepage) = match &handle {
        Some(h) => (
            format!("clawhub:{h}/{slug}"),
            format!("clawhub:{h}--{slug}"),
            format!("https://clawhub.ai/{h}/skills/{slug}"),
        ),
        None => (
            format!("clawhub:{slug}"),
            format!("clawhub:{slug}"),
            format!("https://clawhub.ai/s/skills/{slug}"),
        ),
    };
    StoreSkill {
        id,
        name,
        description: desc,
        source: handle.unwrap_or_else(|| "clawhub".into()),
        store: "clawhub".into(),
        installs: s.downloads,
        install_ref,
        homepage: Some(homepage),
        category,
        requires_api_key: None,
    }
}

async fn fetch_clawhub_search(query: &str, limit: usize, page: usize) -> Result<Vec<StoreSkill>> {
    let client = http_client()?;
    let page = page.max(1);
    // 搜索接口无 offset 分页时表现不一，先拉一页再本地切片
    let fetch_limit = (page.saturating_mul(limit)).clamp(limit, 50);
    let url = format!(
        "{CLAWHUB_API}/api/v1/search?q={}&limit={fetch_limit}",
        urlencoding::encode(query.trim())
    );
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "Astro/0.1 (+skills catalog crawler)")
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("ClawHub search HTTP {}", resp.status()));
    }
    let body: ClawHubSearchResponse = resp.json().await.context("parse ClawHub search JSON")?;
    let all: Vec<StoreSkill> = body
        .results
        .into_iter()
        .map(map_clawhub_search_hit)
        .collect();
    let start = (page - 1).saturating_mul(limit);
    Ok(all.into_iter().skip(start).take(limit).collect())
}

async fn fetch_clawhub(query: &str, limit: usize, page: usize) -> Result<Vec<StoreSkill>> {
    let q = query.trim();
    if !q.is_empty() {
        return fetch_clawhub_search(q, limit, page).await;
    }

    let client = http_client()?;
    let page = page.max(1);
    let need = page.saturating_mul(limit);
    let mut collected: Vec<StoreSkill> = Vec::new();
    let mut cursor: Option<String> = None;
    // 首屏偶发空 items + cursor，最多跟几轮
    for _ in 0..10 {
        if collected.len() >= need {
            break;
        }
        let mut url = format!("{CLAWHUB_API}/api/v1/skills?limit=50&sortBy=downloads");
        if let Some(c) = &cursor {
            url.push_str(&format!("&cursor={}", urlencoding::encode(c)));
        }
        let resp = client
            .get(&url)
            .header("Accept", "application/json")
            .header("User-Agent", "Astro/0.1 (+skills catalog crawler)")
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        if !resp.status().is_success() {
            return Err(anyhow!("ClawHub list HTTP {}", resp.status()));
        }
        let body: ClawHubListResponse = resp.json().await.context("parse ClawHub list JSON")?;
        let batch_len = body.items.len();
        for item in body.items {
            collected.push(map_clawhub(item));
        }
        cursor = body.next_cursor.filter(|c| !c.is_empty());
        if cursor.is_none() {
            break;
        }
        if batch_len == 0 && cursor.is_some() {
            continue;
        }
        if batch_len == 0 {
            break;
        }
    }

    let start = (page - 1).saturating_mul(limit);
    Ok(collected.into_iter().skip(start).take(limit).collect())
}

fn merge_store_lists(
    lists: impl IntoIterator<Item = Vec<StoreSkill>>,
    limit: usize,
) -> Vec<StoreSkill> {
    let mut merged = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for list in lists {
        for item in list {
            if seen.insert(item.id.clone()) {
                merged.push(item);
            }
        }
    }
    merged.truncate(limit);
    merged
}

/// 从 SkillHub / skills.sh / ClawHub 搜索技能（支持 page 分页，从 1 起）
pub async fn search(
    query: &str,
    store: SkillStoreFilter,
    limit: usize,
    page: usize,
) -> Result<Vec<StoreSkill>> {
    let limit = limit.clamp(1, 50);
    let page = page.max(1);
    let q = query.trim();

    match store {
        SkillStoreFilter::SkillHub => fetch_skillhub(q, limit, page).await,
        SkillStoreFilter::SkillsDotSh => fetch_skills_sh(q, limit, page).await,
        SkillStoreFilter::ClawHub => fetch_clawhub(q, limit, page).await,
        SkillStoreFilter::All => {
            let (hub, sh, claw) = tokio::join!(
                fetch_skillhub(q, limit, page),
                fetch_skills_sh(q, limit, page),
                fetch_clawhub(q, limit, page)
            );
            Ok(merge_store_lists(
                [
                    hub.unwrap_or_default(),
                    sh.unwrap_or_default(),
                    claw.unwrap_or_default(),
                ],
                limit,
            ))
        }
    }
}

/// SkillHub `/api/v1/skills/{slug}` 详情响应。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillHubV1Detail {
    latest_version: Option<SkillHubV1LatestVersion>,
    owner: Option<SkillHubV1Owner>,
    skill: Option<SkillHubV1Skill>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillHubV1LatestVersion {
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillHubV1Owner {
    display_name: Option<String>,
    handle: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillHubV1Skill {
    display_name: Option<String>,
    slug: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(rename = "summary_zh")]
    summary_zh: Option<String>,
    source: Option<String>,
    category: Option<String>,
    #[serde(default)]
    sub_categories: Vec<SkillHubSubCategory>,
    icon_url: Option<String>,
    verified: Option<bool>,
    updated_at: Option<i64>,
    stats: Option<SkillHubV1Stats>,
}

#[derive(Debug, Deserialize)]
struct SkillHubSubCategory {
    name: Option<String>,
    key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillHubV1Stats {
    downloads: Option<u64>,
    installs: Option<u64>,
    stars: Option<u64>,
}

fn store_skill_slug(skill: &StoreSkill) -> String {
    if let Some(rest) = skill.id.strip_prefix("clawhub:") {
        // clawhub:slug / clawhub:owner--slug / clawhub:owner/slug
        let after_owner = rest.rsplit_once("--").map(|(_, s)| s).unwrap_or(rest);
        return after_owner
            .rsplit('/')
            .next()
            .unwrap_or(after_owner)
            .to_string();
    }
    skill
        .id
        .rsplit('/')
        .next()
        .unwrap_or(&skill.name)
        .to_string()
}

fn detail_from_list(skill: &StoreSkill) -> StoreSkillDetail {
    let slug = store_skill_slug(skill);
    // SkillHub homepage 常为 api.skillhub.cn/...（非网页）；官网路由仅为 /skills/:slug
    let detail_url = match skill.store.as_str() {
        "skillhub" => format!("https://skillhub.cn/skills/{slug}"),
        "clawhub" => skill
            .homepage
            .clone()
            .unwrap_or_else(|| format!("https://clawhub.ai/s/skills/{slug}")),
        _ => skill
            .homepage
            .clone()
            .unwrap_or_else(|| format!("https://skills.sh/{}/{}", skill.source, slug)),
    };

    StoreSkillDetail {
        name: skill.name.clone(),
        slug,
        description: skill.description.clone(),
        overview: skill.description.clone(),
        source: skill.source.clone(),
        store: skill.store.clone(),
        installs: skill.installs,
        downloads: skill.installs,
        stars: None,
        install_ref: skill.install_ref.clone(),
        homepage: skill.homepage.clone(),
        detail_url,
        icon_url: None,
        category: skill.category.clone(),
        sub_categories: vec![],
        version: None,
        updated_at: None,
        owner_name: None,
        verified: None,
    }
}

async fn fetch_skillhub_v1_detail(slug: &str) -> Result<SkillHubV1Detail> {
    let client = http_client()?;
    let url = format!("{SKILLHUB_API}/api/v1/skills/{}", urlencoding::encode(slug));
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("SkillHub detail HTTP {}", resp.status()));
    }
    resp.json().await.context("parse SkillHub v1 detail JSON")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawHubDetailResponse {
    skill: Option<ClawHubDetailSkill>,
    owner: Option<ClawHubOwner>,
    latest_version: Option<ClawHubLatestVersion>,
    matches: Option<Vec<ClawHubAmbiguousMatch>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawHubDetailSkill {
    slug: Option<String>,
    display_name: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    topics: Option<Vec<String>>,
    stats: Option<ClawHubStats>,
    updated_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawHubOwner {
    handle: Option<String>,
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawHubLatestVersion {
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClawHubAmbiguousMatch {
    owner_handle: Option<String>,
    slug: Option<String>,
    url: Option<String>,
}

async fn fetch_clawhub_v1_detail(slug: &str) -> Result<ClawHubDetailResponse> {
    let client = http_client()?;
    let url = format!("{CLAWHUB_API}/api/v1/skills/{}", urlencoding::encode(slug));
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "Astro/0.1 (+skills catalog crawler)")
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let body = resp.text().await.context("read ClawHub detail body")?;
    // 409 AMBIGUOUS_SKILL_SLUG 仍返回可用 matches JSON
    if !(status.is_success() || status.as_u16() == 409) {
        return Err(anyhow!("ClawHub detail HTTP {status}: {body}"));
    }
    serde_json::from_str(&body).context("parse ClawHub detail JSON")
}

fn apply_clawhub_detail(detail: &mut StoreSkillDetail, body: ClawHubDetailResponse) {
    if let Some(matches) = body.matches.filter(|m| !m.is_empty()) {
        if let Some(first) = matches.into_iter().next() {
            if let Some(url) = first.url.filter(|u| !u.is_empty()) {
                detail.detail_url = url.clone();
                detail.homepage = Some(url);
            }
            if let Some(handle) = first.owner_handle.filter(|h| !h.is_empty()) {
                detail.owner_name = Some(handle.clone());
                if let Some(slug) = first.slug.filter(|s| !s.is_empty()) {
                    detail.slug = slug.clone();
                    detail.install_ref = format!("clawhub:{handle}--{slug}");
                }
            }
        }
        return;
    }

    let Some(api_skill) = body.skill else {
        return;
    };
    if let Some(name) = api_skill.display_name.filter(|s| !s.is_empty()) {
        detail.name = name;
    }
    if let Some(slug) = api_skill.slug.filter(|s| !s.is_empty()) {
        detail.slug = slug;
    }
    let overview = api_skill
        .summary
        .filter(|s| !s.is_empty())
        .or(api_skill.description.filter(|s| !s.is_empty()))
        .unwrap_or_default();
    if !overview.is_empty() {
        detail.description = overview.clone();
        detail.overview = overview;
    }
    detail.sub_categories = api_skill.topics.unwrap_or_default();
    detail.updated_at = api_skill.updated_at;
    if let Some(stats) = api_skill.stats {
        detail.downloads = stats.downloads.or(detail.downloads);
        detail.installs = stats.installs.or(detail.installs);
        detail.stars = stats.stars;
    }
    detail.version = body
        .latest_version
        .and_then(|v| v.version)
        .filter(|s| !s.is_empty());
    if let Some(owner) = body.owner {
        let handle = owner.handle.filter(|s| !s.is_empty());
        detail.owner_name = owner
            .display_name
            .filter(|s| !s.is_empty())
            .or_else(|| handle.clone());
        if let Some(h) = handle {
            detail.detail_url = format!("https://clawhub.ai/{h}/skills/{}", detail.slug);
            detail.homepage = Some(detail.detail_url.clone());
            detail.install_ref = format!("clawhub:{h}--{}", detail.slug);
        }
    }
}

/// 详情：SkillHub / ClawHub 拉 API 补全；其它商店回退列表字段。
pub async fn fetch_detail(skill: &StoreSkill) -> Result<StoreSkillDetail> {
    let mut detail = detail_from_list(skill);

    match skill.store.as_str() {
        "skillhub" => match fetch_skillhub_v1_detail(&detail.slug).await {
            Ok(body) => {
                let Some(api_skill) = body.skill else {
                    return Ok(detail);
                };
                if let Some(name) = api_skill.display_name.filter(|s| !s.is_empty()) {
                    detail.name = name;
                }
                if let Some(slug) = api_skill.slug.filter(|s| !s.is_empty()) {
                    detail.slug = slug;
                    detail.detail_url = format!("https://skillhub.cn/skills/{}", detail.slug);
                }
                let overview = api_skill
                    .summary_zh
                    .filter(|s| !s.is_empty())
                    .or(api_skill.summary.filter(|s| !s.is_empty()))
                    .unwrap_or_default();
                if !overview.is_empty() {
                    detail.description = overview.clone();
                    detail.overview = overview;
                }
                if let Some(source) = api_skill.source.filter(|s| !s.is_empty()) {
                    detail.source = source;
                }
                detail.category = api_skill.category.filter(|s| !s.is_empty());
                detail.sub_categories = api_skill
                    .sub_categories
                    .into_iter()
                    .filter_map(|c| c.name.or(c.key).filter(|s| !s.is_empty()))
                    .collect();
                detail.icon_url = api_skill.icon_url.filter(|s| !s.is_empty());
                detail.verified = api_skill.verified;
                detail.updated_at = api_skill.updated_at;
                if let Some(stats) = api_skill.stats {
                    detail.downloads = stats.downloads.or(detail.downloads);
                    detail.installs = stats.installs.or(detail.installs);
                    detail.stars = stats.stars;
                }
                detail.version = body
                    .latest_version
                    .and_then(|v| v.version)
                    .filter(|s| !s.is_empty());
                detail.owner_name = body.owner.and_then(|o| {
                    o.display_name
                        .filter(|s| !s.is_empty())
                        .or(o.handle.filter(|s| !s.is_empty()))
                });
                Ok(detail)
            }
            Err(err) => {
                tracing::warn!(error = %err, slug = %detail.slug, "SkillHub detail fetch failed; using list fields");
                Ok(detail)
            }
        },
        "clawhub" => match fetch_clawhub_v1_detail(&detail.slug).await {
            Ok(body) => {
                apply_clawhub_detail(&mut detail, body);
                Ok(detail)
            }
            Err(err) => {
                tracing::warn!(error = %err, slug = %detail.slug, "ClawHub detail fetch failed; using list fields");
                Ok(detail)
            }
        },
        _ => Ok(detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skillhub_install_ref_prefers_skillhub_prefix_over_homepage() {
        let s = SkillHubSkill {
            name: "web-tools-guide".into(),
            slug: "web-tools-guide".into(),
            description: None,
            description_zh: Some("desc".into()),
            source: Some("community".into()),
            owner_name: Some("user_x".into()),
            installs: Some(1),
            downloads: None,
            homepage: Some("https://api.skillhub.cn/user_x/web-tools-guide".into()),
            upstream_url: None,
            category: Some("knowledge-management".into()),
            labels: Some(SkillHubLabels {
                requires_api_key: Some("false".into()),
            }),
        };
        let mapped = map_skillhub(s);
        assert_eq!(mapped.install_ref, "skillhub:user_x/web-tools-guide");
        assert!(!mapped.install_ref.contains("api.skillhub.cn"));
        assert_eq!(mapped.category.as_deref(), Some("knowledge-management"));
        assert_eq!(mapped.requires_api_key, Some(false));
    }

    #[test]
    fn clawhub_url_to_install_ref() {
        assert_eq!(
            clawhub_install_ref("https://clawhub.ai/guipi888/find-skills").as_deref(),
            Some("clawhub:guipi888--find-skills")
        );
        assert_eq!(
            clawhub_install_ref("https://clawhub.ai/steipete/skills/weather").as_deref(),
            Some("clawhub:steipete--weather")
        );
    }

    #[test]
    fn clawhub_slug_from_id() {
        let skill = StoreSkill {
            id: "clawhub:outlit-sdk".into(),
            name: "Outlit SDK".into(),
            description: "d".into(),
            source: "clawhub".into(),
            store: "clawhub".into(),
            installs: Some(1),
            install_ref: "clawhub:outlit-sdk".into(),
            homepage: Some("https://clawhub.ai/s/skills/outlit-sdk".into()),
            category: None,
            requires_api_key: None,
        };
        assert_eq!(store_skill_slug(&skill), "outlit-sdk");
        assert_eq!(
            detail_from_list(&skill).detail_url,
            "https://clawhub.ai/s/skills/outlit-sdk"
        );
        let owned = StoreSkill {
            id: "clawhub:steipete/weather".into(),
            name: "Weather".into(),
            description: "d".into(),
            source: "steipete".into(),
            store: "clawhub".into(),
            installs: Some(1),
            install_ref: "clawhub:steipete--weather".into(),
            homepage: Some("https://clawhub.ai/steipete/skills/weather".into()),
            category: None,
            requires_api_key: None,
        };
        assert_eq!(store_skill_slug(&owned), "weather");
    }

    #[test]
    fn clawhub_search_hit_maps_owner() {
        let hit = ClawHubSearchHit {
            slug: "weather".into(),
            display_name: Some("Weather".into()),
            summary: Some("Get weather".into()),
            downloads: Some(163969),
            owner_handle: Some("steipete".into()),
            native: Some(ClawHubSearchNative {
                skill: Some(ClawHubSearchNativeSkill {
                    categories: Some(vec!["lifestyle".into()]),
                    topics: Some(vec!["Weather".into()]),
                }),
            }),
        };
        let mapped = map_clawhub_search_hit(hit);
        assert_eq!(mapped.id, "clawhub:steipete/weather");
        assert_eq!(mapped.install_ref, "clawhub:steipete--weather");
        assert_eq!(
            mapped.homepage.as_deref(),
            Some("https://clawhub.ai/steipete/skills/weather")
        );
        assert_eq!(mapped.category.as_deref(), Some("life-service"));
    }

    #[test]
    fn skills_sh_install_ref_includes_skill_id() {
        let html = r#"\"source\":\"vercel-labs/skills\",\"skillId\":\"find-skills\",\"name\":\"find-skills\",\"installs\":1"#;
        let list = parse_skills_sh_html(html, "");
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].install_ref,
            "skillsdotsh:vercel-labs/skills/find-skills"
        );
    }

    #[test]
    fn skillhub_detail_url_ignores_api_homepage() {
        let skill = StoreSkill {
            id: "skillhub:user_x/web-tools-guide".into(),
            name: "web-tools-guide".into(),
            description: "d".into(),
            source: "community".into(),
            store: "skillhub".into(),
            installs: Some(1),
            install_ref: "skillhub:user_x/web-tools-guide".into(),
            homepage: Some("https://api.skillhub.cn/user_x/web-tools-guide".into()),
            category: Some("knowledge-management".into()),
            requires_api_key: Some(false),
        };
        let detail = detail_from_list(&skill);
        assert_eq!(
            detail.detail_url,
            "https://skillhub.cn/skills/web-tools-guide"
        );
        assert_eq!(detail.slug, "web-tools-guide");
    }

    #[test]
    fn skillhub_v1_detail_parses_stats() {
        let raw = r#"{
            "latestVersion": {"version": "1.0.2"},
            "owner": {"displayName": "user_x", "handle": "user_x"},
            "skill": {
                "displayName": "web-tools-guide",
                "slug": "web-tools-guide",
                "summary_zh": "desc",
                "source": "community",
                "category": "knowledge-management",
                "subCategories": [{"key": "knowledge-retrieval", "name": "信息检索"}],
                "iconUrl": "https://example.com/icon.png",
                "verified": false,
                "updatedAt": 1784078500822,
                "stats": {"downloads": 182494, "installs": 3459, "stars": 129}
            }
        }"#;
        let body: SkillHubV1Detail = serde_json::from_str(raw).unwrap();
        let skill = body.skill.unwrap();
        let stats = skill.stats.unwrap();
        assert_eq!(stats.downloads, Some(182494));
        assert_eq!(stats.installs, Some(3459));
        assert_eq!(stats.stars, Some(129));
        assert_eq!(
            body.latest_version.unwrap().version.as_deref(),
            Some("1.0.2")
        );
        assert_eq!(body.owner.unwrap().handle.as_deref(), Some("user_x"));
    }
}
