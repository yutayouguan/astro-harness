//! SkillHub 在线技能市场：列表、搜索与详情。

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::Deserialize;

use crate::models::{StoreSkill, StoreSkillDetail};

/// SkillHub HTTP API 根地址。
const SKILLHUB_API: &str = "https://api.skillhub.cn";

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
    icon_url: Option<String>,
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
    let install_ref = format!("skillhub:{owner}/{slug}");
    StoreSkill {
        id: format!("skillhub:{owner}/{slug}"),
        name: s.name,
        description: desc,
        source: s.source.unwrap_or_else(|| "skillhub".into()),
        store: "skillhub".into(),
        installs: s.installs.or(s.downloads),
        install_ref,
        homepage: s.homepage,
        icon_url: s.icon_url.filter(|value| !value.trim().is_empty()),
        category: s.category.filter(|value| !value.trim().is_empty()),
        requires_api_key,
    }
}

fn skillhub_list_url(
    query: &str,
    limit: usize,
    page: usize,
    sort: Option<&str>,
    category: Option<&str>,
    api_key: Option<&str>,
) -> String {
    let mut url = format!(
        "{SKILLHUB_API}/api/skills?page={}&pageSize={}",
        page.max(1),
        limit.clamp(1, 50)
    );
    let sort_by = match sort.map(str::trim) {
        Some("trending") => Some("score"),
        Some("downloads") => Some("downloads"),
        Some("recent") => Some("updated_at"),
        _ => None,
    };
    if let Some(sort_by) = sort_by {
        url.push_str(&format!("&sortBy={sort_by}&order=desc"));
    }
    if !query.trim().is_empty() {
        url.push_str(&format!("&keyword={}", urlencoding::encode(query.trim())));
    }
    if let Some(category) = category.map(str::trim).filter(|value| !value.is_empty()) {
        url.push_str(&format!("&category={}", urlencoding::encode(category)));
    }
    let api_key_label = match api_key.map(str::trim) {
        Some("required") => Some("requires_api_key:true"),
        Some("not-required") => Some("requires_api_key:false"),
        _ => None,
    };
    if let Some(label) = api_key_label {
        url.push_str(&format!("&labels={}", urlencoding::encode(label)));
    }
    url
}

async fn fetch_skillhub(
    query: &str,
    limit: usize,
    page: usize,
    sort: Option<&str>,
    category: Option<&str>,
    api_key: Option<&str>,
) -> Result<Vec<StoreSkill>> {
    let client = http_client()?;
    let url = skillhub_list_url(query, limit, page, sort, category, api_key);

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

/// 从 SkillHub 搜索技能（支持 page 分页，从 1 起）。
pub async fn search(query: &str, limit: usize, page: usize) -> Result<Vec<StoreSkill>> {
    search_with_filters(query, limit, page, Some("trending"), None, None).await
}

/// 从 SkillHub 搜索并按官方列表接口的排序、场景和 API Key 标签筛选。
pub async fn search_with_filters(
    query: &str,
    limit: usize,
    page: usize,
    sort: Option<&str>,
    category: Option<&str>,
    api_key: Option<&str>,
) -> Result<Vec<StoreSkill>> {
    fetch_skillhub(query.trim(), limit, page, sort, category, api_key).await
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
    skill
        .id
        .rsplit('/')
        .next()
        .unwrap_or(&skill.name)
        .to_string()
}

fn detail_from_list(skill: &StoreSkill) -> StoreSkillDetail {
    let slug = store_skill_slug(skill);
    // SkillHub homepage 常为 api.skillhub.cn/...（非网页）；官网路由仅为 /skills/:slug。
    let detail_url = format!("https://skillhub.cn/skills/{slug}");

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
        icon_url: skill.icon_url.clone(),
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

/// 从 SkillHub 拉取详情；失败时回退列表字段。
pub async fn fetch_detail(skill: &StoreSkill) -> Result<StoreSkillDetail> {
    let mut detail = detail_from_list(skill);

    match fetch_skillhub_v1_detail(&detail.slug).await {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skillhub_list_parses_and_maps_icon_url() {
        let raw = r#"{
            "code": 0,
            "data": {
                "skills": [{
                    "name": "Demo",
                    "slug": "demo",
                    "iconUrl": "https://cdn.example.com/demo.png"
                }]
            }
        }"#;
        let body: SkillHubResponse = serde_json::from_str(raw).unwrap();
        let mapped = map_skillhub(body.data.unwrap().skills.into_iter().next().unwrap());
        assert_eq!(
            mapped.icon_url.as_deref(),
            Some("https://cdn.example.com/demo.png")
        );
    }

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
            icon_url: Some("https://cdn.example.com/web-tools-guide.png".into()),
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
        assert_eq!(
            mapped.icon_url.as_deref(),
            Some("https://cdn.example.com/web-tools-guide.png")
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
            icon_url: Some("https://cdn.example.com/web-tools-guide.png".into()),
            category: Some("knowledge-management".into()),
            requires_api_key: Some(false),
        };
        let detail = detail_from_list(&skill);
        assert_eq!(
            detail.detail_url,
            "https://skillhub.cn/skills/web-tools-guide"
        );
        assert_eq!(detail.slug, "web-tools-guide");
        assert_eq!(
            detail.icon_url.as_deref(),
            Some("https://cdn.example.com/web-tools-guide.png")
        );
    }

    #[test]
    fn skillhub_list_url_maps_marketplace_filters() {
        let url = skillhub_list_url(
            "  rust agent  ",
            80,
            0,
            Some("recent"),
            Some("dev-programming"),
            Some("not-required"),
        );
        assert_eq!(
            url,
            "https://api.skillhub.cn/api/skills?page=1&pageSize=50&sortBy=updated_at&order=desc&keyword=rust%20agent&category=dev-programming&labels=requires_api_key%3Afalse"
        );
    }

    #[test]
    fn skillhub_list_url_keeps_all_unsorted_and_maps_trending_to_score() {
        let all = skillhub_list_url("", 24, 1, Some("all"), None, None);
        assert_eq!(all, "https://api.skillhub.cn/api/skills?page=1&pageSize=24");

        let trending = skillhub_list_url("", 24, 1, Some("trending"), None, None);
        assert_eq!(
            trending,
            "https://api.skillhub.cn/api/skills?page=1&pageSize=24&sortBy=score&order=desc"
        );
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
