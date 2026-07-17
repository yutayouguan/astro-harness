//! Agno 风格 MemoryStore 协议：list / search / propose_write / apply。
//!
//! 不改变 Markdown + 审批产品形态；仅把现有 [`MemoryStore`] 与
//! [`crate::pending`] 路径收成统一接口，供工具 / review / 入梦复用。

use crate::config::load_memory_config;
use crate::pending::{self, PendingMemoryWrite};
use crate::{MemoryStore, MemoryTarget, MemoryWriteResult};
use std::path::{Path, PathBuf};

/// 协议级写入意图。
#[derive(Debug, Clone)]
pub struct MemoryWriteIntent {
    pub target: MemoryTarget,
    /// `add` | `replace` | `remove` | `replace_all`
    pub action: String,
    pub content: Option<String>,
    pub old_text: Option<String>,
    pub source: String,
    pub agent_id: String,
}

/// Propose 或直接 apply 的结果。
#[derive(Debug)]
pub enum MemoryOpsResult {
    Applied(MemoryWriteResult),
    Proposed { pending_id: String },
}

/// 记忆读写协议（对齐 Agno MemoryManager 策略挂点）。
pub trait MemoryOps {
    fn list_entries(&self) -> &[String];
    fn search(&self, query: &str) -> Vec<String>;
    fn propose_write(&self, intent: &MemoryWriteIntent) -> anyhow::Result<String>;
    fn apply(&mut self, intent: &MemoryWriteIntent) -> anyhow::Result<MemoryOpsResult>;
}

/// 单文件 [`MemoryStore`] 上的协议实现；`propose_write` 走 pending 目录。
pub struct FileMemoryOps {
    store: MemoryStore,
    base_dir: PathBuf,
    target: MemoryTarget,
}

impl FileMemoryOps {
    pub fn open(
        path: PathBuf,
        max_chars: usize,
        base_dir: PathBuf,
        target: MemoryTarget,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            store: MemoryStore::open(path, max_chars)?,
            base_dir,
            target,
        })
    }

    pub fn store(&self) -> &MemoryStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut MemoryStore {
        &mut self.store
    }

    /// `write_approval` 开启时 propose，否则 apply。
    pub fn propose_or_apply(
        &mut self,
        intent: &MemoryWriteIntent,
    ) -> anyhow::Result<MemoryOpsResult> {
        let cfg = load_memory_config(&self.base_dir);
        if cfg.write_approval {
            let id = self.propose_write(intent)?;
            Ok(MemoryOpsResult::Proposed { pending_id: id })
        } else {
            self.apply(intent)
        }
    }
}

impl MemoryOps for FileMemoryOps {
    fn list_entries(&self) -> &[String] {
        self.store.live_entries()
    }

    fn search(&self, query: &str) -> Vec<String> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.store.live_entries().to_vec();
        }
        self.store
            .live_entries()
            .iter()
            .filter(|e| e.to_lowercase().contains(&q))
            .cloned()
            .collect()
    }

    fn propose_write(&self, intent: &MemoryWriteIntent) -> anyhow::Result<String> {
        let pending = pending::enqueue(
            &self.base_dir,
            PendingMemoryWrite {
                id: String::new(),
                agent_id: intent.agent_id.clone(),
                target: intent.target,
                action: intent.action.clone(),
                content: intent.content.clone(),
                old_text: intent.old_text.clone(),
                source: intent.source.clone(),
                created_at: String::new(),
            },
        )?;
        Ok(pending.id)
    }

    fn apply(&mut self, intent: &MemoryWriteIntent) -> anyhow::Result<MemoryOpsResult> {
        if intent.target != self.target {
            anyhow::bail!(
                "MemoryOps target mismatch: {:?} vs {:?}",
                intent.target,
                self.target
            );
        }
        match intent.action.as_str() {
            "add" => {
                let content = intent
                    .content
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("add 需要 content"))?;
                Ok(MemoryOpsResult::Applied(self.store.add(content)?))
            }
            "replace" => {
                let old = intent
                    .old_text
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("replace 需要 old_text"))?;
                let new = intent
                    .content
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("replace 需要 content"))?;
                Ok(MemoryOpsResult::Applied(self.store.replace(old, new)?))
            }
            "remove" => {
                let old = intent
                    .old_text
                    .as_deref()
                    .or(intent.content.as_deref())
                    .ok_or_else(|| anyhow::anyhow!("remove 需要 old_text 或 content"))?;
                Ok(MemoryOpsResult::Applied(self.store.remove(old)?))
            }
            "replace_all" => {
                let content = intent.content.as_deref().unwrap_or("");
                let entries = crate::parse_memory_entries(content);
                self.store.replace_all_entries(entries)?;
                Ok(MemoryOpsResult::Applied(MemoryWriteResult {
                    message: "已整页替换记忆".into(),
                    usage: format!("{}/?", self.store.current_chars()),
                    duplicate: false,
                }))
            }
            other => anyhow::bail!("未知 memory action: {other}"),
        }
    }
}

/// 便于测试：从临时目录构造 MEMORY 目标 ops。
pub fn open_memory_ops_for_test(base: &Path, max_chars: usize) -> anyhow::Result<FileMemoryOps> {
    let path = base.join("MEMORY.md");
    FileMemoryOps::open(path, max_chars, base.to_path_buf(), MemoryTarget::Memory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::set_write_approval;
    use tempfile::TempDir;

    #[test]
    fn apply_add_without_approval() {
        let dir = TempDir::new().unwrap();
        set_write_approval(dir.path(), false).unwrap();
        let mut ops = open_memory_ops_for_test(dir.path(), 2000).unwrap();
        let r = ops
            .propose_or_apply(&MemoryWriteIntent {
                target: MemoryTarget::Memory,
                action: "add".into(),
                content: Some("hello protocol".into()),
                old_text: None,
                source: "test".into(),
                agent_id: "workspace".into(),
            })
            .unwrap();
        match r {
            MemoryOpsResult::Applied(w) => assert!(!w.duplicate),
            other => panic!("expected Applied, got {other:?}"),
        }
        assert_eq!(ops.search("protocol").len(), 1);
    }

    #[test]
    fn propose_when_write_approval() {
        let dir = TempDir::new().unwrap();
        set_write_approval(dir.path(), true).unwrap();
        let mut ops = open_memory_ops_for_test(dir.path(), 2000).unwrap();
        let r = ops
            .propose_or_apply(&MemoryWriteIntent {
                target: MemoryTarget::Memory,
                action: "add".into(),
                content: Some("pending entry".into()),
                old_text: None,
                source: "test".into(),
                agent_id: "workspace".into(),
            })
            .unwrap();
        match r {
            MemoryOpsResult::Proposed { pending_id } => {
                assert!(!pending_id.is_empty());
                let list = pending::list_pending(dir.path()).unwrap();
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].id, pending_id);
            }
            other => panic!("expected Proposed, got {other:?}"),
        }
        assert!(ops.list_entries().is_empty());
    }
}
