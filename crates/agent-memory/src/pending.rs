//! `write_approval` 写入审批队列：MEMORY/USER 变更先入盘上 pending，批准后才改 live。
//!
//! 路径：`{base}/pending/memory/{id}.json`
//!
//! - 扫描失败**永不**入队
//! - 日记 [`crate::MemoryManager::append_daily`] 不受本门禁约束

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::load_memory_config;
use crate::parse_memory_entries;
use crate::MemoryManager;
use crate::MemoryStore;
use crate::MemoryTarget;
use home::scan_memory_content;

/// 一条待审批的记忆写入。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingMemoryWrite {
    pub id: String,
    pub agent_id: String,
    #[serde(with = "memory_target_serde")]
    pub target: MemoryTarget,
    /// `add` | `replace` | `remove` | `replace_all`（入梦整页）
    pub action: String,
    pub content: Option<String>,
    pub old_text: Option<String>,
    /// `"tool"` | `"review"` | `"dreaming"`
    pub source: String,
    pub created_at: String,
}

mod memory_target_serde {
    use super::MemoryTarget;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(t: &MemoryTarget, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_str(match t {
            MemoryTarget::Memory => "memory",
            MemoryTarget::User => "user",
        })
    }

    pub fn deserialize<'de, D>(d: D) -> Result<MemoryTarget, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(d)?;
        match raw.as_str() {
            "memory" => Ok(MemoryTarget::Memory),
            "user" => Ok(MemoryTarget::User),
            other => Err(serde::de::Error::custom(format!(
                "unknown memory target: {other}"
            ))),
        }
    }
}

/// `{base}/memory/pending`
pub fn pending_dir(base: &Path) -> PathBuf {
    base.join("memory").join("pending")
}

fn pending_path(base: &Path, id: &str) -> PathBuf {
    pending_dir(base).join(format!("{id}.json"))
}

/// 扫描 `add`/`replace`/`replace_all` 的内容；失败则不入队。
fn scan_for_enqueue(action: &str, content: Option<&str>) -> anyhow::Result<()> {
    match action {
        "add" | "replace" => {
            let c = content
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("缺少 content 参数"))?;
            scan_memory_content(c).map_err(|e| anyhow::anyhow!(e))?;
        }
        "replace_all" => {
            let c = content
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("缺少 content 参数"))?;
            let entries = parse_memory_entries(c);
            if entries.is_empty() {
                anyhow::bail!("未能从 MEMORY 输出中解析出任何条目");
            }
            for entry in &entries {
                scan_memory_content(entry).map_err(|e| anyhow::anyhow!(e))?;
            }
        }
        "remove" => {}
        other => anyhow::bail!("未知 memory action: {other}"),
    }
    Ok(())
}

/// 将写入请求入队（会先扫描；扫描失败则 Err 且不落盘）。
///
/// 若 `id` / `created_at` 为空则自动生成。
pub fn enqueue(base: &Path, mut item: PendingMemoryWrite) -> anyhow::Result<PendingMemoryWrite> {
    scan_for_enqueue(item.action.as_str(), item.content.as_deref())?;

    if item.id.trim().is_empty() {
        item.id = Uuid::new_v4().to_string();
    }
    if item.created_at.trim().is_empty() {
        item.created_at = Utc::now().to_rfc3339();
    }

    let dir = pending_dir(base);
    fs::create_dir_all(&dir)?;
    let path = pending_path(base, &item.id);
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(&item)?;
    fs::write(&tmp, raw)?;
    fs::rename(&tmp, &path)?;
    Ok(item)
}

/// 列出全部 pending（按 `created_at` 升序）。
pub fn list_pending(base: &Path) -> anyhow::Result<Vec<PendingMemoryWrite>> {
    let dir = pending_dir(base);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path)?;
        match serde_json::from_str::<PendingMemoryWrite>(&raw) {
            Ok(item) => out.push(item),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "skipping unreadable pending memory write"
                );
            }
        }
    }
    out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(out)
}

fn load_pending(base: &Path, id: &str) -> anyhow::Result<PendingMemoryWrite> {
    let path = pending_path(base, id);
    if !path.is_file() {
        anyhow::bail!("pending 写入不存在: {id}");
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

fn remove_pending_file(base: &Path, id: &str) -> anyhow::Result<()> {
    let path = pending_path(base, id);
    if path.is_file() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// 批准并应用一条 pending（绕过 `write_approval`，直接写 live）。
pub fn approve(base: &Path, id: &str) -> anyhow::Result<String> {
    let item = load_pending(base, id)?;
    let msg = apply_pending(base, &item)?;
    remove_pending_file(base, id)?;
    Ok(msg)
}

/// 拒绝并丢弃一条 pending（不改 live）。
pub fn reject(base: &Path, id: &str) -> anyhow::Result<()> {
    let path = pending_path(base, id);
    if !path.is_file() {
        anyhow::bail!("pending 写入不存在: {id}");
    }
    // 先读摘要再删，便于 DecisionLog
    let summary = fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<PendingMemoryWrite>(&raw).ok())
        .map(|item| {
            let target = match item.target {
                MemoryTarget::Memory => "memory",
                MemoryTarget::User => "user",
            };
            format!(
                "rejected memory {} {} for agent {}",
                item.action, target, item.agent_id
            )
        })
        .unwrap_or_else(|| format!("rejected pending {id}"));
    fs::remove_file(&path)?;
    crate::decision_log::try_append_decision(
        base,
        crate::decision_log::DecisionEntry::new(
            crate::decision_log::DecisionKind::MemoryRejected,
            summary,
        ),
    );
    Ok(())
}

fn apply_pending(base: &Path, item: &PendingMemoryWrite) -> anyhow::Result<String> {
    if item.action == "replace_all" {
        return apply_replace_all(base, item);
    }

    let mut mgr = MemoryManager::for_agent(base.to_path_buf(), &item.agent_id)?;
    // apply_direct 绕过 write_approval，避免再次入队
    mgr.apply_memory_op_direct(
        &item.action,
        item.target,
        item.content.as_deref(),
        item.old_text.as_deref(),
    )
}

fn apply_replace_all(base: &Path, item: &PendingMemoryWrite) -> anyhow::Result<String> {
    let content = item
        .content
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("replace_all 缺少 content"))?;
    let entries = parse_memory_entries(content);
    if entries.is_empty() {
        anyhow::bail!("未能从 MEMORY 输出中解析出任何条目");
    }
    let cfg = load_memory_config(base);
    let mgr = MemoryManager::for_agent(base.to_path_buf(), &item.agent_id)?;
    match item.target {
        MemoryTarget::Memory => {
            let mut store =
                MemoryStore::open(mgr.workspace_dir.join("MEMORY.md"), cfg.memory_char_limit)?;
            store.replace_all_entries(entries)?;
            Ok("已批准并写入长期记忆 MEMORY.md（整页替换）".into())
        }
        MemoryTarget::User => {
            let mut store =
                MemoryStore::open(mgr.workspace_dir.join("USER.md"), cfg.user_char_limit)?;
            store.replace_all_entries(entries)?;
            Ok("已批准并写入用户档案 USER.md（整页替换）".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn reject_missing_id_errors() {
        let dir = tempdir().unwrap();
        let err = reject(dir.path(), "nope").unwrap_err().to_string();
        assert!(err.contains("不存在"));
    }
}
