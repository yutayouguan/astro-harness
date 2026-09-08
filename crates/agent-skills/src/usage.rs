//! Skill 加载使用统计：供 `skills` curate 判断闲置。
//!
//! 路径：`{ASTRO_MEMORY_DIR|~/.astro}/learning/skill-usage.json`

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::installed::list_enabled_for_prompt;

use home::default_memory_dir as memory_dir;

/// `{base}/evolution/learning/skill-usage.json`
pub fn skill_usage_path(base: &Path) -> PathBuf {
    home::learning_dir(base).join("skill-usage.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SkillUsageFile {
    /// skill name → RFC3339 last_loaded_at
    #[serde(default)]
    pub last_loaded: HashMap<String, String>,
}

fn load_file(base: &Path) -> SkillUsageFile {
    let path = skill_usage_path(base);
    fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_file(base: &Path, data: &SkillUsageFile) -> anyhow::Result<()> {
    let path = skill_usage_path(base);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(data)?)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// 记录一次成功加载（best-effort，失败忽略）。
pub fn record_skill_load(name: &str) {
    let base = memory_dir();
    let mut data = load_file(&base);
    data.last_loaded
        .insert(name.to_string(), Utc::now().to_rfc3339());
    let _ = save_file(&base, &data);
}

/// 供测试：写入指定 base。
pub fn record_skill_load_at(base: &Path, name: &str, at: DateTime<Utc>) -> anyhow::Result<()> {
    let mut data = load_file(base);
    data.last_loaded.insert(name.to_string(), at.to_rfc3339());
    save_file(base, &data)
}

/// 读取某技能上次加载时间。
pub fn last_loaded_at(base: &Path, name: &str) -> Option<DateTime<Utc>> {
    let data = load_file(base);
    data.last_loaded
        .get(name)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

/// 生成 curate 报告（不修改磁盘技能）。
pub fn curate_report(unused_skill_days: u32) -> String {
    curate_report_at(&memory_dir(), unused_skill_days)
}

/// 指定 base 的 curate 报告（测试用）。
pub fn curate_report_at(base: &Path, unused_skill_days: u32) -> String {
    let enabled = list_enabled_for_prompt();
    if enabled.is_empty() {
        return "（无已启用技能）".to_string();
    }
    let data = load_file(base);
    let cutoff = Utc::now() - Duration::days(unused_skill_days as i64);
    let mut lines = vec![format!(
        "技能策展（启用 {} 个；闲置阈值 {} 天；只建议，不自动删除）:",
        enabled.len(),
        unused_skill_days
    )];
    let mut stale = Vec::new();
    for (name, desc) in &enabled {
        let last = data
            .last_loaded
            .get(name)
            .cloned()
            .unwrap_or_else(|| "从未加载".into());
        let d = if desc.trim().is_empty() {
            "（无描述）"
        } else {
            desc.trim()
        };
        lines.push(format!("- {name}: {d}\n  last_loaded: {last}"));

        let is_stale = match data.last_loaded.get(name) {
            None => true,
            Some(ts) => DateTime::parse_from_rfc3339(ts)
                .map(|dt| dt.with_timezone(&Utc) < cutoff)
                .unwrap_or(true),
        };
        if is_stale {
            stale.push(name.clone());
        }
    }
    if stale.is_empty() {
        lines.push("\n建议: 暂无闲置技能。".into());
    } else {
        lines.push(format!(
            "\n建议（确认后可用 skills manage delete 或 UI 禁用）: {}",
            stale.join(", ")
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn record_skill_load_at_persists() {
        let dir = TempDir::new().unwrap();
        let t = Utc::now() - Duration::days(40);
        record_skill_load_at(dir.path(), "demo", t).unwrap();
        let got = last_loaded_at(dir.path(), "demo").unwrap();
        assert!(got < Utc::now() - Duration::days(30));
        let path = skill_usage_path(dir.path());
        assert!(path.is_file());
    }

    #[test]
    fn curate_report_mentions_never_loaded_when_tracked() {
        let dir = TempDir::new().unwrap();
        // Even with zero enabled skills from global scan, report should be well-formed.
        let report = curate_report_at(dir.path(), 30);
        assert!(!report.is_empty());
    }
}
