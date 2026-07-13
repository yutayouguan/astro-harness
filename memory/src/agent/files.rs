//! Markdown 列表式记忆文件的读写与容量管理。
//!
//! 每条记忆以 `- ` 或 `* ` 开头的列表项存储；超出 `max_chars` 时从最早条目起逐条淘汰。
//! 持久化采用临时文件 + `rename` 的原子写入，避免写入中断导致文件损坏。

use std::fs;
use std::path::{Path, PathBuf};

/// 单文件记忆存储：维护条目列表并在写入时强制执行字符上限。
///
/// 构造时从磁盘加载已有条目；文件不存在时从空列表开始。
pub struct MemoryFile {
    /// 目标 Markdown 文件路径。
    path: PathBuf,
    /// 全部条目序列化后的最大字符数（含 `- ` 前缀与换行）。
    max_chars: usize,
    /// 已解析的纯文本条目（不含列表前缀）。
    entries: Vec<String>,
}

impl MemoryFile {
    /// 打开或初始化记忆文件。
    ///
    /// 仅解析以 `- ` 或 `* ` 开头的行；空行与无前缀行会被忽略。
    pub fn new(path: PathBuf, max_chars: usize) -> Self {
        let entries = if path.exists() {
            fs::read_to_string(&path)
                .unwrap_or_default()
                .lines()
                .filter(|line| line.starts_with("- ") || line.starts_with("* "))
                .map(|line| line[2..].trim().to_string())
                .filter(|entry| !entry.is_empty())
                .collect()
        } else {
            Vec::new()
        };
        MemoryFile {
            path,
            max_chars,
            entries,
        }
    }

    /// 当前条目序列化后的总字符数（每条按 `len + 3` 估算，含 `- ` 与换行）。
    pub fn current_chars(&self) -> usize {
        self.entries.iter().map(|e| e.len() + 3).sum()
    }

    /// 将条目格式化为 Markdown 列表文本；无条目时返回空字符串。
    pub fn content(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }
        self.entries
            .iter()
            .map(|entry| format!("- {entry}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 追加一条记忆；空白条目报错，完全重复条目静默跳过。
    ///
    /// 写入后若超出 `max_chars`，从最旧条目起删除，但至少保留一条。
    pub fn add(&mut self, entry: &str) -> anyhow::Result<()> {
        let entry = entry.trim().to_string();
        if entry.is_empty() {
            anyhow::bail!("记忆条目不能为空");
        }
        if self.entries.iter().any(|e| e == &entry) {
            return Ok(());
        }

        self.entries.push(entry);
        while self.current_chars() > self.max_chars && self.entries.len() > 1 {
            self.entries.remove(0);
        }
        self.save()
    }

    /// 将第一条包含 `old_text` 的条目全文替换为 `new_text`；未找到时报错。
    pub fn replace(&mut self, old_text: &str, new_text: &str) -> anyhow::Result<()> {
        let mut found = false;
        for entry in &mut self.entries {
            if entry.contains(old_text) {
                *entry = new_text.trim().to_string();
                found = true;
                break;
            }
        }
        if !found {
            anyhow::bail!("未找到包含 '{old_text}' 的条目");
        }
        self.save()
    }

    /// 删除所有包含 `text` 的条目；若无匹配则报错。
    pub fn remove(&mut self, text: &str) -> anyhow::Result<()> {
        let before = self.entries.len();
        self.entries.retain(|e| !e.contains(text));
        if self.entries.len() == before {
            anyhow::bail!("未找到条目");
        }
        self.save()
    }

    /// 返回底层文件路径的只读引用。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 原子写入：先写 `.md.tmp` 再 `rename` 覆盖目标文件。
    fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("md.tmp");
        fs::write(&tmp, self.content())?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
