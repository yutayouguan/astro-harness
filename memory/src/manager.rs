//! 记忆管理器：聚合 Markdown 记忆与会话存储，供 Agent Prompt 与记忆工具调用。
//!
//! [`MemoryManager`] 绑定单个 Agent 工作区，统一管理 `MEMORY.md`、`USER.md`、每日记忆、
//! 以及单库 [`SessionStore`]。对外提供 Prompt 内容读取、消息记录、上下文构建及
//! `memory_*` / `session_search` 工具分发。

use std::path::PathBuf;

use crate::build_conversation_context;
use crate::files::MemoryFile;
use crate::message_db::ScrolledMessage;
use crate::session_store::{NewMessage, RecentSession, SearchHit, SessionStore};
use crate::workspace::{
    active_agent_id, daily_memory_path, ensure_daily_memory, today_date_string, DEFAULT_AGENT_ID,
};

/// 记忆写入/替换/删除的目标存储位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryTarget {
    /// `MEMORY.md` — 长期精炼记忆（上限 8000 字符）。
    Project,
    /// `USER.md` — 用户档案（上限 4000 字符）。
    User,
    /// `mermaid/YYYY-MM-DD.md` — 按日滚动的每日记忆。
    Daily,
}

/// 单 Agent 记忆子系统的聚合入口。
///
/// 构造时会确保工作区与 Agent 空间存在；`session_store` 位于
/// `{base_dir}/sessions/state.db`，为所有 Agent 共享路径（按 `session_id` 隔离）。
pub struct MemoryManager {
    /// Astro 数据根目录（通常为 `~/.astro`）。
    pub base_dir: PathBuf,
    /// 当前绑定的 Agent 标识（已规范化）。
    pub agent_id: String,
    /// Agent 工作区目录，含 `MEMORY.md`、`USER.md`、`mermaid/` 等。
    pub workspace_dir: PathBuf,
    /// 长期记忆文件句柄。
    pub memory: MemoryFile,
    /// 用户档案文件句柄。
    pub user: MemoryFile,
    /// 单库会话存储（`state.db`，含 sessions / messages / FTS）。
    pub session_store: SessionStore,
}

impl MemoryManager {
    /// 以 `base_dir` 下当前活跃 Agent 构造管理器。
    ///
    /// 活跃 Agent 由 `active_agent_id` 决定；空或缺失时回退默认 Agent。
    pub fn new(base_dir: PathBuf) -> anyhow::Result<Self> {
        crate::workspace::ensure_workspace(&base_dir)?;
        let agent_id = active_agent_id(&base_dir);
        Self::for_agent(base_dir, &agent_id)
    }

    /// 为指定 `agent_id` 构造管理器；空白 id 回退 [`DEFAULT_AGENT_ID`]。
    ///
    /// 会创建 Agent 工作区（若不存在）并打开/迁移会话库。
    pub fn for_agent(base_dir: PathBuf, agent_id: &str) -> anyhow::Result<Self> {
        crate::workspace::ensure_workspace(&base_dir)?;
        let id = if agent_id.trim().is_empty() {
            DEFAULT_AGENT_ID.to_string()
        } else {
            crate::workspace::normalize_agent_id(agent_id)
        };
        let workspace = crate::workspace::ensure_agent_space(&base_dir, &id, None)?;
        let sessions_dir = base_dir.join("sessions");
        Ok(MemoryManager {
            base_dir,
            agent_id: id,
            workspace_dir: workspace.clone(),
            memory: MemoryFile::new(workspace.join("MEMORY.md"), 8000),
            user: MemoryFile::new(workspace.join("USER.md"), 4000),
            session_store: SessionStore::open_with_legacy_migration(&sessions_dir)?,
        })
    }

    /// 今日每日记忆文件路径（必要时创建）
    pub fn today_daily_path(&self) -> anyhow::Result<PathBuf> {
        let date = today_date_string();
        ensure_daily_memory(&self.workspace_dir, &date)
    }

    /// 读取今日每日记忆全文（供 Prompt 注入）
    pub fn daily_content_today(&self) -> String {
        let date = today_date_string();
        let path = daily_memory_path(&self.workspace_dir, &date);
        std::fs::read_to_string(path).unwrap_or_default()
    }

    /// 向每日记忆追加一段（按日期，默认今天）
    pub fn append_daily(&self, entry: &str, date: Option<&str>) -> anyhow::Result<String> {
        let date = date
            .map(|s| s.to_string())
            .unwrap_or_else(today_date_string);
        let path = ensure_daily_memory(&self.workspace_dir, &date)?;
        let mut content = std::fs::read_to_string(&path).unwrap_or_default();
        if !content.ends_with('\n') && !content.is_empty() {
            content.push('\n');
        }
        let line = entry.trim();
        if line.starts_with('-') || line.starts_with('*') {
            content.push_str(line);
        } else {
            content.push_str("- ");
            content.push_str(line);
        }
        content.push('\n');
        std::fs::write(&path, &content)?;
        Ok(format!("已写入每日记忆 mermaid/{date}.md"))
    }

    /// 返回 `(长期记忆, 用户档案)` 的 Markdown 列表文本，供 Prompt 注入。
    pub fn prompt_content(&self) -> (String, String) {
        (self.memory.content(), self.user.content())
    }

    /// (长期记忆, 用户档案, 今日每日记忆)
    pub fn prompt_content_with_daily(&self) -> (String, String, String) {
        (
            self.memory.content(),
            self.user.content(),
            self.daily_content_today(),
        )
    }

    /// 确保会话行存在（不存在则按 `source` 创建）。
    pub fn ensure_session(&self, session_id: &str, source: &str) -> anyhow::Result<()> {
        self.session_store.ensure_session(session_id, source)
    }

    /// 向消息库插入一条会话消息，返回自增 `id`。
    ///
    /// 薄封装：自动 `ensure_session(..., "tauri")` 后委托 [`SessionStore::append_message`]。
    pub fn record_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
    ) -> anyhow::Result<i64> {
        self.ensure_session(session_id, "tauri")?;
        self.session_store.append_message(NewMessage {
            content: Some(content),
            ..NewMessage::empty(session_id, role)
        })
    }

    /// 写入富消息行（含 tool / reasoning 等字段）；调用方需先 [`ensure_session`]。
    pub fn record_message_ex(
        &self,
        _session_id: &str,
        msg: NewMessage<'_>,
    ) -> anyhow::Result<i64> {
        self.session_store.append_message(msg)
    }

    /// 构建会话上下文：最近 `recent_turns` 条 + 可选 FTS 关键词召回窗口。
    ///
    /// 委托 [`build_conversation_context`]；`fts_keywords` 为 `None` 时仅取最近消息。
    pub fn build_session_context(
        &self,
        session_id: &str,
        recent_turns: usize,
        fts_keywords: Option<&str>,
    ) -> anyhow::Result<Vec<ScrolledMessage>> {
        build_conversation_context(
            &self.session_store,
            session_id,
            recent_turns,
            fts_keywords,
        )
    }

    /// 按 `started_at` 降序列出近期会话（供侧栏等后续任务使用）。
    pub fn list_recent_sessions(&self, limit: usize) -> anyhow::Result<Vec<RecentSession>> {
        self.session_store.list_recent_sessions(limit)
    }

    /// 向指定目标追加一条记忆；返回面向用户的中文操作结果。
    pub fn handle_memory_add(
        &mut self,
        entry: &str,
        target: MemoryTarget,
    ) -> anyhow::Result<String> {
        match target {
            MemoryTarget::Project => {
                self.memory.add(entry)?;
                Ok(format!(
                    "已写入长期记忆 MEMORY.md（{}/8000 字符）",
                    self.memory.current_chars()
                ))
            }
            MemoryTarget::User => {
                self.user.add(entry)?;
                Ok(format!(
                    "已写入用户档案（{}/4000 字符）",
                    self.user.current_chars()
                ))
            }
            MemoryTarget::Daily => self.append_daily(entry, None),
        }
    }

    /// 在指定目标中替换首次匹配 `old_text` 的片段；每日记忆按全文 `replacen` 处理。
    pub fn handle_memory_replace(
        &mut self,
        old_text: &str,
        new_text: &str,
        target: MemoryTarget,
    ) -> anyhow::Result<String> {
        match target {
            MemoryTarget::Project => {
                self.memory.replace(old_text, new_text)?;
                Ok("长期记忆已更新".to_string())
            }
            MemoryTarget::User => {
                self.user.replace(old_text, new_text)?;
                Ok("用户档案已更新".to_string())
            }
            MemoryTarget::Daily => {
                let date = today_date_string();
                let path = ensure_daily_memory(&self.workspace_dir, &date)?;
                let content = std::fs::read_to_string(&path)?;
                if !content.contains(old_text) {
                    anyhow::bail!("每日记忆中未找到匹配文本");
                }
                let updated = content.replacen(old_text, new_text, 1);
                std::fs::write(&path, updated)?;
                Ok(format!("每日记忆 mermaid/{date}.md 已更新"))
            }
        }
    }

    /// 从指定目标删除包含 `text` 的条目；每日记忆按全文 `replacen` 删除首次匹配。
    pub fn handle_memory_remove(
        &mut self,
        text: &str,
        target: MemoryTarget,
    ) -> anyhow::Result<String> {
        match target {
            MemoryTarget::Project => {
                self.memory.remove(text)?;
                Ok("长期记忆已删除".to_string())
            }
            MemoryTarget::User => {
                self.user.remove(text)?;
                Ok("用户档案已删除".to_string())
            }
            MemoryTarget::Daily => {
                let date = today_date_string();
                let path = ensure_daily_memory(&self.workspace_dir, &date)?;
                let content = std::fs::read_to_string(&path)?;
                if !content.contains(text) {
                    anyhow::bail!("每日记忆中未找到匹配文本");
                }
                let updated = content.replacen(text, "", 1);
                std::fs::write(&path, updated)?;
                Ok(format!("已从每日记忆 mermaid/{date}.md 删除"))
            }
        }
    }

    /// FTS 检索历史消息，格式化为 Markdown 列表；无结果时返回提示文案。
    ///
    /// 最多展示 `limit` 条；正文取 snippet（或邻接 context）。
    pub fn handle_session_search(&self, query: &str, limit: usize) -> anyhow::Result<String> {
        let hits = self
            .session_store
            .search_messages(query, None, None, limit as i64)?;
        Ok(format_session_search_hits(&hits))
    }
}

/// 将 [`SearchHit`] 列表格式化为「相关历史消息」Markdown。
fn format_session_search_hits(hits: &[SearchHit]) -> String {
    if hits.is_empty() {
        return "未找到相关历史消息".to_string();
    }

    let body = hits
        .iter()
        .map(|h| {
            let text = if h.snippet.trim().is_empty() {
                h.context.as_str()
            } else {
                h.snippet.as_str()
            };
            format!("- [{}] {}", h.session_id, text)
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("## 相关历史消息\n{body}")
}

/// 将召回消息列表格式化为 `[id] role: content [anchor]` 多行文本。
///
/// `is_anchor` 为 true 时在行尾附加 ` [anchor]` 标记，供 LLM 识别 FTS 锚点。
pub fn format_recalled_context(messages: &[ScrolledMessage]) -> String {
    if messages.is_empty() {
        return String::new();
    }

    messages
        .iter()
        .map(|m| {
            let marker = if m.is_anchor { " [anchor]" } else { "" };
            format!("[{}] {}: {}{}", m.id, m.role, m.content, marker)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 将工具参数 `target` 字符串解析为 [`MemoryTarget`]；未知或缺失时默认 `Project`。
///
/// 接受 `"user"`、`"daily"`、`"mermaid"`（后二者均映射为每日记忆）。
fn parse_memory_target(value: Option<&str>) -> MemoryTarget {
    match value {
        Some("user") => MemoryTarget::User,
        Some("daily") | Some("mermaid") => MemoryTarget::Daily,
        _ => MemoryTarget::Project,
    }
}

/// 记忆相关 Agent 工具的统一分发入口。
///
/// 支持 `memory_add`、`memory_replace`、`memory_remove`、`session_search`；
/// 参数从 `args` JSON 提取，`target` 经 [`parse_memory_target`] 解析。未知工具名报错。
pub fn dispatch_memory_tool(
    memory: &mut MemoryManager,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    match name {
        "memory_add" => {
            let entry = args["entry"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("缺少 entry 参数"))?;
            let target = parse_memory_target(args["target"].as_str());
            memory.handle_memory_add(entry, target)
        }
        "memory_replace" => {
            let old_text = args["old_text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("缺少 old_text 参数"))?;
            let new_text = args["new_text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("缺少 new_text 参数"))?;
            let target = parse_memory_target(args["target"].as_str());
            memory.handle_memory_replace(old_text, new_text, target)
        }
        "memory_remove" => {
            let text = args["text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("缺少 text 参数"))?;
            let target = parse_memory_target(args["target"].as_str());
            memory.handle_memory_remove(text, target)
        }
        "session_search" => {
            let query = args["query"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("缺少 query 参数"))?;
            let limit = args["limit"].as_u64().unwrap_or(5) as usize;
            memory.handle_session_search(query, limit)
        }
        _ => anyhow::bail!("未知记忆工具: {name}"),
    }
}
