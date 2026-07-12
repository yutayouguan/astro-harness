//! Astro Skills 领域层：本机扫描、商店搜索、安装与运行时注册表。
//!
//! ```text
//! skills/          领域库（agent / backend / Tauri 共用）
//! ├── models             DTO 与商店筛选
//! ├── installed          扫描 SKILL.md、启用状态
//! ├── store              SkillHub API、skills.sh 爬虫
//! ├── install            npx skills add
//! ├── skill / registry   运行时 LoadedSkill
//! ```

pub mod install;
pub mod installed;
pub mod models;
pub mod registry;
pub mod seed;
pub mod skill;
pub mod store;

pub use install::install_from_ref;
pub use seed::{
    is_public_skill_installed, seed_default_public_skills, SeedReport, DEFAULT_PUBLIC_SKILLS,
};
pub use installed::{
    link_skill_to_agent, list_enabled_for_prompt, list_installed, list_installed_for_agent,
    load_skill_by_name, set_enabled, set_enabled_for_agent,
};
pub use models::{InstalledSkill, SkillStoreFilter, StoreSkill, StoreSkillDetail};
pub use registry::SkillRegistry;
pub use skill::{LoadedSkill, SkillMetadata};
pub use store::{fetch_detail, search};
