//! Astro Skills 领域层：本机扫描、商店搜索、安装与运行时注册表。
//!
//! ```text
//! skills/          领域库（agent / backend / Tauri 共用）
//! ├── models             DTO 与商店筛选
//! ├── installed          扫描 SKILL.md、启用状态
//! ├── store              SkillHub API、skills.sh 爬虫
//! ├── install            SkillHub HTTP / npx skills add
//! ├── skill / registry   运行时 LoadedSkill
//! ```

pub mod check;
pub mod digest;
pub mod install;
pub mod installed;
pub mod models;
pub mod origins;
pub mod preview;
pub mod registry;
pub mod seed;
pub mod skill;
pub mod store;
pub mod update;

pub use install::{install_from_ref, InstallOriginHint};
pub use update::{update_all_with_origin, update_installed_skill, update_outdated_skills};
pub use seed::{
    is_public_skill_installed, seed_default_public_skills, SeedReport, DEFAULT_PUBLIC_SKILLS,
};
pub use installed::{
    link_skill_to_agent, list_enabled_for_prompt, list_installed, list_installed_for_agent,
    list_skill_files, list_skill_files_ex, load_skill_by_name, open_skill_file_externally,
    open_skill_folder, read_skill_file, read_skill_file_ex, reveal_skill_file, set_enabled,
    set_enabled_for_agent,
};
pub use models::{
    InstalledSkill, SkillBundle, SkillFileEntry, SkillStoreFilter, SkillUpdateCheckResult,
    SkillUpdateItemResult, SkillUpdatePreview, SkillUpdateStatus, StoreSkill, StoreSkillDetail,
};
pub use preview::preview_skill_update;
pub use check::{
    check_origin_against_detail, check_updates_for_agent, classify_update_status,
    filter_outdated_folders, origin_to_store_skill,
};
pub use registry::SkillRegistry;
pub use skill::{LoadedSkill, SkillMetadata};
pub use store::{fetch_detail, search};
