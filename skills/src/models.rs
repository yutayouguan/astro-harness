//! Skills 领域 DTO：本机安装条目、商店搜索结果与筛选。

use serde::{Deserialize, Serialize};

/// 技能包内单个文件条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFileEntry {
    /// 相对 skill 根目录的路径（`/` 分隔）。
    pub relative_path: String,
    /// 分类：overview / scripts / references / assets / other。
    pub category: String,
    /// 是否按文本预览。
    pub is_text: bool,
    /// 字节大小。
    pub size: u64,
}

/// 技能包文件清单（用于查看抽屉）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillBundle {
    pub name: String,
    pub description: String,
    /// 技能根目录绝对路径。
    pub root: String,
    pub files: Vec<SkillFileEntry>,
}

/// 本机已安装的 Skill（扫描 `SKILL.md` 得到）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSkill {
    /// 稳定 id。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// 描述。
    pub description: String,
    /// `SKILL.md` 路径。
    pub path: String,
    /// 所在源目录。
    pub source_dir: String,
    /// 是否对当前 Agent 启用。
    pub enabled: bool,
    /// 作用域（如全局 / Agent）。
    #[serde(default)]
    pub scope: String,
    /// 是否已链接到某 Agent。
    #[serde(default)]
    pub linked: bool,
}

/// 商店搜索结果条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreSkill {
    /// 商店侧 id。
    pub id: String,
    /// 名称。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 来源仓库或标识。
    pub source: String,
    /// 商店名（skillhub / skills.sh 等）。
    pub store: String,
    /// 安装次数（若有）。
    pub installs: Option<u64>,
    /// 安装引用（传给 install）。
    pub install_ref: String,
    /// 主页。
    pub homepage: Option<String>,
}

/// 商店详情（详情页 / 安装提示）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreSkillDetail {
    /// 名称。
    pub name: String,
    /// URL slug。
    pub slug: String,
    /// 短描述。
    pub description: String,
    /// 长概述。
    pub overview: String,
    /// 来源。
    pub source: String,
    /// 商店。
    pub store: String,
    /// 安装次数。
    pub installs: Option<u64>,
    /// 下载次数。
    pub downloads: Option<u64>,
    /// Star 数。
    pub stars: Option<u64>,
    /// 安装引用。
    pub install_ref: String,
    /// 主页。
    pub homepage: Option<String>,
    /// 详情页 URL。
    pub detail_url: String,
    /// 图标。
    pub icon_url: Option<String>,
    /// 分类。
    pub category: Option<String>,
    /// 子分类。
    pub sub_categories: Vec<String>,
    /// 版本。
    pub version: Option<String>,
    /// 更新时间戳。
    pub updated_at: Option<i64>,
    /// 作者。
    pub owner_name: Option<String>,
    /// 是否认证。
    pub verified: Option<bool>,
}

/// 商店筛选：`skillhub` | `skillsdotsh` | `clawhub` | `all`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillStoreFilter {
    /// 全部来源。
    All,
    /// 仅 SkillHub。
    SkillHub,
    /// 仅 skills.sh。
    SkillsDotSh,
    /// 仅 ClawHub。
    ClawHub,
}

impl SkillStoreFilter {
    /// 解析查询字符串；未知值返回 `None`。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "all" => Some(Self::All),
            "skillhub" => Some(Self::SkillHub),
            "skillsdotsh" => Some(Self::SkillsDotSh),
            "clawhub" => Some(Self::ClawHub),
            _ => None,
        }
    }
}

/// 技能安装来源记录（`skill-origins.json` 单条）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillOriginRecord {
    pub folder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    pub name: String,
    pub store: String,
    pub install_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub installed_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_updated_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
}

/// 技能安装来源清单文件。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillOriginsFile {
    pub version: u32,
    pub records: Vec<SkillOriginRecord>,
}

/// 批量更新单条结果（「更新」Tab）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUpdateItemResult {
    pub folder: String,
    pub ok: bool,
    pub message: String,
}

/// 技能更新检查状态（与远端元数据比对）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillUpdateStatus {
    Outdated,
    Current,
    /// 无可用远端 version/updated_at。
    Unknown,
    Error,
}

/// 单条技能更新检查结果（「更新」Tab 检查 API）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUpdateCheckResult {
    pub folder: String,
    pub status: SkillUpdateStatus,
    pub remote_version: Option<String>,
    pub remote_updated_at: Option<i64>,
    pub message: String,
}

/// 更新前本地改动预览（与安装时 `content_digest` baseline 比对）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillUpdatePreview {
    pub folder: String,
    pub has_local_changes: bool,
    pub has_baseline_digest: bool,
    pub current_digest: Option<String>,
    pub baseline_digest: Option<String>,
}
