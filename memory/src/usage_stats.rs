//! 按 Agent 聚合的工具集与技能调用计数。
//!
//! 统计数据持久化于 `~/.astro/agents/{id}/usage-stats.json`，供前端展示用量摘要。
//! 工具名经 [`tool_name_to_toolset`](crate::tools_enabled::tool_name_to_toolset) 归并为工具集计数；
//! `skills` 工具若携带 `skill_id` 参数则额外累加技能维度。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::tools_enabled::tool_name_to_toolset;
use crate::workspace::{
    agent_config_dir, default_memory_dir, ensure_default_workspace, normalize_agent_id,
    DEFAULT_AGENT_ID,
};

/// 单 Agent 的工具集与技能调用计数快照（可序列化）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentUsageStats {
    /// 工具集 id → 调用次数（与前端 `AGENT_TOOLS` / `tools-enabled` 对齐）。
    #[serde(default)]
    pub tools: HashMap<String, u64>,
    /// 技能名 → 调用次数（仅 `skills` 工具且带 `skill_id` 时累加）。
    #[serde(default)]
    pub skills: HashMap<String, u64>,
}

/// 面向 API 的用量摘要，含合计字段与 Agent 标识。
#[derive(Debug, Clone, Serialize)]
pub struct AgentUsageSummary {
    /// 目标 Agent 标识（已规范化）。
    pub agent_id: String,
    /// 全部工具集调用次数之和。
    pub tool_total: u64,
    /// 全部技能调用次数之和。
    pub skill_total: u64,
    /// 各工具集明细计数。
    pub tools: HashMap<String, u64>,
    /// 各技能明细计数。
    pub skills: HashMap<String, u64>,
}

impl AgentUsageStats {
    /// 返回 `tools` 映射中所有计数的总和。
    pub fn tool_total(&self) -> u64 {
        self.tools.values().copied().sum()
    }

    /// 返回 `skills` 映射中所有计数的总和。
    pub fn skill_total(&self) -> u64 {
        self.skills.values().copied().sum()
    }

    /// 转换为带合计字段的 [`AgentUsageSummary`]，消耗 `self`。
    pub fn into_summary(self, agent_id: String) -> AgentUsageSummary {
        AgentUsageSummary {
            tool_total: self.tool_total(),
            skill_total: self.skill_total(),
            tools: self.tools,
            skills: self.skills,
            agent_id,
        }
    }
}

/// 返回指定 Agent 的 `usage-stats.json` 路径。
fn usage_path(agent_id: &str) -> PathBuf {
    let base = default_memory_dir();
    let id = normalize_agent_id(agent_id);
    agent_config_dir(&base, &id).join("usage-stats.json")
}

/// 规范化 Agent 键：`None`、空白、`"default"` 均映射为 [`DEFAULT_AGENT_ID`]。
fn normalize_key(agent_id: Option<&str>) -> String {
    match agent_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) if id == "default" => DEFAULT_AGENT_ID.to_string(),
        Some(id) => normalize_agent_id(id),
        None => DEFAULT_AGENT_ID.to_string(),
    }
}

/// 从磁盘加载用量统计；文件不存在或解析失败时返回默认值。
pub fn load_usage_stats(agent_id: Option<&str>) -> AgentUsageStats {
    let id = normalize_key(agent_id);
    let path = usage_path(&id);
    if !path.exists() {
        return AgentUsageStats::default();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// 原子写入用量统计（临时文件 + `rename`）。
pub fn save_usage_stats(agent_id: Option<&str>, stats: &AgentUsageStats) -> anyhow::Result<()> {
    let _ = ensure_default_workspace()?;
    let id = normalize_key(agent_id);
    let path = usage_path(&id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(stats)?)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// 加载并转换为 [`AgentUsageSummary`]，供前端/API 直接返回。
pub fn get_usage_summary(agent_id: Option<&str>) -> AgentUsageSummary {
    let id = normalize_key(agent_id);
    load_usage_stats(Some(&id)).into_summary(id)
}

/// 记录一次工具调用并写盘；进程内通过互斥锁串行化，避免并发丢计数。
///
/// 工具名归并为工具集后 `tools` 计数 +1；若为 `skills` 且 `args.skill_id` 非空，
/// 则对应 `skills` 条目亦 +1。
pub fn record_tool_call(
    agent_id: &str,
    tool_name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<()> {
    // 串行化写盘，避免并发丢计数
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let id = normalize_key(Some(agent_id));
    let mut stats = load_usage_stats(Some(&id));
    let toolset = tool_name_to_toolset(tool_name).to_string();
    *stats.tools.entry(toolset).or_insert(0) += 1;

    if tool_name == "skills" {
        if let Some(skill_id) = args
            .get("skill_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            *stats.skills.entry(skill_id.to_string()).or_insert(0) += 1;
        }
    }

    save_usage_stats(Some(&id), &stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    /// 串行化依赖 `ASTRO_MEMORY_DIR` 的用例，避免并行污染。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn records_toolset_and_skill_counts() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        record_tool_call(
            "workspace",
            "skills",
            &json!({ "skill_id": "demo-skill", "input": {} }),
        )
        .unwrap();
        record_tool_call("workspace", "skills", &json!({ "skill_id": "demo-skill" })).unwrap();
        record_tool_call("workspace", "web_search", &json!({ "query": "hi" })).unwrap();

        let summary = get_usage_summary(Some("workspace"));
        assert_eq!(summary.tools.get("skills").copied().unwrap_or(0), 2);
        assert_eq!(summary.tools.get("web_search").copied().unwrap_or(0), 1);
        assert_eq!(summary.skills.get("demo-skill").copied().unwrap_or(0), 2);
        assert_eq!(summary.skill_total, 2);
        assert_eq!(summary.tool_total, 3);

        std::env::remove_var("ASTRO_MEMORY_DIR");
    }

    #[test]
    fn skills_without_skill_id_only_bumps_toolset() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        record_tool_call("workspace", "skills", &json!({ "skill_id": "  " })).unwrap();
        let summary = get_usage_summary(Some("workspace"));
        assert_eq!(summary.tools.get("skills").copied().unwrap_or(0), 1);
        assert!(summary.skills.is_empty());
        assert_eq!(summary.skill_total, 0);

        std::env::remove_var("ASTRO_MEMORY_DIR");
    }
}
