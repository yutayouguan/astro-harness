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

use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// 线程安全的 workspace 目录覆盖，替代全局 `ASTRO_WORKSPACE` 环境变量。
///
/// `agent` 和 `tools` 在需要时调用 [`set_workspace_override`]，
/// `installed` 在发现 skill 根目录时通过 [`workspace_override`] 读取。
static WORKSPACE_OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// 设置当前进程的 workspace 目录覆盖（线程安全）。
pub fn set_workspace_override(path: &Path) {
    if let Ok(mut g) = WORKSPACE_OVERRIDE.write() {
        *g = Some(path.to_path_buf());
    }
}

/// 读取当前 workspace 目录覆盖；未设置时返回 `None`。
pub fn workspace_override() -> Option<PathBuf> {
    WORKSPACE_OVERRIDE.read().ok().and_then(|g| g.clone())
}

pub mod backups;
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
pub mod snapshots;
pub mod store;
pub mod update;
pub mod usage;

pub use backups::{list_skill_backups, reveal_skill_backup, SkillBackupEntry};
pub use check::{
    check_origin_against_detail, check_updates_for_agent, classify_update_status,
    filter_outdated_folders, origin_to_store_skill,
};
pub use install::{install_from_ref, InstallOriginHint};
pub use installed::{
    link_skill_to_agent, list_enabled_for_prompt, list_installed, list_installed_for_agent,
    list_skill_files, list_skill_files_ex, load_skill_by_name, open_skill_file_externally,
    open_skill_folder, parse_skill_frontmatter_full, read_skill_file, read_skill_file_ex,
    recent_astro_tools, reveal_skill_file, set_enabled, set_enabled_for_agent,
};
pub use models::{
    InstalledSkill, SkillBundle, SkillFileEntry, SkillStoreFilter, SkillUpdateCheckResult,
    SkillUpdateItemResult, SkillUpdatePreview, SkillUpdateStatus, StoreSkill, StoreSkillDetail,
    UpdateSkillOpts,
};
pub use preview::preview_skill_update;
pub use registry::SkillRegistry;
pub use seed::{
    is_public_skill_installed, seed_bundled_into, seed_default_public_skills, SeedReport,
    BUNDLED_SKILLS, DEFAULT_PUBLIC_SKILLS,
};
pub use skill::{LoadedSkill, SkillMetadata};
pub use snapshots::{
    list_snapshots, restore_latest as restore_skill_snapshot, save_snapshot, SkillSnapshot,
};
pub use store::{fetch_detail, search};
pub use update::{
    backup_skill_dir, update_all_with_origin, update_installed_skill, update_installed_skill_ex,
    update_outdated_skills,
};
pub use usage::{
    curate_report, curate_report_at, last_loaded_at, record_skill_load, skill_usage_path,
};
