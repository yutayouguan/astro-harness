//! 运行时已加载的 Skill：元数据 + `SKILL.md` 正文 + 路径。

use serde::{Deserialize, Serialize};

/// Skill 前置元数据（通常来自 front matter 或文件头）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// Skill 名称（注册表键）。
    pub name: String,
    /// 简短说明。
    pub description: String,
    /// 可选：加载本 skill 后 additive 放宽的 toolset id 列表（`astro_tools` frontmatter）。
    #[serde(default)]
    pub astro_tools: Vec<String>,
}

/// 磁盘加载后的完整 Skill。
#[derive(Debug, Clone)]
pub struct LoadedSkill {
    /// 元数据。
    pub metadata: SkillMetadata,
    /// `SKILL.md` 正文。
    pub content: String,
    /// 源文件路径。
    pub path: std::path::PathBuf,
}
