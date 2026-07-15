//! Skill 商店：SkillHub API 搜索/详情，以及 skills.sh 页面爬取。

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;

use crate::models::{SkillStoreFilter, StoreSkill, StoreSkillDetail};

/// SkillHub HTTP API 根地址。
const SKILLHUB_API: &str = "https://api.skillhub.cn";
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
    // 勿用 homepage（api.skillhub.cn/...）：那不是可安装引用，会导致 CLI 安装必失败。
    let install_ref = match s.upstream_url.filter(|u| !u.trim().is_empty()) {
        Some(url) if url.contains("github.com/") => url,
        Some(url) if url.contains("clawhub") => clawhub_install_ref(&url)
            .unwrap_or_else(|| format!("skillhub:{owner}/{slug}")),
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
    }
}

/// `https://clawhub.ai/owner/slug` → `clawhub:owner--slug`
fn clawhub_install_ref(url: &str) -> Option<String> {
    let path = url
        .split("clawhub.ai/")
        .nth(1)
        .or_else(|| url.split("clawhub.com/").nth(1))?
        .trim_matches('/');
    let mut parts = path.split('/').filter(|p| !p.is_empty());
    let owner = parts.next()?;
    let slug = parts.next()?;
    Some(format!("clawhub:{owner}--{slug}"))
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
        });
    }
    out
}

async fn fetch_skills_sh(query: &str, limit: usize, page: usize) -> Result<Vec<StoreSkill>> {
    let client = http_client()?;
    let resp = client
        .get(SKILLS_SH_URL)
        .header(
            "User-Agent",
            "Astro/0.1 (+skills catalog crawler)",
        )
        .send()
        .await
        .context("GET skills.sh")?;
    let html = resp.text().await.context("read skills.sh body")?;
    let all = parse_skills_sh_html(&html, query);
    let page = page.max(1);
    let start = (page - 1).saturating_mul(limit);
    Ok(all.into_iter().skip(start).take(limit).collect())
}

/// 从 SkillHub / skills.sh 搜索技能（支持 page 分页，从 1 起）
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
        SkillStoreFilter::All => {
            let (hub, sh) = tokio::join!(
                fetch_skillhub(q, limit, page),
                fetch_skills_sh(q, limit, page)
            );
            let mut merged = hub.unwrap_or_default();
            let mut seen: std::collections::HashSet<String> =
                merged.iter().map(|s| s.id.clone()).collect();
            for item in sh.unwrap_or_default() {
                if seen.insert(item.id.clone()) {
                    merged.push(item);
                }
            }
            merged.truncate(limit);
            Ok(merged)
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

fn detail_from_list(skill: &StoreSkill) -> StoreSkillDetail {
    let slug = skill
        .id
        .rsplit('/')
        .next()
        .unwrap_or(&skill.name)
        .to_string();
    // SkillHub homepage 常为 api.skillhub.cn/...（非网页）；官网路由仅为 /skills/:slug
    let detail_url = match skill.store.as_str() {
        "skillhub" => format!("https://skillhub.cn/skills/{slug}"),
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
        category: None,
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

/// 详情：SkillHub 拉 v1 详情补全下载量等；其它商店回退列表字段。
pub async fn fetch_detail(skill: &StoreSkill) -> Result<StoreSkillDetail> {
    let mut detail = detail_from_list(skill);

    if skill.store != "skillhub" {
        return Ok(detail);
    }

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
        };
        let mapped = map_skillhub(s);
        assert_eq!(mapped.install_ref, "skillhub:user_x/web-tools-guide");
        assert!(!mapped.install_ref.contains("api.skillhub.cn"));
    }

    #[test]
    fn clawhub_url_to_install_ref() {
        assert_eq!(
            clawhub_install_ref("https://clawhub.ai/guipi888/find-skills").as_deref(),
            Some("clawhub:guipi888--find-skills")
        );
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
        assert_eq!(body.latest_version.unwrap().version.as_deref(), Some("1.0.2"));
        assert_eq!(body.owner.unwrap().handle.as_deref(), Some("user_x"));
    }
}
