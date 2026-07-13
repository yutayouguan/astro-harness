//! Agent 工作区路径解析与规范化。

use std::fs;
use std::path::{Path, PathBuf};

/// 默认 Agent 的 id / 目录名：`~/.astro/workspace`
pub const DEFAULT_AGENT_ID: &str = "workspace";

/// 当前激活 Agent 的持久化文件名（位于数据根目录）
pub(crate) const ACTIVE_AGENT_FILE: &str = "active-agent.json";

/// 默认记忆/工作空间根目录：`$ASTRO_MEMORY_DIR` 或 `~/.astro`
pub fn default_memory_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ASTRO_MEMORY_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".astro"))
        .unwrap_or_else(|_| PathBuf::from(".astro"))
}

/// 解析 Agent 工作区路径。
///
/// - `workspace`（默认）→ `{base}/workspace`
/// - 其他 id → `{base}/workspace-{id}`
pub fn agent_workspace_dir(base: &Path, agent_id: &str) -> PathBuf {
    let id = normalize_agent_id(agent_id);
    if id == DEFAULT_AGENT_ID {
        base.join(DEFAULT_AGENT_ID)
    } else {
        base.join(format!("workspace-{id}"))
    }
}

/// Agent 配置目录：`{base}/agents/{id}/`（模型、工具、MCP 等，不含工作区文件）
pub fn agent_config_dir(base: &Path, agent_id: &str) -> PathBuf {
    let id = normalize_agent_id(agent_id);
    base.join("agents").join(id)
}

/// 从工作区目录名解析 agent id（`workspace` / `workspace-xxx`）
pub fn agent_id_from_workspace_dir_name(name: &str) -> Option<String> {
    if name == DEFAULT_AGENT_ID {
        return Some(DEFAULT_AGENT_ID.to_string());
    }
    name.strip_prefix("workspace-")
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// 返回当前激活 Agent 的工作区（缺省 `workspace`）
pub fn default_agent_workspace_dir() -> PathBuf {
    let base = default_memory_dir();
    let id = active_agent_id(&base);
    agent_workspace_dir(&base, &id)
}

/// 兼容旧单参调用：等价于 `agent_workspace_dir(base, DEFAULT_AGENT_ID)`
pub fn default_workspace_dir(base: &Path) -> PathBuf {
    agent_workspace_dir(base, DEFAULT_AGENT_ID)
}

/// 规范化 agent id：小写、空格转 `-`，仅保留 `[a-z0-9_-]`
/// 纯非 ASCII 名称（如中文）用 `agent-{hash}` 兜底，避免落到默认 `workspace`
pub fn normalize_agent_id(raw: &str) -> String {
    let s = raw.trim().to_lowercase().replace(' ', "-");
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let cleaned = cleaned
        .trim_matches('-')
        .trim_matches('_')
        .to_string();
    if cleaned.is_empty() {
        if raw.trim().is_empty() {
            return DEFAULT_AGENT_ID.to_string();
        }
        let mut hash: u32 = 2166136261;
        for b in raw.trim().bytes() {
            hash ^= u32::from(b);
            hash = hash.wrapping_mul(16777619);
        }
        return format!("agent-{:x}", hash);
    }
    cleaned
}

/// 读取当前激活的 Agent id（缺省为 `workspace`）
pub fn active_agent_id(base: &Path) -> String {
    let path = base.join(ACTIVE_AGENT_FILE);
    let Ok(text) = fs::read_to_string(&path) else {
        return DEFAULT_AGENT_ID.to_string();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return DEFAULT_AGENT_ID.to_string();
    };
    v.get("id")
        .and_then(|x| x.as_str())
        .map(normalize_agent_id)
        .filter(|id| agent_workspace_dir(base, id).is_dir())
        .unwrap_or_else(|| DEFAULT_AGENT_ID.to_string())
}

/// 设置当前激活的 Agent（目标工作区必须已存在）
pub fn set_active_agent(base: &Path, agent_id: &str) -> anyhow::Result<String> {
    let id = normalize_agent_id(agent_id);
    let dir = agent_workspace_dir(base, &id);
    if !dir.is_dir() {
        anyhow::bail!("Agent 工作区不存在: {id}");
    }
    let path = base.join(ACTIVE_AGENT_FILE);
    let json = serde_json::json!({ "id": id });
    fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&json)?))?;
    Ok(id)
}

/// 某 Agent 工作区内的日记忆路径：`mermaid/YYYY-MM-DD.md`
pub fn daily_memory_path(workspace: &Path, date: &str) -> PathBuf {
    workspace.join("mermaid").join(format!("{date}.md"))
}

/// 今日日期（本地）`YYYY-MM-DD`
pub fn today_date_string() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 列出工作区内已有的日记忆文件名（不含扩展名），新→旧
pub fn list_daily_memory_dates(workspace: &Path) -> Vec<String> {
    let dir = workspace.join("mermaid");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut dates: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("md") {
                return None;
            }
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .filter(|s| s.len() == 10 && s.chars().nth(4) == Some('-') && s.chars().nth(7) == Some('-'))
        .collect();
    dates.sort();
    dates.reverse();
    dates
}
